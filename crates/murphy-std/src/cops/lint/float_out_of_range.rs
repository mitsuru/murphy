//! `Lint/FloatOutOfRange` — flag float literals Ruby represents as infinity or underflowed zero.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FloatOutOfRange
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's on_float check for infinite float values and non-zero
//!   literals that underflow to zero. No options or autocorrect.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct FloatOutOfRange;

#[cop(
    name = "Lint/FloatOutOfRange",
    description = "Flag float literals outside Ruby's representable range.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FloatOutOfRange {
    #[on_node(kind = "float")]
    fn check_float(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Float(value) = *cx.kind(node) else { return; };
        let source = cx.raw_source(cx.range(node));
        if value.is_infinite() || (value == 0.0 && source.bytes().any(|b| matches!(b, b'1'..=b'9'))) {
            cx.emit_offense(cx.range(node), "Float out of range.", None);
        }
    }
}

murphy_plugin_api::submit_cop!(FloatOutOfRange);

#[cfg(test)]
mod tests {
    use super::FloatOutOfRange;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_overflow_and_underflow() {
        test::<FloatOutOfRange>()
            .expect_offense(indoc! {r#"
                float = 3.0e400
                        ^^^^^^^ Float out of range.
            "#})
            .expect_offense(indoc! {r#"
                float = 1.0e-400
                        ^^^^^^^^ Float out of range.
            "#});
    }

    #[test]
    fn accepts_representable_floats_and_literal_zero() {
        test::<FloatOutOfRange>()
            .expect_no_offenses("float = 42.9\n")
            .expect_no_offenses("float = 0.0\n");
    }
}
