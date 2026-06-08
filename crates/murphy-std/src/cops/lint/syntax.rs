//! `Lint/Syntax` — reports Ruby syntax errors.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Syntax
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-zpgm]
//! notes: >
//!   RuboCop Lint/Syntax runs before normal AST investigation and reports
//!   parser diagnostics/errors. Murphy plugin cops currently receive Cx only
//!   after parsing/translating, so this cannot be implemented against the
//!   single-surface murphy-plugin-api without a parse-diagnostics hook
//!   (murphy-zpgm). This cop is registered as a no-op placeholder until that
//!   ABI/API gap is closed.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions};

#[derive(Default)]
pub struct Syntax;

#[cop(
    name = "Lint/Syntax",
    description = "Reports Ruby syntax errors.",
    default_severity = "error",
    default_enabled = true,
    options = NoOptions,
)]
impl Syntax {
    #[on_new_investigation]
    fn check_file(&self, _cx: &Cx<'_>) {}
}

murphy_plugin_api::submit_cop!(Syntax);
