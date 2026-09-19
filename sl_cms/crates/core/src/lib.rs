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

/// Where the HTTP API lives, from the origin: `https://cms.example.com/api/...`.
///
/// One prefix, in one place, because it is a contract between three things that cannot share
/// code: this router (`http::router_with` nests everything under it), the paths this crate hands
/// out (`preview_link`, and the on-premises adapter's image URLs), and the frontend
/// (`frontend/sl_cms/src/app/core/api-url.ts` mirrors the constant). An earlier shape served the
/// API unprefixed and had every deployment strip `/api` on the way in - the dev proxy, nginx, and
/// CloudFront each in their own way - which is three places to get one thing right.
///
/// The liveness route (`GET /`) stays outside it: platform readiness checks ask for `/`, and it
/// does not touch storage.
pub const API_PREFIX: &str = "/api";

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
