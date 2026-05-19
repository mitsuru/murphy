//! Murphy core: the native engine for the Murphy Ruby linter/formatter.

mod aggregator;
mod cop;
mod cops;
mod discovery;
mod offense;
mod parse;

pub use aggregator::aggregate;
pub use cop::{Cop, CopContext, run_cops};
pub use cops::no_receiver_puts::NoReceiverPuts;
pub use discovery::{ConfigError, discover};
pub use offense::{Offense, Range, SYNTAX_COP_NAME, Severity};
pub use parse::{Ast, ParseError, parse};

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
