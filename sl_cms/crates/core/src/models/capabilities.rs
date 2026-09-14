//! What a deployment can do, so a client can ask instead of guessing.
//!
//! The same CMS runs in places that differ in ways a client has to know about: the on-premises
//! deployment verifies passwords itself and takes image bytes through its own endpoints, while
//! the AWS one signs users in through Cognito and hands out presigned URLs so the browser talks
//! to S3 directly. Discovering that from a 501 is a poor way to find out.

/// A deployment's shape, as `GET /auth/capabilities` reports it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether the CMS verifies passwords: `/auth/login`, `/auth/me/password` and the reset
    /// links exist only where it does.
    pub password_login: bool,
    /// Whether an administrator can mint a password-reset link to hand to someone.
    pub password_reset_links: bool,
    /// How image bytes reach storage.
    pub image_upload: ImageUpload,
    /// Where to send someone to sign in, when that is not here.
    ///
    /// Without it a client can only say "sign in at your identity provider" and leave the user
    /// to find it. A deployment that knows its provider's sign-in page says so here.
    #[serde(default)]
    pub login_url: Option<String>,
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
    pub const ON_PREMISES: Capabilities = Capabilities {
        password_login: true,
        password_reset_links: true,
        image_upload: ImageUpload::Proxied,
        // This deployment *is* the sign-in page.
        login_url: None,
    };

    /// The AWS deployment: Cognito signs users in, and S3 takes the bytes directly.
    ///
    /// `login_url` is the pool's hosted sign-in page when the deployment knows it (Terraform
    /// creates the domain and passes it in); without one, a client can only describe where to
    /// go.
    pub fn aws(login_url: Option<String>) -> Capabilities {
        Capabilities {
            password_login: false,
            password_reset_links: false,
            image_upload: ImageUpload::Presigned,
            login_url,
        }
    }
}
