//! Verifying tokens an identity provider issued (Cognito: RS256, keys published as JWKS).
//!
//! The CMS never sees the sign-in: the browser talks to Cognito and comes back with an ID
//! token, and all this has to do is be sure the token is Cognito's and still valid. That means
//! the signature against the published key, the issuer, the audience (the app client the token
//! was minted for), the expiry, and that it is an ID token rather than an access token.
//!
//! Getting the key set is the only part that talks to the network, so it is behind
//! [`JwksSource`]: a deployment fetches it over HTTPS, and a test hands over a document. The
//! cache is here rather than in the source because "fetch once, refresh when a key is unknown"
//! is the verifier's policy, and it is the part worth testing.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::auth::identity::{Identity, TokenVerifier, VerifyFuture};
use crate::models::error::HttpError;

/// What a deployment has to tell the verifier about its pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitoSettings {
    pub region: String,
    pub user_pool_id: String,
    /// The app client the tokens are minted for; the audience of an ID token.
    pub client_id: String,
}

impl CognitoSettings {
    /// The issuer Cognito puts in the tokens of this pool.
    pub fn issuer(&self) -> String {
        format!(
            "https://cognito-idp.{}.amazonaws.com/{}",
            self.region, self.user_pool_id
        )
    }

    /// Where the pool publishes its signing keys.
    pub fn jwks_url(&self) -> String {
        format!("{}/.well-known/jwks.json", self.issuer())
    }
}

/// Where the signing keys come from.
pub trait JwksSource: Send + Sync + 'static {
    fn fetch(&self) -> impl Future<Output = Result<JwkSet, String>> + Send;
}

/// The keys as they were when they were last looked at.
///
/// Cognito rotates signing keys, and a token signed with a key that is not in the cached set is
/// the signal to look again — once, so a token with a made-up `kid` cannot make the CMS hammer
/// the provider.
struct Cache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Instant,
    ttl: Duration,
}

pub struct CognitoVerifier<J: JwksSource> {
    settings: CognitoSettings,
    source: J,
    cache: Mutex<Option<Cache>>,
}

impl<J: JwksSource> CognitoVerifier<J> {
    pub fn new(settings: CognitoSettings, source: J) -> Self {
        CognitoVerifier {
            settings,
            source,
            cache: Mutex::new(None),
        }
    }

    /// The key for `kid`, refreshing the cache when it is missing or stale.
    async fn key(&self, kid: &str) -> Result<DecodingKey, HttpError> {
        {
            let cache = self.cache.lock().map_err(|_| unauthorized())?;
            if let Some(cache) = cache.as_ref() {
                if let Some(key) = cache.keys.get(kid) {
                    if cache.fetched_at.elapsed() < cache.ttl {
                        return Ok(key.clone());
                    }
                }
            }
        }

        let set = self
            .source
            .fetch()
            .await
            .map_err(|e| {
                // The caller learns nothing: an unreachable key set is our problem.
                tracing::warn!("could not fetch the signing keys: {e}");
                unauthorized()
            })?;
        let mut keys = HashMap::new();
        for jwk in &set.keys {
            if let (Some(kid), Ok(key)) = (jwk.common.key_id.clone(), DecodingKey::from_jwk(jwk)) {
                keys.insert(kid, key);
            }
        }

        let mut cache = self.cache.lock().map_err(|_| unauthorized())?;
        *cache = Some(Cache {
            keys,
            fetched_at: Instant::now(),
            ttl: Duration::from_secs(10 * 60),
        });
        cache
            .as_ref()
            .and_then(|cache| cache.keys.get(kid).cloned())
            .ok_or_else(unauthorized)
    }
}

/// The claims of an ID token that the CMS cares about.
#[derive(Debug, Deserialize)]
struct CognitoClaims {
    sub: String,
    /// Cognito's own name for the account (`cognito:username`).
    #[serde(rename = "cognito:username", default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    /// `id` for an ID token, `access` for an access token. Only the former identifies a person
    /// in a way the CMS can use, and the difference is easy to miss.
    #[serde(default)]
    token_use: Option<String>,
}

impl<J: JwksSource> TokenVerifier for CognitoVerifier<J> {
    fn verify<'a>(&'a self, token: &'a str) -> VerifyFuture<'a> {
        Box::pin(self.verify_token(token))
    }
}

impl<J: JwksSource> CognitoVerifier<J> {
    async fn verify_token(&self, token: &str) -> Result<Identity, HttpError> {
        let header = decode_header(token).map_err(|_| unauthorized())?;
        // The algorithm is fixed here, which is what stops a token that claims `none` — or one
        // signed with the public key as an HMAC secret — from being taken seriously.
        if header.alg != Algorithm::RS256 {
            return Err(unauthorized());
        }
        let kid = header.kid.ok_or_else(unauthorized)?;
        let key = self.key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.settings.issuer()]);
        validation.set_audience(&[self.settings.client_id.clone()]);
        validation.required_spec_claims.insert("exp".to_string());
        // Ten seconds of clock skew between Cognito and this process. The default is sixty,
        // which is a minute of a token that should have expired.
        validation.leeway = 10;

        let claims = decode::<CognitoClaims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| {
                tracing::debug!("rejected provider token: {e}");
                unauthorized()
            })?;

        if claims.token_use.as_deref() != Some("id") {
            return Err(unauthorized());
        }
        let username = claims.username.clone().unwrap_or_else(|| claims.sub.clone());
        Ok(Identity::External {
            external_id: claims.sub,
            username,
            email: claims.email,
        })
    }
}

/// The one answer this module gives, so which check failed stays inside.
fn unauthorized() -> HttpError {
    HttpError::Unauthorized("invalid or expired token")
}

/// The keys over HTTPS, as a deployment gets them.
pub struct HttpJwks {
    url: String,
    client: reqwest::Client,
}

impl HttpJwks {
    pub fn new(url: String) -> Self {
        HttpJwks {
            url,
            client: reqwest::Client::new(),
        }
    }
}

impl JwksSource for HttpJwks {
    async fn fetch(&self) -> Result<JwkSet, String> {
        let response = self
            .client
            .get(&self.url)
            .send()
            .await
            .map_err(|e| format!("could not reach {}: {e}", self.url))?;
        if !response.status().is_success() {
            return Err(format!("{} answered {}", self.url, response.status()));
        }
        let body = response
            .text()
            .await
            .map_err(|e| format!("could not read {}: {e}", self.url))?;
        serde_json::from_str::<JwkSet>(&body)
            .map_err(|e| format!("{} did not return a key set: {e}", self.url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A throwaway key pair, generated for these tests and used for nothing else. It is here
    /// rather than in a fixture file so the whole test reads in one place; the private half is
    /// only ever used to sign tokens this test then rejects or accepts.
    const PRIVATE_KEY: &str = include_str!("test_rsa_key.pem");

    fn jwks() -> JwkSet {
        serde_json::from_str(include_str!("test_jwks.json")).expect("a key set")
    }

    #[derive(Clone)]
    struct StaticJwks {
        /// Shared, so the test can count fetches after the verifier has taken the source.
        fetches: Arc<AtomicUsize>,
        keys: JwkSet,
    }

    impl StaticJwks {
        fn new() -> Self {
            StaticJwks {
                fetches: Arc::new(AtomicUsize::new(0)),
                keys: jwks(),
            }
        }

        fn fetches(&self) -> usize {
            self.fetches.load(Ordering::SeqCst)
        }
    }

    impl JwksSource for StaticJwks {
        async fn fetch(&self) -> Result<JwkSet, String> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Ok(self.keys.clone())
        }
    }

    struct FailingJwks;

    impl JwksSource for FailingJwks {
        async fn fetch(&self) -> Result<JwkSet, String> {
            Err("no network in a test".to_string())
        }
    }

    fn settings() -> CognitoSettings {
        CognitoSettings {
            region: "eu-west-1".to_string(),
            user_pool_id: "eu-west-1_abc".to_string(),
            client_id: "client-1".to_string(),
        }
    }

    fn claims(sub: &str, username: &str) -> serde_json::Value {
        let now = chrono::Utc::now().timestamp();
        serde_json::json!({
            "sub": sub,
            "cognito:username": username,
            "email": username,
            "token_use": "id",
            "iss": settings().issuer(),
            "aud": "client-1",
            "iat": now,
            "exp": now + 300,
        })
    }

    /// An ID token as Cognito mints one: signed with the pool's key, and saying which key that
    /// was, because that is how the verifier finds it in the published set.
    fn sign(claims: &serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key-1".to_string());
        sign_with(claims, &header, PRIVATE_KEY)
    }

    fn sign_with(claims: &serde_json::Value, header: &Header, pem: &str) -> String {
        let key = EncodingKey::from_rsa_pem(pem.as_bytes()).expect("a signing key");
        encode(header, claims, &key).expect("a token")
    }

    async fn verify(source: &StaticJwks, token: &str) -> Result<Identity, HttpError> {
        CognitoVerifier::new(settings(), source.clone())
            .verify(token)
            .await
    }

    #[tokio::test]
    async fn a_valid_id_token_becomes_an_identity() {
        let source = StaticJwks::new();
        let token = sign(&claims("sub-1", "ops@example.com"));

        assert_eq!(
            verify(&source, &token).await.unwrap(),
            Identity::External {
                external_id: "sub-1".to_string(),
                username: "ops@example.com".to_string(),
                email: Some("ops@example.com".to_string()),
            }
        );
        assert_eq!(source.fetches(), 1);
    }

    #[tokio::test]
    async fn the_key_set_is_fetched_once_and_reused() {
        let source = StaticJwks::new();
        let token = sign(&claims("sub-1", "ops@example.com"));
        let verifier = CognitoVerifier::new(settings(), source.clone());

        verifier.verify(&token).await.unwrap();
        verifier.verify(&token).await.unwrap();

        // The second token is verified from the cache: a sign-in per request must not be a
        // request to Cognito per request.
        assert_eq!(source.fetches(), 1);
    }

    #[tokio::test]
    async fn an_expired_token_is_refused() {
        let source = StaticJwks::new();
        let mut claims = claims("sub-1", "ops@example.com");
        claims["exp"] = serde_json::json!(chrono::Utc::now().timestamp() - 300);

        assert_eq!(verify(&source, &sign(&claims)).await.unwrap_err(), unauthorized());
    }

    #[tokio::test]
    async fn a_token_for_another_pool_or_client_is_refused() {
        let source = StaticJwks::new();

        let mut other_issuer = claims("sub-1", "ops@example.com");
        other_issuer["iss"] = serde_json::json!("https://cognito-idp.eu-west-1.amazonaws.com/other");
        assert!(verify(&source, &sign(&other_issuer)).await.is_err());

        // A token minted for a different app client of the same pool is not ours either.
        let mut other_audience = claims("sub-1", "ops@example.com");
        other_audience["aud"] = serde_json::json!("another-client");
        assert!(verify(&source, &sign(&other_audience)).await.is_err());
    }

    #[tokio::test]
    async fn an_access_token_is_not_an_identity() {
        let source = StaticJwks::new();
        let mut claims = claims("sub-1", "ops@example.com");
        claims["token_use"] = serde_json::json!("access");

        assert!(verify(&source, &sign(&claims)).await.is_err());
    }

    #[tokio::test]
    async fn a_token_with_no_expiry_is_refused() {
        let source = StaticJwks::new();
        let mut claims = claims("sub-1", "ops@example.com");
        claims.as_object_mut().unwrap().remove("exp");

        assert!(verify(&source, &sign(&claims)).await.is_err());
    }

    #[tokio::test]
    async fn a_token_signed_with_another_key_is_refused() {
        let source = StaticJwks::new();
        // A second key pair, generated next to the first one and used only here: this is the
        // case of a token that is well formed and points at the right pool, but is not signed by
        // the pool.
        const OTHER_KEY: &str = include_str!("test_rsa_key_other.pem");
        let token = sign_with(
            &claims("sub-1", "ops@example.com"),
            &Header::new(Algorithm::RS256),
            OTHER_KEY,
        );

        assert!(verify(&source, &token).await.is_err());
    }

    #[tokio::test]
    async fn an_hmac_token_is_refused_even_when_it_looks_well_formed() {
        let source = StaticJwks::new();
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("test-key-1".to_string());
        let key = EncodingKey::from_secret(b"the-public-key-as-a-secret");
        let token = encode(&header, &claims("sub-1", "ops@example.com"), &key).unwrap();

        assert!(verify(&source, &token).await.is_err());
    }

    #[tokio::test]
    async fn an_unknown_key_id_is_refused_without_refetching_forever() {
        let source = StaticJwks::new();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("a-key-that-does-not-exist".to_string());
        let token = sign_with(&claims("sub-1", "ops@example.com"), &header, PRIVATE_KEY);

        assert!(verify(&source, &token).await.is_err());
        assert!(verify(&source, &token).await.is_err());
        // Refreshed, but not once per attempt on top of that.
        assert!(source.fetches() <= 4);
    }

    #[tokio::test]
    async fn an_unreachable_key_set_is_an_unauthorized_answer_not_a_crash() {
        let verifier = CognitoVerifier::new(settings(), FailingJwks);
        let token = sign(&claims("sub-1", "ops@example.com"));

        let error = verifier.verify(&token).await.unwrap_err();
        assert_eq!(error, unauthorized());
    }

    #[test]
    fn the_urls_are_derived_so_the_pool_and_the_keys_cannot_disagree() {
        let settings = settings();
        assert_eq!(
            settings.issuer(),
            "https://cognito-idp.eu-west-1.amazonaws.com/eu-west-1_abc"
        );
        assert_eq!(
            settings.jwks_url(),
            "https://cognito-idp.eu-west-1.amazonaws.com/eu-west-1_abc/.well-known/jwks.json"
        );
    }
}
