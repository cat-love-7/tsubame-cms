//! The settings an AWS deployment is described by.

/// What this backend needs, read from the environment and checked before it is used.
///
/// It lives here rather than in the core's `Config` because no other backend has a use for it:
/// the shared configuration describes the CMS, and this describes the AWS deployment. Checked
/// at startup so a misconfigured deployment fails with a sentence, which is also what the
/// on-premises backend does when it cannot open its storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsSettings {
    pub region: String,
    pub table: String,
    pub bucket: String,
    pub user_pool_id: String,
    /// The app client the browser signs in with; the audience of the ID token it gets back.
    pub client_id: String,
    /// Cognito's hosted sign-in page, when the deployment has one, so a client can send people
    /// there instead of telling them to find it.
    pub login_url: Option<String>,
    /// Where the DynamoDB endpoint is, for a local emulator. `None` means the real AWS
    /// endpoint, which is what a deployment uses.
    pub endpoint_url: Option<String>,
    /// The same for S3. Falls back to `endpoint_url` when only that one is set.
    pub s3_endpoint_url: Option<String>,
    /// Where an uploaded image is read from, when that is not the bucket itself.
    pub image_base_url: Option<String>,
    /// Set for a local emulator, which verifies the signature. `None` means the SDK's own
    /// credential chain, which is the Lambda execution role in a deployment.
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    /// May be empty: then nobody can provision themselves and an administrator has to create
    /// the first account record another way.
    pub bootstrap_admin_usernames: Vec<String>,
}

impl AwsSettings {
    /// The endpoint the SDK should talk to: a local emulator when set, AWS otherwise.
    pub fn endpoint_url(&self) -> String {
        self.endpoint_url
            .clone()
            .unwrap_or_else(|| format!("https://dynamodb.{}.amazonaws.com", self.region))
    }

    /// The endpoint the S3 client should talk to: a local emulator when set, AWS otherwise.
    pub fn s3_endpoint_url(&self) -> Option<String> {
        self.s3_endpoint_url.clone().or_else(|| self.endpoint_url.clone())
    }

    /// The URL an uploaded image is served from.
    ///
    /// Deliberately stable, not presigned: the URL is written into content and rendered on a
    /// public page, so it must not expire the way a signature does. A deployment puts a CDN in
    /// front (`AWS_IMAGE_BASE_URL`); a local emulator serves path-style `endpoint/bucket/key`.
    pub fn image_url(&self, file_name: &str) -> String {
        match (&self.image_base_url, self.s3_endpoint_url()) {
            (Some(base), _) => format!("{}/{file_name}", base.trim_end_matches('/')),
            (None, Some(endpoint)) => format!(
                "{}/{}/{file_name}",
                endpoint.trim_end_matches('/'),
                self.bucket
            ),
            (None, None) => format!(
                "https://{}.s3.{}.amazonaws.com/{file_name}",
                self.bucket, self.region
            ),
        }
    }

    /// Where Cognito publishes the signing keys for this pool. Derived rather than configured,
    /// so a pool id and a region cannot disagree.
    pub fn jwks_url(&self) -> String {
        format!(
            "https://cognito-idp.{}.amazonaws.com/{}/.well-known/jwks.json",
            self.region, self.user_pool_id
        )
    }
}

impl AwsSettings {
    /// Read the settings a deployment is described by.
    ///
    /// Missing values are hard errors naming the variable, rather than defaults discovered to
    /// be wrong later: a CMS that comes up pointed at the wrong table is worse than one that
    /// refuses to come up at all.
    pub fn from_env() -> Result<Self, String> {
        let required = |value: Option<String>, name: &str| {
            value.ok_or_else(|| format!("{name} must be set for the aws backend"))
        };
        Ok(AwsSettings {
            region: required(non_empty_env("AWS_REGION"), "AWS_REGION")?,
            table: required(non_empty_env("DYNAMODB_TABLE"), "DYNAMODB_TABLE")?,
            bucket: required(non_empty_env("S3_BUCKET"), "S3_BUCKET")?,
            // Cognito is not wired up yet (doc/aws-plan.md P4), but a deployment that names its
            // pool now does not have to change its environment later.
            user_pool_id: required(non_empty_env("COGNITO_USER_POOL_ID"), "COGNITO_USER_POOL_ID")?,
            client_id: required(non_empty_env("COGNITO_CLIENT_ID"), "COGNITO_CLIENT_ID")?,
            login_url: non_empty_env("COGNITO_LOGIN_URL"),
            endpoint_url: non_empty_env("AWS_ENDPOINT_URL"),
            s3_endpoint_url: non_empty_env("AWS_ENDPOINT_URL_S3"),
            image_base_url: non_empty_env("AWS_IMAGE_BASE_URL"),
            access_key_id: non_empty_env("AWS_ACCESS_KEY_ID"),
            secret_access_key: non_empty_env("AWS_SECRET_ACCESS_KEY"),
            bootstrap_admin_usernames: std::env::var("BOOTSTRAP_ADMIN_USERNAMES")
                .map(|names| parse_usernames(&names))
                .unwrap_or_default(),
        })
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// `BOOTSTRAP_ADMIN_USERNAMES` as a list, normalized the way accounts are stored so the
/// comparison at sign-in is a plain equality check.
fn parse_usernames(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(sl_cms_core::models::user::normalize_username)
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AwsSettings {
        AwsSettings {
            region: "eu-west-1".to_string(),
            table: "cms".to_string(),
            bucket: "cms-images".to_string(),
            user_pool_id: "eu-west-1_abc".to_string(),
            client_id: "client-1".to_string(),
            login_url: None,
            endpoint_url: None,
            s3_endpoint_url: None,
            image_base_url: None,
            access_key_id: None,
            secret_access_key: None,
            bootstrap_admin_usernames: parse_usernames("Ops"),
        }
    }

    #[test]
    fn the_jwks_url_is_derived_so_pool_and_region_cannot_disagree() {
        assert_eq!(
            settings().jwks_url(),
            "https://cognito-idp.eu-west-1.amazonaws.com/eu-west-1_abc/.well-known/jwks.json"
        );
    }

    #[test]
    fn usernames_are_split_and_normalized() {
        assert_eq!(parse_usernames("Ops"), vec!["ops".to_string()]);
        assert_eq!(
            parse_usernames(" Ops , Alice@Example.com ,, "),
            vec!["ops".to_string(), "alice@example.com".to_string()]
        );
        assert_eq!(parse_usernames(""), Vec::<String>::new());
    }

    #[test]
    fn an_image_url_is_stable_and_never_signed() {
        // A CDN in front of the bucket wins when one is configured.
        let mut with_cdn = settings();
        with_cdn.image_base_url = Some("https://images.example.com/".to_string());
        assert_eq!(with_cdn.image_url("a.png"), "https://images.example.com/a.png");

        // An emulator serves path-style URLs.
        let mut with_endpoint = settings();
        with_endpoint.s3_endpoint_url = Some("http://localhost:9000".to_string());
        assert_eq!(
            with_endpoint.image_url("a.png"),
            "http://localhost:9000/cms-images/a.png"
        );

        // A deployment without either gets the bucket's own host name.
        assert_eq!(
            settings().image_url("a.png"),
            "https://cms-images.s3.eu-west-1.amazonaws.com/a.png"
        );
    }
}
