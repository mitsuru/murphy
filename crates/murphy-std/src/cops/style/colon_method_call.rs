//! `Style/ColonMethodCall` — checks for `::` used for method calls instead of `.`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ColonMethodCall
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-nweq]
//! notes: >
//!   Uses source-text scanning since Murphy's Send node does not preserve
//!   the double-colon vs dot distinction. Java interop guard mirrors
//!   RuboCop's `java_interop?`: the receiver chain is walked down to its
//!   root via `cx.call_receiver`, and the `::` is left alone when the root
//!   is a bare `Java` constant (strictly nil scope — `is_global_const` is
//!   deliberately avoided because it also accepts `::Java` / cbase, which
//!   RuboCop flags). `camel_case_method?` uses ASCII-uppercase to match
//!   RuboCop's `/\A[A-Z]/`. `autocorrect_incompatible_with [RedundantSelf]`
//!   is corrector-ordering metadata with no expression in Murphy's
//!   single-cop harness.
//!   Residual gap (murphy-nweq): Murphy's parser collapses the leading-`::`
//!   cbase scope to `None` for receiver constants, so `::Java::foo` is
//!   indistinguishable from `Java::foo` and is wrongly suppressed where
//!   RuboCop flags it. Affects only top-level-qualified `Java` interop.
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
        // RuboCop's `camel_case_method?` is `/\A[A-Z]/`, ASCII-only.
        if cx
            .method_name(node)
            .and_then(|m| m.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            return;
        }
        // Java interop guard: walk the receiver chain to its root and leave
        // the `::` alone when the root is a bare `Java` constant
        // (`Java::int.new(1)`). Mirrors RuboCop's `java_interop?`.
        if java_interop(recv_id, cx) {
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

/// Mirrors RuboCop's `java_interop?`: descend the receiver chain to its root
/// (const nodes have no `.receiver`, so `cx.call_receiver` stops there) and
/// return whether the root is a bare `Java` constant.
fn java_interop(receiver: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = receiver;
    while let Some(inner) = cx.call_receiver(current).get() {
        current = inner;
    }
    // `java_root?` is strictly `(const nil? :Java)` — nil scope only. We avoid
    // `cx.is_global_const`, which also accepts cbase (`::Java`), a case RuboCop
    // flags rather than suppresses.
    matches!(
        *cx.kind(current),
        NodeKind::Const { scope, name }
            if scope.get().is_none() && cx.symbol_str(name) == "Java"
    )
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

    #[test]
    fn accepts_java_interop_bare_method() {
        // `Java::foo` — root receiver is the bare `Java` constant; RuboCop's
        // `java_interop?` guard leaves the `::` alone.
        test::<ColonMethodCall>().expect_no_offenses("Java::foo\n");
    }

    #[test]
    fn accepts_java_interop_constructor() {
        test::<ColonMethodCall>().expect_no_offenses("Java::int.new(1)\n");
    }

    #[test]
    fn accepts_java_interop_chain() {
        // Both `::` are suppressed because the chain root is the bare `Java`
        // constant.
        test::<ColonMethodCall>().expect_no_offenses("Java::foo::bar\n");
    }

    #[test]
    fn flags_non_java_capitalized_receiver() {
        // `Object::foo` — receiver is a constant but not `Java`; flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Object::foo
                      ^^ Do not use `::` for method calls.
            "},
            "Object.foo\n",
        );
    }

    #[test]
    fn cbase_java_receiver_parser_limited() {
        // RuboCop's `java_root?` is strictly nil scope, so `::Java::foo` (an
        // explicit top-level `::Java`) is NOT Java interop and RuboCop flags
        // it. Murphy's parser collapses the leading-`::` cbase scope to `None`
        // for receiver constants, so `::Java` is indistinguishable from a bare
        // `Java` const here and the Java-interop guard suppresses the offense.
        // Residual parity gap tracked in murphy-nweq.
        test::<ColonMethodCall>().expect_no_offenses("::Java::foo\n");
    }
}
murphy_plugin_api::submit_cop!(ColonMethodCall);
