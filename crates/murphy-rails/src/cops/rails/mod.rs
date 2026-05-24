//! Real arena-dispatch implementations of Rails cops promoted out of
//! the `register_cops!`-level stubs in `lib.rs`. Each cop here is
//! removed from the `is_cop_disabled_by_default` hardcode list in
//! `crates/murphy-core/src/config.rs` (cleanup tracked by
//! `murphy-bnd`).

mod assert_not;
mod output;
mod request_referer;

pub use assert_not::AssertNot;
pub use output::Output;
pub use request_referer::RequestReferer;
