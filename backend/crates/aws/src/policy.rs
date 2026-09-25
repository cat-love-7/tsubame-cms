//! The S3 policy a deployment gives its function, as the tests are told it.
//!
//! It mirrors `infra/lambda.tf`; see the note in `docs/aws-decisions.md` about the two being kept in
//! step by hand.

/// The S3 policy a deployment gives its function, as the tests are told it.
///
/// A copy of `infra/lambda.tf`'s two statements. It is here so a test that depends on a permission
/// can say so out loud, and so that changing the deployment's policy is something the tests are
/// expected to follow - they are the only place the difference is visible.
pub fn deployment_s3_policy(bucket: &str, list_bucket: bool) -> String {
    policy(bucket, None, list_bucket)
}

/// The same policy, as a **bucket** policy naming one user.
///
/// The emulator's built-in IAM has no identity policies (versitygw with `--iam-dir`): what a user
/// may do is what a bucket policy grants them, and anything else is denied - which is the same shape
/// as a deployment, where the function's policy is the only thing that grants it anything. So the
/// statements are the deployment's, with the user as their `Principal`.
pub fn deployment_bucket_policy(bucket: &str, principal: &str, list_bucket: bool) -> String {
    policy(bucket, Some(principal), list_bucket)
}

fn policy(bucket: &str, principal: Option<&str>, list_bucket: bool) -> String {
    let who = principal
        .map(|name| format!(r#""Principal":"{name}","#))
        .unwrap_or_default();
    let mut statements = vec![format!(
        r#"{{{who}"Effect":"Allow","Action":["s3:GetObject","s3:PutObject","s3:DeleteObject"],"Resource":["arn:aws:s3:::{bucket}/*"]}}"#
    )];
    if list_bucket {
        statements.push(format!(
            r#"{{{who}"Effect":"Allow","Action":["s3:ListBucket"],"Resource":["arn:aws:s3:::{bucket}"]}}"#
        ));
    }
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{}]}}"#,
        statements.join(",")
    )
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    /// The fixture is the deployment's policy: the statements the function is granted, and nothing
    /// else. A test that depends on a permission reads the deployment, not this file.
    #[test]
    fn the_emulator_policy_is_the_deployments() {
        let allowed = deployment_s3_policy("cms-images", true);
        assert!(allowed.contains(r#""s3:GetObject","s3:PutObject","s3:DeleteObject""#));
        assert!(allowed.contains(r#""arn:aws:s3:::cms-images/*""#));
        assert!(allowed.contains(r#""s3:ListBucket""#));
        assert!(allowed.contains(r#""arn:aws:s3:::cms-images""#));

        // The other half of the puzzle: what the tests are like without it.
        let denied = deployment_s3_policy("cms-images", false);
        assert!(!denied.contains("ListBucket"));
        assert!(denied.contains("s3:GetObject"));
    }
}
