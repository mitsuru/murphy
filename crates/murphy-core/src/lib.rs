//! Murphy core: the native engine for the Murphy Ruby linter/formatter.

mod aggregator;
pub mod autocorrect;
mod cop;
mod cops;
mod discovery;
mod mruby;
mod offense;
mod parse;
mod registry;

pub use aggregator::aggregate;
pub use autocorrect::{ApplyOutcome, Conflict, ConflictReason, apply_edits, apply_edits_logged};
pub use cop::{Cop, CopContext, run_cops};
pub use cops::no_receiver_puts::NoReceiverPuts;
pub use discovery::{ConfigError, discover};
// Phase 3 Task 2 keystone — the mruby lifecycle/ownership wrapper. Task 3's
// read-only native-primitive IDL registration (`register_primitives`) is
// in-crate only (`pub(crate)` in `mruby`), so it is deliberately NOT
// re-exported here — Task 4/5/7 reach it via `crate::mruby::register_primitives`.
// Nothing in the CLI pipeline calls it yet (Task 7 wires it).
pub use mruby::{
    AstContext, COP_DEADLINE, MrubyState, run_mruby_cop, run_mruby_cop_isolated,
    run_mruby_cop_isolated_with_deadline,
};
pub use offense::{Autocorrect, Edit, Offense, Range, SYNTAX_COP_NAME, Severity};
pub use parse::{Ast, ParseError, parse};
pub use registry::CopRegistry;

/// Returns the Murphy core crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(
            !version().is_empty(),
            "version() must return a non-empty string"
        );
    }
}
