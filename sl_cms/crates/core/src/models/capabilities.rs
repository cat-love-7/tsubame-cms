//! What a deployment can do, so a client can ask instead of guessing.
//!
//! The same CMS runs in places that differ in ways a client has to know about: the on-premises
//! deployment verifies passwords itself and takes image bytes through its own endpoints, while
//! the AWS one signs users in through Cognito and hands out presigned URLs so the browser talks
//! to S3 directly. Discovering that from a 501 is a poor way to find out.

/// A deployment's shape, as `GET /auth/capabilities` reports it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether the CMS verifies passwords: `/auth/login`, `/auth/me/password` and completing a
    /// reset link exist only where it does.
    pub password_login: bool,
    /// What an administrator gets to hand over after a password reset, or `None` where an
    /// administrator cannot reset one at all.
    ///
    /// The client shows what it is told to show; the answer to the reset request says the same
    /// thing again, because that is the moment it has something to copy.
    #[serde(default)]
    pub password_reset: Option<PasswordResetKind>,
    /// How image bytes reach storage.
    pub image_upload: ImageUpload,
    /// Where to send someone to sign in, when that is not here.
    ///
    /// Without it a client can only say "sign in at your identity provider" and leave the user
    /// to find it. A deployment that knows its provider's sign-in page says so here.
    #[serde(default)]
    pub login_url: Option<String>,
    /// Where a shared preview link should be opened: the site that renders unpublished content,
    /// when the deployment has one.
    ///
    /// The API's preview answer is JSON, so without a site there is nothing to hand a reviewer.
    /// A client that finds this absent offers nothing rather than a link nobody can read.
    #[serde(default)]
    pub preview_site_url: Option<String>,
    /// The largest image this deployment accepts, in bytes (`config::Limits::max_image_bytes`).
    ///
    /// A browser knows the size of the file before it sends it, so this is what lets it refuse a
    /// file that is too big rather than upload it and be told afterwards. It is not a promise
    /// about what the API accepts in a body: that is another limit, for JSON.
    #[serde(default)]
    pub max_image_bytes: usize,
}

/// What an administrator hands to an account's owner after resetting its password.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PasswordResetKind {
    /// A single-use link the owner opens to choose the password: a deployment that stores the
    /// credential itself. Nobody else ever sees the password.
    Link,
    /// A temporary password the provider already set, which its owner has to change before the
    /// next sign-in completes: a deployment whose identity provider owns the credential.
    Temporary,
}

/// Who accepts the bytes of an uploaded image.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageUpload {
    /// The CMS accepts them and stores them itself (the upload URL points at this server).
    Proxied,
    /// The CMS hands out a presigned URL and the browser PUTs straight to object storage.
    Presigned,
}

impl Capabilities {
    /// The on-premises deployment: it is the CMS that verifies passwords and keeps the bytes.
    pub fn on_premises(max_image_bytes: usize, preview_site_url: Option<String>) -> Capabilities {
        Capabilities {
            password_login: true,
            password_reset: Some(PasswordResetKind::Link),
            image_upload: ImageUpload::Proxied,
            // This deployment *is* the sign-in page.
            login_url: None,
            preview_site_url,
            max_image_bytes,
        }
    }

    /// The AWS deployment: Cognito signs users in, and S3 takes the bytes directly.
    ///
    /// `login_url` is the pool's hosted sign-in page when the deployment knows it (Terraform
    /// creates the domain and passes it in); without one, a client can only describe where to
    /// go.
    ///
    /// A reset here is Cognito's `AdminSetUserPassword`: the CMS cannot choose a password for an
    /// account it does not own the credential of, so an administrator gets a temporary one to pass
    /// on and the person changes it at their next sign-in.
    pub fn aws(
        login_url: Option<String>,
        max_image_bytes: usize,
        preview_site_url: Option<String>,
    ) -> Capabilities {
        Capabilities {
            password_login: false,
            password_reset: Some(PasswordResetKind::Temporary),
            image_upload: ImageUpload::Presigned,
            login_url,
            preview_site_url,
            max_image_bytes,
        }
    }
}
