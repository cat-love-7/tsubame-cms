//! The CMS itself: models, storage contracts, services and the HTTP layer.
//!
//! This crate knows nothing about where data is kept. Storage is a set of traits
//! ([`repositories`]) that a backend package implements, and the adapters are separate
//! packages so that building one never builds the other:
//!
//! - `sl-cms-on-premises` — rkv (LMDB) and image files on disk;
//! - `sl-cms-aws` — DynamoDB and S3.
//!
//! Everything shared lives here, which is why there is not a single `#[cfg(feature = ...)]`
//! in this crate: a capability an adapter may lack is a trait it does not implement or a
//! route it does not merge, not something compiled in or out.

pub mod app_module;
pub mod auth;
pub mod config;
pub mod http;
pub mod models;
pub mod password_reset;
pub mod preview_link;
pub mod repositories;
pub mod services;
pub mod signing;
pub mod webhook;
