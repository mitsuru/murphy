//! `Style/CollectionQuerying` — prefer predicate methods over `count` comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CollectionQuerying
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles count.positive?, count > 0, count != 0 → any?,
//!   count.zero?, count == 0 → none?,
//!   count == 1 → one?.
//!   count(&:foo?).positive? block form is a v1 gap.
//!   ActiveSupportExtensions (many?) not handled.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct CollectionQuerying;

#[cop(
    name = "Style/CollectionQuerying",
    description = "Use predicate methods instead of `count` comparisons.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CollectionQuerying {
    #[on_node(kind = "send", methods = ["positive?", ">", "!=", "zero?", "=="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let NodeKind::Send { receiver: count_recv, method: count_method, .. } = *cx.kind(recv_id) else {
            return;
        };
        let count_method_str = cx.symbol_str(count_method);
        if count_method_str != "count" {
            return;
        }
        if count_recv == murphy_plugin_api::OptNodeId::NONE {
            return;
        }
        let method_str = cx.symbol_str(method);
        let replacement = match method_str {
            "positive?" => Some("any?"),
            ">" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else if cx.raw_source(cx.range(arg_list[0])) == "0" { Some("any?") }
                else { None }
            }
            "!=" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else if cx.raw_source(cx.range(arg_list[0])) == "0" { Some("any?") }
                else { None }
            }
            "zero?" => Some("none?"),
            "==" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else {
                    match cx.raw_source(cx.range(arg_list[0])) {
                        "0" => Some("none?"),
                        "1" => Some("one?"),
                        _ => None,
                    }
                }
            }
            _ => None,
        };
        if let Some(repl) = replacement {
            cx.emit_offense(
                cx.range(node),
                &format!("Use `{}` instead of `count` comparison.", repl),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CollectionQuerying;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_count_positive() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count.positive?
            ^^^^^^^^^^^^^^^^^ Use `any?` instead of `count` comparison.
        "});
    }

    #[test]
    fn flags_count_gt_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count > 0
            ^^^^^^^^^^^ Use `any?` instead of `count` comparison.
        "});
    }

    #[test]
    fn flags_count_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count.zero?
            ^^^^^^^^^^^^^ Use `none?` instead of `count` comparison.
        "});
    }

    #[test]
    fn flags_count_eq_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count == 0
            ^^^^^^^^^^^^ Use `none?` instead of `count` comparison.
        "});
    }

    #[test]
    fn flags_count_eq_one() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count == 1
            ^^^^^^^^^^^^ Use `one?` instead of `count` comparison.
        "});
    }

    #[test]
    fn accepts_any() {
        test::<CollectionQuerying>().expect_no_offenses("x.any?\n");
    }
}
murphy_plugin_api::submit_cop!(CollectionQuerying);
