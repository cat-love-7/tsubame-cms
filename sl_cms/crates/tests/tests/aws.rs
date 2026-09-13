//! The contract suite against the AWS adapter (DynamoDB + S3).
//!
//! Needs the emulators from `sl_cms/docker-compose.yml`: DynamoDB Local on 8000 (or
//! `CMS_TEST_DYNAMODB_ENDPOINT`) and MinIO on 9000 (or `CMS_TEST_S3_ENDPOINT`). Without them
//! this file fails with instructions rather than passing quietly — `scripts/test-rust.sh`
//! checks first and skips the whole file when they are not running.

use sl_cms_tests::backends::Aws;

type Backend = Aws;

mod contract {
    include!("../suite.rs");
}
