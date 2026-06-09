//! `Style/ColonMethodCall` — checks for `::` used for method calls instead of `.`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ColonMethodCall
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Uses source-text scanning since Murphy's Send node does not preserve
//!   the double-colon vs dot distinction. Java interop guard omitted.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "Do not use `::` for method calls.";

#[derive(Default)]
pub struct ColonMethodCall;

#[cop(
    name = "Style/ColonMethodCall",
    description = "Do not use `::` for method calls.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ColonMethodCall {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let method_str = cx.symbol_str(method);
        if method_str.starts_with(char::is_uppercase) {
            return;
        }
        if is_java_type_node(node, cx) {
            return;
        }
        let recv_end = cx.range(recv_id).end;
        let gap_src = cx.raw_source(Range {
            start: recv_end,
            end: recv_end + 2,
        });
        if gap_src != "::" {
            return;
        }
        let dot_range = Range {
            start: recv_end,
            end: recv_end + 1,
        };
        let colon_range = Range {
            start: recv_end,
            end: recv_end + 2,
        };
        cx.emit_offense(colon_range, MSG, None);
        cx.emit_edit(dot_range, ".");
        cx.emit_edit(colon_range, ".");
    }
}

fn is_java_type_node(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return false;
    };
    let method_str = cx.symbol_str(method);
    if !method_str.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    matches!(cx.kind(recv_id), NodeKind::Const { .. })
}

const _: () = {
    let _ = std::mem::size_of::<OptNodeId>();
};

#[cfg(test)]
mod tests {
    use super::ColonMethodCall;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_colon_method_call() {
        test::<ColonMethodCall>().expect_offense(indoc! {"
            Timeout.timeout(500) { do_something }
        "});
    }

    #[test]
    fn accepts_dot_method_call() {
        test::<ColonMethodCall>().expect_no_offenses("Timeout.timeout(500) { do_something }\n");
    }

    #[test]
    fn accepts_constant_ref() {
        test::<ColonMethodCall>().expect_no_offenses("x = Foo::Bar\n");
    }
}
murphy_plugin_api::submit_cop!(ColonMethodCall);
