//! Outbound webhooks for content events.
//!
//! A static site is rebuilt when content is published, so the CMS has to tell whoever
//! performs the build that something changed. That is all this module does: it posts a
//! small JSON description of the change to every configured receiver.
//!
//! Two properties matter:
//!
//! * **Publishing must not depend on a receiver being up.** [`Notifier::notify`] only
//!   spawns a task, so the publish request returns as soon as the status is stored.
//! * **The receiver can trust the call.** With `WEBHOOK_SECRET` set, the raw request body
//!   is signed with HMAC-SHA256 and the digest travels in `X-CMS-Signature`.
//!
//! Delivery is best effort: three attempts with a short backoff, then the failure is
//! logged. There is no outbox, so a CMS that is restarted mid-delivery loses that event;
//! see `doc/content-api.md`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::Sha256;

use crate::models::collection::{CollectionItemId, CollectionName};
use crate::models::item_status::{ItemMetadata, ItemStatus};
use crate::models::single_page::SinglePageName;

/// Attempts per receiver. Retrying is for a receiver that is briefly down or a connection
/// that broke, not for one that understood the request and refused it.
const MAX_ATTEMPTS: u32 = 3;
const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub const EVENT_HEADER: &str = "x-cms-event";
pub const DELIVERY_HEADER: &str = "x-cms-delivery";
pub const SIGNATURE_HEADER: &str = "x-cms-signature";

/// Which kind of content changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    CollectionItem,
    SinglePage,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            EventKind::CollectionItem => "collection_item",
            EventKind::SinglePage => "single_page",
        }
    }
}

/// A status change, built by the services at the moment it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentEvent {
    pub kind: EventKind,
    pub status: ItemStatus,
    pub published_at: Option<DateTime<Utc>>,
    pub occurred_at: DateTime<Utc>,
    pub collection: Option<CollectionName>,
    pub page: Option<SinglePageName>,
    pub item_id: Option<CollectionItemId>,
}

impl ContentEvent {
    pub fn collection_item(
        collection: &CollectionName,
        item_id: CollectionItemId,
        metadata: &ItemMetadata,
    ) -> Self {
        ContentEvent {
            kind: EventKind::CollectionItem,
            status: metadata.status,
            published_at: metadata.published_at,
            occurred_at: Utc::now(),
            collection: Some(collection.clone()),
            page: None,
            item_id: Some(item_id),
        }
    }

    pub fn single_page(page: &SinglePageName, metadata: &ItemMetadata) -> Self {
        ContentEvent {
            kind: EventKind::SinglePage,
            status: metadata.status,
            published_at: metadata.published_at,
            occurred_at: Utc::now(),
            collection: None,
            page: Some(page.clone()),
            item_id: None,
        }
    }

    /// The name a receiver switches on, e.g. `collection_item.published`.
    pub fn name(&self) -> String {
        let action = match self.status {
            ItemStatus::Published => "published",
            ItemStatus::Draft => "unpublished",
        };
        format!("{}.{action}", self.kind.as_str())
    }

    /// The exact bytes that are signed and sent.
    pub fn body(&self) -> String {
        serde_json::to_string(&Payload {
            event: self.name(),
            collection: self.collection.clone(),
            page: self.page.clone(),
            id: self.item_id,
            status: self.status,
            published_at: self.published_at,
            occurred_at: self.occurred_at,
        })
        .expect("a content event always serialises")
    }
}

/// Wire form. Spelled out rather than derived from [`ContentEvent`] so the JSON contract
/// is visible in one place (and so internals cannot leak into it by accident).
#[derive(Serialize)]
struct Payload {
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    collection: Option<CollectionName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<SinglePageName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<CollectionItemId>,
    status: ItemStatus,
    published_at: Option<DateTime<Utc>>,
    occurred_at: DateTime<Utc>,
}

/// Reports content events. Implementations must return promptly: they are called on the
/// request path, right after the status has been stored.
pub trait Notifier: Send + Sync + 'static {
    fn notify(&self, event: ContentEvent);
}

/// Does nothing. Used when no webhook is configured, and by tests.
#[derive(Debug, Default)]
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify(&self, _event: ContentEvent) {}
}

/// Posts every event to each configured URL.
pub struct WebhookNotifier {
    client: reqwest::Client,
    targets: Vec<String>,
    secret: Option<Vec<u8>>,
}

/// reqwest 0.13 no longer picks a rustls crypto provider for the process, so the
/// application has to. Ring is the pure-Rust provider, which keeps CMake out of the
/// build; a provider installed by someone else first is just as good, hence the
/// ignored error.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

impl WebhookNotifier {
    pub fn new(targets: Vec<String>, secret: Option<Vec<u8>>) -> Result<Self, String> {
        install_crypto_provider();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| format!("could not create the webhook HTTP client: {e}"))?;
        Ok(WebhookNotifier {
            client,
            targets,
            secret,
        })
    }

    pub fn targets(&self) -> &[String] {
        &self.targets
    }
}

impl Notifier for WebhookNotifier {
    fn notify(&self, event: ContentEvent) {
        let body = event.body();
        let event_name = event.name();
        let signature = self.secret.as_deref().map(|secret| sign(secret, body.as_bytes()));

        for target in &self.targets {
            let client = self.client.clone();
            let target = target.clone();
            let body = body.clone();
            let event_name = event_name.clone();
            let signature = signature.clone();
            let delivery = uuid::Uuid::new_v4().to_string();

            // Spawned and never awaited: a slow receiver must not slow down publishing.
            tokio::spawn(async move {
                deliver(&client, &target, &event_name, &delivery, &body, signature.as_deref()).await;
            });
        }
    }
}

/// Post one delivery, retrying only what could plausibly succeed later.
async fn deliver(
    client: &reqwest::Client,
    target: &str,
    event: &str,
    delivery: &str,
    body: &str,
    signature: Option<&str>,
) {
    let mut last_failure = String::from("no attempt was made");

    for attempt in 1..=MAX_ATTEMPTS {
        let mut request = client
            .post(target)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(EVENT_HEADER, event)
            .header(DELIVERY_HEADER, delivery)
            .body(body.to_string());
        if let Some(signature) = signature {
            request = request.header(SIGNATURE_HEADER, format!("sha256={signature}"));
        }

        match request.send().await {
            Ok(response) if response.status().is_success() => {
                tracing::info!(
                    target_uri = target,
                    event,
                    delivery,
                    status = %response.status(),
                    "webhook delivered"
                );
                return;
            }
            Ok(response) => {
                // A 4xx means the receiver understood the request and refused it; the
                // same body will be refused again.
                if response.status().is_client_error() {
                    tracing::warn!(
                        target_uri = target,
                        event,
                        delivery,
                        status = %response.status(),
                        "webhook rejected, not retrying"
                    );
                    return;
                }
                last_failure = format!("HTTP {}", response.status());
            }
            Err(error) => last_failure = error.to_string(),
        }

        if attempt < MAX_ATTEMPTS {
            tokio::time::sleep(FIRST_BACKOFF * 2u32.pow(attempt - 1)).await;
        }
    }

    tracing::error!(
        target_uri = target,
        event,
        delivery,
        attempts = MAX_ATTEMPTS,
        error = %last_failure,
        "webhook delivery failed"
    );
}

/// HMAC-SHA256 of the raw body, hex encoded, for the `X-CMS-Signature` header.
pub fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts a key of any length");
    mac.update(body);
    to_hex(&mac.finalize().into_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Build the notifier for the configured receivers.
///
/// No receivers means no webhooks at all. A client that cannot be built (a TLS provider
/// problem, which is not something the operator can fix through configuration) disables
/// webhooks with an error log rather than preventing the CMS from starting.
pub fn build_notifier(targets: Vec<String>, secret: Option<Vec<u8>>) -> Arc<dyn Notifier> {
    if targets.is_empty() {
        if secret.is_some() {
            tracing::warn!(
                "WEBHOOK_SECRET is set but WEBHOOK_URLS is empty, so no webhooks are sent"
            );
        }
        return Arc::new(NoopNotifier);
    }

    match WebhookNotifier::new(targets, secret) {
        Ok(notifier) => {
            tracing::info!(receivers = notifier.targets().len(), "content webhooks enabled");
            Arc::new(notifier)
        }
        Err(error) => {
            tracing::error!("webhooks are disabled: {error}");
            Arc::new(NoopNotifier)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection_event(status: ItemStatus) -> ContentEvent {
        let metadata = ItemMetadata::default().with_status(status, None);
        ContentEvent::collection_item(
            &CollectionName::from("blog"),
            CollectionItemId::from_u64(7),
            &metadata,
        )
    }

    #[test]
    fn names_the_event_after_the_kind_and_the_new_status() {
        assert_eq!(collection_event(ItemStatus::Published).name(), "collection_item.published");
        assert_eq!(collection_event(ItemStatus::Draft).name(), "collection_item.unpublished");

        let page = ContentEvent::single_page(
            &SinglePageName::from("home"),
            &ItemMetadata::default().with_status(ItemStatus::Published, None),
        );
        assert_eq!(page.name(), "single_page.published");
    }

    #[test]
    fn payload_carries_only_the_fields_that_apply() {
        let published = collection_event(ItemStatus::Published);
        let body: serde_json::Value =
            serde_json::from_str(&published.body()).expect("the payload is valid JSON");

        assert_eq!(body["event"], "collection_item.published");
        assert_eq!(body["collection"], "blog");
        assert_eq!(body["id"], 7);
        assert_eq!(body["status"], "published");
        assert!(body["published_at"].is_string());
        assert!(body["occurred_at"].is_string());
        // A collection event says nothing about single pages.
        assert!(body.get("page").is_none());
    }

    #[test]
    fn an_unpublished_event_has_no_publication_time() {
        let body: serde_json::Value =
            serde_json::from_str(&collection_event(ItemStatus::Draft).body()).unwrap();

        assert_eq!(body["event"], "collection_item.unpublished");
        assert_eq!(body["status"], "draft");
        assert_eq!(body["published_at"], serde_json::Value::Null);
    }

    #[test]
    fn a_single_page_event_names_the_page_instead_of_a_collection() {
        let event = ContentEvent::single_page(
            &SinglePageName::from("home"),
            &ItemMetadata::default().with_status(ItemStatus::Published, None),
        );
        let body: serde_json::Value = serde_json::from_str(&event.body()).unwrap();

        assert_eq!(body["page"], "home");
        assert!(body.get("collection").is_none());
        assert!(body.get("id").is_none());
    }

    /// The digest has to match what a receiver computes with its own HMAC library, so the
    /// expected value below was produced independently (Python's `hmac` module).
    #[test]
    fn signs_the_raw_body_with_hmac_sha256() {
        assert_eq!(
            sign(b"test-secret", br#"{"hello":"world"}"#),
            "84cc33df716ed0b0598f07437c94069ace3730358778a592bd6bbd1423d111f3"
        );
    }

    #[test]
    fn the_signature_depends_on_the_secret_and_the_body() {
        let body = b"{}";
        assert_ne!(sign(b"secret-a", body), sign(b"secret-b", body));
        assert_ne!(sign(b"secret-a", body), sign(b"secret-a", b"[]"));
        // Always lower-case hex of 32 bytes.
        assert_eq!(sign(b"secret-a", body).len(), 64);
        assert!(sign(b"secret-a", body).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hex_encoding_is_zero_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
