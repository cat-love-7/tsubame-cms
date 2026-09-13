//! AWS storage adapter: DynamoDB (single table) for structured data, S3 for image bytes.
//!
//! Not written yet. The shape is settled, and what is left is the work itself:
//!
//! * a `Repository` over one DynamoDB table (`Repository: Storage`), with the published copy,
//!   the working copy and the metadata as three records per item, an atomic counter for item
//!   ids, and `TransactWriteItems` for the publish that copies one to the other;
//! * S3 behind [`crate::repositories::image_repository::ImageRepository`]: a presigned PUT as
//!   the upload target, the object's own URL stored in the content (a *stable* URL - never a
//!   presigned one, which would rot in the content), and no local byte serving at all;
//! * [`build_app_module`] as the composition root, exactly like `on_premises::build_app_module`;
//! * a Lambda entry point that hands the shared router to `lambda_http`.
//!
//! Until that exists, selecting this backend is refused here rather than failing somewhere
//! less obvious. The plan, including what has to be decided first, is in `doc/aws-plan.md`.

compile_error!(
    "the `aws` backend (DynamoDB + S3) is not implemented yet; build with the default \
     `on-premises` feature"
);
