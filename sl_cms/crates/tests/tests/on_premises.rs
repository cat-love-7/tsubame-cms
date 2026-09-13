//! The contract suite against the local adapter (rkv + image files on disk).
//!
//! Nothing here but the choice of backend: `suite.rs` is the same file the AWS runner
//! includes, so what passes here and what passes there are the same tests.

use sl_cms_tests::backends::OnPremises;

type Backend = OnPremises;

mod contract {
    include!("../suite.rs");
}
