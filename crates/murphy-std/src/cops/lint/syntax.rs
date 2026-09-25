//! `Lint/Syntax` — reports Ruby syntax errors.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Syntax
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop Lint/Syntax runs before normal AST investigation and reports
//!   parser diagnostics/errors. Murphy threads prism diagnostics into every
//!   cop's `Cx` via `Cx::parse_diagnostics()` (murphy-zpgm), so this cop
//!   reports them from its `on_new_investigation` hook — the plugin-API
//!   analog of running before AST investigation. The host still reports
//!   `Murphy/Syntax` and skips the cop pass on parse failure (ADR 0006), so
//!   in `murphy lint` this cop fires on the test harness / partial-AST path
//!   while production syntax errors keep the stable `Murphy/Syntax` contract.
//! ```

use murphy_plugin_api::{Severity, cop, Cx, NoOptions};

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
    fn check_file(&self, cx: &Cx<'_>) {
        for diag in cx.parse_diagnostics() {
            // Host guarantees `message` bytes live for the dispatch call.
            let message =
                String::from_utf8_lossy(unsafe { diag.message.as_bytes() }).into_owned();
            cx.emit_offense(diag.range, &message, Some(Severity::Error));
        }
    }
}

murphy_plugin_api::submit_cop!(Syntax);

#[cfg(test)]
mod tests {
    use super::Syntax;
    use murphy_plugin_api::Severity;
    use murphy_plugin_api::test_support::{run_cop, test};

    #[test]
    fn clean_source_produces_no_offenses() {
        test::<Syntax>().expect_no_offenses("x = 1\n");
    }

    #[test]
    fn reports_prism_diagnostic_as_error() {
        let offenses = run_cop::<Syntax>("def (\n");
        assert!(
            !offenses.is_empty(),
            "prism diagnostics must surface, got none"
        );
        for offense in &offenses {
            assert_eq!(offense.cop_name, "Lint/Syntax");
            assert!(!offense.message.is_empty());
            assert_eq!(offense.severity, Some(Severity::Error));
        }
    }

    #[test]
    fn diagnostic_range_matches_prism_location() {
        // `def (` is a hard parse error; the diagnostic range must be a
        // valid byte span inside the source (not ZERO-clamped).
        let offenses = run_cop::<Syntax>("def (\n");
        let range = offenses[0].range;
        assert!(range.start <= range.end);
        assert!(range.end as usize <= "def (\n".len());
    }
}
