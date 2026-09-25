pub mod collection_repository;
pub mod composite_field_repository;
pub mod content_reader;
pub mod image_repository;
pub mod local_credentials;
pub mod local_image_bytes;
pub mod relation_repository;
pub mod relation_rules;
pub mod relation_targets;
pub mod single_page_repository;
pub mod user_repository;

/// In-memory storage, for the service tests (see the module for why it is shared).
#[cfg(test)]
pub mod memory;
