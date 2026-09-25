//! The contract suite against the local adapter (rkv + image files on disk).
//!
//! Nothing here but the choice of backend: `suite/` is the same directory the AWS runner compiles,
//! so what passes here and what passes there are the same tests.

use sl_cms_tests::backends::OnPremises;

type Backend = OnPremises;

#[path = "../suite/mod.rs"]
mod contract;
