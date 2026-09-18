//! Finishing a sign-in that happened at the identity provider.
//!
//! The browser is sent to the pool's Hosted UI with a PKCE challenge, Cognito sends it back to
//! the CMS with an authorization code, and the code has to be exchanged for tokens. That
//! exchange is a plain HTTPS form post - and it happens **here** rather than in the browser,
//! because the token endpoint answers without CORS headers. The client is public (the browser
//! cannot keep a secret), so the proof that the code came back to the same caller is the PKCE
//! verifier it generated before it left.

use std::time::{Duration, SystemTime};

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::{AppState, Storage};
use crate::models::error::HttpError;

/// What the browser sends back from the provider's redirect.
#[derive(Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
    /// The verifier whose challenge was in the sign-in URL.
    pub code_verifier: String,
    /// The address the provider was told to send the browser back to; it has to match, because
    /// the token endpoint checks it against the code it issued.
    pub redirect_uri: String,
}

/// The tokens the provider answers with, as far as the CMS cares.
#[derive(Deserialize)]
struct ProviderTokens {
    id_token: String,
    expires_in: Option<u64>,
}

/// What the browser gets: the token the CMS will verify, and when it stops working.
#[derive(serde::Serialize)]
pub struct Session {
    pub token: String,
    pub expires_at: String,
}

/// Where a deployment's token endpoint is, given the sign-in page it was told about.
///
/// Both live on the pool's own domain (`<domain>.auth.<region>.amazoncognito.com`), which is what
/// the sign-in URL names; anything else about it - the client, the response type, the scope - is
/// irrelevant here. `None` when there is nothing to derive it from.
pub fn token_endpoint(login_url: &str) -> Option<String> {
    let (scheme, rest) = login_url.split_once("://")?;
    let host = rest.split('/').next()?;
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}/oauth2/token"))
}

/// The form a token exchange is made of.
///
/// A public client sends no secret: the code and the verifier are the proof.
fn exchange_form(client_id: &str, request: &ExchangeRequest) -> Vec<(String, String)> {
    vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("client_id".to_string(), client_id.to_string()),
        ("code".to_string(), request.code.clone()),
        ("code_verifier".to_string(), request.code_verifier.clone()),
        ("redirect_uri".to_string(), request.redirect_uri.clone()),
    ]
}

/// A form body, percent-encoded.
///
/// Written out rather than taken from reqwest's `form`, because this build of reqwest has no
/// serde support compiled in - and because what needs encoding here is worth being explicit
/// about: a redirect URI is full of `:` and `/`, and a verifier is base64url.
fn form_body(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The characters a form value may keep, and percent-encoding for everything else.
fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// When a token minted now stops working, as the CMS writes timestamps.
fn expiry(expires_in: Option<u64>) -> String {
    let seconds = expires_in.unwrap_or(3600);
    let at = SystemTime::now() + Duration::from_secs(seconds);
    chrono::DateTime::<chrono::Utc>::from(at).to_rfc3339()
}

/// What the exchange route needs: which client the code was issued for, and where to ask.
///
/// A value rather than part of the shared state: only a deployment that signs users in elsewhere
/// has one, and whoever composes the router hands it to the route as a request extension.
#[derive(Clone)]
pub struct TokenEndpoint {
    pub client_id: String,
    pub endpoint: String,
    /// One client for every exchange: building one per request spins up a TLS stack each time,
    /// and this route is on the sign-in path where that is the slowest thing it does. Built on
    /// first use, because the TLS provider is installed by the request path and a client built
    /// before that would panic.
    client: std::sync::OnceLock<reqwest::Client>,
}

impl TokenEndpoint {
    /// The endpoint one deployment signs users in at.
    pub fn new(client_id: String, endpoint: String) -> Self {
        TokenEndpoint {
            client_id,
            endpoint,
            client: std::sync::OnceLock::new(),
        }
    }
}

/// Why an exchange did not produce a session.
///
/// The two are answered differently because they *are* different: the provider refusing is about
/// the code the browser brought back (401, and the reader is told nothing more - a used code, a
/// wrong verifier and an unknown client look the same from here), while not reaching it is our
/// problem (500, and the detail goes to the log rather than to the reader).
enum ExchangeFailure {
    Refused,
    Unavailable(String),
}

impl ExchangeFailure {
    fn into_http(self) -> HttpError {
        match self {
            ExchangeFailure::Refused => {
                HttpError::Unauthorized("the sign-in could not be completed")
            }
            ExchangeFailure::Unavailable(detail) => {
                tracing::warn!("the sign-in could not reach the provider: {detail}");
                crate::models::error::map_internal_error(
                    "the provider's token endpoint could not be reached",
                )
            }
        }
    }
}

impl TokenEndpoint {
    /// Ask the provider to turn the code into tokens.
    async fn exchange(&self, request: &ExchangeRequest) -> Result<Session, ExchangeFailure> {
        crate::webhook::install_crypto_provider();
        let client = self.client.get_or_init(|| {
            crate::webhook::install_crypto_provider();
            reqwest::Client::new()
        });
        let response = client
            .post(&self.endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&exchange_form(&self.client_id, request)))
            .send()
            .await
            .map_err(|e| ExchangeFailure::Unavailable(format!("could not be reached: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            // The provider's own words are worth the log, and not worth the reader: a code that
            // was already used, a wrong verifier and an unknown client all look the same from
            // here, and none of them is the reader's business to tell apart.
            let body = response.text().await.unwrap_or_default();
            tracing::warn!("the token exchange was refused ({status}): {body}");
            return Err(ExchangeFailure::Refused);
        }
        // Read as text and parse here, for the same reason the form is written by hand: this build
        // of reqwest carries no serde integration.
        let body = response.text().await.map_err(|e| {
            ExchangeFailure::Unavailable(format!("answered something unreadable: {e}"))
        })?;
        let tokens: ProviderTokens = serde_json::from_str(&body).map_err(|e| {
            ExchangeFailure::Unavailable(format!("answered something unexpected: {e}"))
        })?;
        Ok(Session {
            token: tokens.id_token,
            expires_at: expiry(tokens.expires_in),
        })
    }
}

/// Turn the code from the provider's redirect into a session.
///
/// Public, like the sign-in page: the caller has no session yet, and the code *is* the credential
/// - single-use, short-lived, and only usable with the verifier that asked for it.
async fn exchange_code(
    Extension(endpoint): Extension<TokenEndpoint>,
    Json(request): Json<ExchangeRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let session = endpoint
        .exchange(&request)
        .await
        .map_err(ExchangeFailure::into_http)?;
    Ok((StatusCode::OK, Json(session)))
}

/// The route a deployment that signs users in elsewhere answers the redirect with.
///
/// Composed only by such a deployment (see `crates/aws/src/lib.rs`): a CMS that handles passwords
/// itself has no code to exchange.
pub fn routes<R: Storage>(endpoint: TokenEndpoint) -> Router<AppState<R>> {
    Router::new()
        .route("/auth/cognito/exchange", post(exchange_code))
        .layer(Extension(endpoint))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two failures are answered differently, and neither answers with the reason.
    ///
    /// A provider that refuses is about the code the browser brought back: 401, and nothing about
    /// which of the reasons it was. A provider we could not reach is our problem, not the reader's
    /// credentials - and the detail (host, TLS, parse position) belongs in the log.
    #[tokio::test]
    async fn an_unreachable_provider_is_not_a_401() {
        let endpoint = TokenEndpoint::new(
            "abc".to_string(),
            "http://127.0.0.1:1/oauth2/token".to_string(),
        );
        let request = ExchangeRequest {
            code: "code".to_string(),
            code_verifier: "verifier".to_string(),
            redirect_uri: "http://localhost:4200/auth/callback".to_string(),
        };

        let failure = match endpoint.exchange(&request).await {
            Ok(_) => panic!("a port nothing listens on answered"),
            Err(failure) => failure,
        };
        let http = failure.into_http();
        assert_eq!(
            http.status_code, 500,
            "an outage is not the reader's credentials"
        );
        assert!(
            !http.message.contains("127.0.0.1") && !http.message.contains("oauth2"),
            "the reason leaked: {}",
            http.message
        );
    }

    #[test]
    fn a_refusal_says_only_that_it_could_not_be_completed() {
        let http = ExchangeFailure::Refused.into_http();
        assert_eq!(http.status_code, 401);
        assert_eq!(http.message, "the sign-in could not be completed");
    }

    const LOGIN: &str = "https://cms.auth.eu-west-1.amazoncognito.com/login?client_id=abc&response_type=code&scope=openid+email";

    #[test]
    fn the_token_endpoint_is_the_sign_in_pages_own_domain() {
        assert_eq!(
            token_endpoint(LOGIN).as_deref(),
            Some("https://cms.auth.eu-west-1.amazoncognito.com/oauth2/token")
        );
        assert_eq!(token_endpoint("not a url"), None);
        assert_eq!(token_endpoint("https://"), None);
    }

    /// A public client proves itself with the verifier, and the form has to say which address the
    /// code was issued for: the provider checks both against the code.
    #[test]
    fn the_exchange_form_carries_the_code_the_verifier_and_the_redirect() {
        let form = exchange_form(
            "abc",
            &ExchangeRequest {
                code: "the-code".to_string(),
                code_verifier: "the-verifier".to_string(),
                redirect_uri: "https://cms.example.com/auth/callback".to_string(),
            },
        );
        let get = |name: &str| {
            form.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(get("grant_type").as_deref(), Some("authorization_code"));
        assert_eq!(get("client_id").as_deref(), Some("abc"));
        assert_eq!(get("code").as_deref(), Some("the-code"));
        assert_eq!(get("code_verifier").as_deref(), Some("the-verifier"));
        assert_eq!(
            get("redirect_uri").as_deref(),
            Some("https://cms.example.com/auth/callback")
        );
        assert_eq!(get("client_secret"), None, "a public client has no secret");
    }

    #[test]
    fn the_form_is_percent_encoded_the_way_a_form_post_is() {
        let body = form_body(&[
            (
                "redirect_uri".to_string(),
                "https://cms.example.com/auth/callback".to_string(),
            ),
            ("code_verifier".to_string(), "abc-123_XYZ.~".to_string()),
            ("code".to_string(), "a b+c".to_string()),
        ]);
        assert_eq!(
            body,
            "redirect_uri=https%3A%2F%2Fcms.example.com%2Fauth%2Fcallback\
             &code_verifier=abc-123_XYZ.~\
             &code=a+b%2Bc"
        );
    }

    #[test]
    fn a_token_without_a_lifetime_gets_the_pools_hour() {
        let expiry = expiry(None);
        let when = chrono::DateTime::parse_from_rfc3339(&expiry).expect("a timestamp");
        let in_an_hour = chrono::Utc::now() + chrono::Duration::minutes(59);
        assert!(when > in_an_hour, "{expiry}");
    }
}
