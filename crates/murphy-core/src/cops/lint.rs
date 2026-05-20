//! Native lint cops.
//!
//! This group is reserved for shipped lint cops as they are ported.

mod debugger;
mod deprecated_class_methods;

pub use debugger::Debugger;
pub use deprecated_class_methods::DeprecatedClassMethods;
