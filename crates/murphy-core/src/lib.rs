//! Murphy core: the native engine for the Murphy Ruby linter/formatter.

mod aggregator;
mod cop;
mod cops;
mod discovery;
mod mruby;
mod offense;
mod parse;
mod registry;

pub use aggregator::aggregate;
pub use cop::{Cop, CopContext, run_cops};
pub use cops::no_receiver_puts::NoReceiverPuts;
pub use discovery::{ConfigError, discover};
// Phase 3 Task 2 keystone — the mruby lifecycle/ownership wrapper. Re-exported
// for later tasks (3/4/5/7) and the wrapper's own tests; nothing in the CLI
// pipeline calls this yet (Task 7 wires it).
pub use mruby::{AstContext, MrubyState};
pub use offense::{Offense, Range, SYNTAX_COP_NAME, Severity};
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
