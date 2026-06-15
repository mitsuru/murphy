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
//!   the double-colon vs dot distinction. Java interop guard omitted (v1 gap).
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
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let recv_end = cx.range(recv_id).end;
        if !cx.source()[recv_end as usize..].starts_with("::") {
            return;
        }
        // A method name beginning with an uppercase letter is ambiguous with
        // constant access (`Nokogiri::HTML5(...)`); RuboCop leaves the `::`.
        if cx
            .method_name(node)
            .and_then(|m| m.chars().next())
            .is_some_and(|c| c.is_uppercase())
        {
            return;
        }
        let colon_range = Range {
            start: recv_end,
            end: recv_end + 2,
        };
        cx.emit_offense(colon_range, MSG, None);
        cx.emit_edit(colon_range, ".");
    }
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
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                FileUtils::rmdir(dir)
                         ^^ Do not use `::` for method calls.
            "},
            "FileUtils.rmdir(dir)\n",
        );
    }

    #[test]
    fn flags_timeout() {
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Timeout::timeout(500) { do_something }
                       ^^ Do not use `::` for method calls.
            "},
            "Timeout.timeout(500) { do_something }\n",
        );
    }

    #[test]
    fn accepts_dot_method_call() {
        test::<ColonMethodCall>().expect_no_offenses("Timeout.timeout(500) { do_something }\n");
    }

    #[test]
    fn accepts_constant_ref() {
        test::<ColonMethodCall>().expect_no_offenses("x = Foo::Bar\n");
    }

    #[test]
    fn accepts_capitalized_method_name() {
        // `Nokogiri::HTML5(html)` calls a method whose name begins with an
        // uppercase letter; that is ambiguous with constant access, so RuboCop
        // (and Murphy) leave the `::` alone.
        test::<ColonMethodCall>().expect_no_offenses("Nokogiri::HTML5(html)\n");
        test::<ColonMethodCall>().expect_no_offenses("doc = Nokogiri::XML(str)\n");
    }
}
murphy_plugin_api::submit_cop!(ColonMethodCall);
