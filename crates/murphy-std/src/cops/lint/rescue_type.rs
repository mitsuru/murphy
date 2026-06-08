//! `Lint/RescueType` — checks invalid literal exception classes in `rescue`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RescueType
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port reports literal rescue exception arguments that would
//!   raise `TypeError`. RuboCop's autocorrection that removes invalid entries
//!   from mixed rescue lists is a documented v1 gap.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG_PREFIX: &str = "Rescuing from `";
const MSG_SUFFIX: &str = "` will raise a `TypeError` instead of catching the actual exception.";

#[derive(Default)]
pub struct RescueType;

#[cop(
    name = "Lint/RescueType",
    description = "Checks invalid literal exception classes in rescue clauses.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RescueType {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody { exceptions, .. } = *cx.kind(node) else {
            return;
        };
        let invalid: Vec<NodeId> = cx
            .list(exceptions)
            .iter()
            .copied()
            .filter(|&exception| is_invalid_exception(exception, cx))
            .collect();
        let (Some(first), Some(last)) = (invalid.first(), invalid.last()) else {
            return;
        };

        let invalid_sources: Vec<&str> = invalid
            .iter()
            .map(|&exception| cx.raw_source(cx.range(exception)))
            .collect();
        let message = format!("{MSG_PREFIX}{}{MSG_SUFFIX}", invalid_sources.join(", "));
        cx.emit_offense(
            Range {
                start: cx.range(*first).start,
                end: cx.range(*last).end,
            },
            &message,
            None,
        );
    }
}

fn is_invalid_exception(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Array(_)
            | NodeKind::Complex(_)
            | NodeKind::Dstr(_)
            | NodeKind::False_
            | NodeKind::Float(_)
            | NodeKind::Hash(_)
            | NodeKind::Nil
            | NodeKind::Int(_)
            | NodeKind::Rational(_)
            | NodeKind::Str(_)
            | NodeKind::Sym(_)
            | NodeKind::True_
    )
}

murphy_plugin_api::submit_cop!(RescueType);

#[cfg(test)]
mod tests {
    use super::RescueType;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_nil_rescue_type() {
        test::<RescueType>().expect_offense(indoc! {r#"
            begin
              work
            rescue nil
                   ^^^ Rescuing from `nil` will raise a `TypeError` instead of catching the actual exception.
              recover
            end
        "#});
    }

    #[test]
    fn flags_multiple_invalid_rescue_types() {
        test::<RescueType>().expect_offense(indoc! {r#"
            begin
              work
            rescue 1, 'a'
                   ^^^^^^ Rescuing from `1, 'a'` will raise a `TypeError` instead of catching the actual exception.
              recover
            end
        "#});
    }

    #[test]
    fn accepts_class_rescue_type() {
        test::<RescueType>()
            .expect_no_offenses("begin\n  work\nrescue NameError\n  recover\nend\n");
    }
}
