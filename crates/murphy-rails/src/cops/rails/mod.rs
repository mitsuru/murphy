//! Real arena-dispatch implementations of Rails cops promoted out of
//! the `register_cops!`-level stubs in `lib.rs`. Each cop here is
//! removed from the `is_cop_disabled_by_default` hardcode list in
//! `crates/murphy-core/src/config.rs` (cleanup tracked by
//! `murphy-bnd`).

mod assert_not;
mod environment_variable_access;
mod i18n_locale_assignment;
mod negate_include;
mod output;
mod pick;
mod request_referer;
mod uniq_before_pluck;

pub use assert_not::AssertNot;
pub use environment_variable_access::EnvironmentVariableAccess;
pub use i18n_locale_assignment::I18nLocaleAssignment;
pub use negate_include::NegateInclude;
pub use output::Output;
pub use pick::Pick;
pub use request_referer::RequestReferer;
pub use uniq_before_pluck::UniqBeforePluck;
