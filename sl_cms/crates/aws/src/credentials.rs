//! Credentials for a local endpoint.
//!
//! A deployment signs with the execution role and nothing here applies; a local run points the
//! SDKs at the emulators, which only accept the credentials they were started with.

use super::*;

/// Credentials for a local emulator, or `None` to let the SDK's own chain find them.
///
/// A deployment has nothing to put here: on Lambda the chain resolves the execution role. An
/// emulator, unlike DynamoDB Local, verifies the signature, so the CMS has to be told the same
/// keys MinIO was started with.
///
/// **The endpoint decides, not the variable.** Lambda sets `AWS_ACCESS_KEY_ID`,
/// `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN` to the execution role's temporary credentials,
/// so treating those names as "an emulator is configured" would replace the role - with the same
/// key and secret but *without* the session token - and every request would then be signed for an
/// identity that does not exist. A session token in the environment is carried along for the same
/// reason: an emulator may be started with one, and dropping it would break that too.
pub(crate) fn emulator_credentials(
    settings: &AwsSettings,
) -> Option<aws_sdk_dynamodb::config::Credentials> {
    if settings.endpoint_url.is_none() && settings.s3_endpoint_url.is_none() {
        return None;
    }
    match (&settings.access_key_id, &settings.secret_access_key) {
        (Some(access_key_id), Some(secret_access_key)) => {
            Some(aws_sdk_dynamodb::config::Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                settings.session_token.clone(),
                None,
                "cms",
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    fn settings(endpoint: Option<&str>) -> AwsSettings {
        AwsSettings {
            region: "us-east-1".to_string(),
            table: "cms".to_string(),
            bucket: "cms-images".to_string(),
            user_pool_id: "pool".to_string(),
            client_id: "client".to_string(),
            login_url: None,
            preview_site_url: None,
            endpoint_url: endpoint.map(str::to_string),
            s3_endpoint_url: None,
            image_base_url: None,
            image_delivery: ImageDelivery::stable(),
            access_key_id: Some("AKIA-FROM-THE-ENVIRONMENT".to_string()),
            secret_access_key: Some("secret".to_string()),
            session_token: Some("session".to_string()),
            bootstrap_admin_usernames: Vec::new(),
        }
    }

    /// The variables exist in a deployment too, holding the execution role's credentials, so the
    /// endpoint is what says "this is an emulator". Reading them as `Some` was how the role's
    /// session token came to be dropped, which signs for an identity that does not exist.
    #[test]
    fn explicit_credentials_are_only_for_an_endpoint_override() {
        assert!(emulator_credentials(&settings(None)).is_none());

        let credentials = emulator_credentials(&settings(Some("http://localhost:8000")))
            .expect("an emulator is configured");
        assert_eq!(credentials.access_key_id(), "AKIA-FROM-THE-ENVIRONMENT");
        assert_eq!(
            credentials.session_token(),
            Some("session"),
            "the session token travels with the credentials"
        );
    }

    /// An emulator started without a session token still works: the SDK wants `None`, not an
    /// empty string.
    #[test]
    fn credentials_without_a_session_token_are_plain() {
        let mut settings = settings(Some("http://localhost:8000"));
        settings.session_token = None;
        let credentials = emulator_credentials(&settings).expect("an emulator is configured");
        assert_eq!(credentials.session_token(), None);
    }
}
