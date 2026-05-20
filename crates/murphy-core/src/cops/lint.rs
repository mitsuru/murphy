//! Native lint cops.
//!
//! This group is reserved for shipped lint cops as they are ported.

mod debugger;
mod deprecated_class_methods;
mod empty_when;
mod unreachable_code;

pub use debugger::Debugger;
pub use deprecated_class_methods::DeprecatedClassMethods;
pub use empty_when::EmptyWhen;
pub use unreachable_code::UnreachableCode;
