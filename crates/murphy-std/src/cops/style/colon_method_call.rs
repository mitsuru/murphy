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
//!   Uses source-text scanning to detect the `::` separator since Murphy's
//!   Send node does not preserve the double-colon vs dot distinction. The
//!   Java interop guard mirrors RuboCop's `java_type_node?` node matcher
//!   verbatim — `(send (const nil? :Java) _)` — i.e. a Send whose receiver
//!   is the bare `Java` const AND which takes no arguments (the classic
//!   `Java::int` constructor shape). This is an O(1) AST predicate on the
//!   current Send, with no receiver-chain walk. Historical note: an
//!   earlier iteration used `cx.call_receiver` to walk to the chain root,
//!   which both diverged from upstream (`java_type_node?` never walked)
//!   AND turned a linear `Java::a::b::c::...` input into O(N²) CPU (codex
//!   2026-07-02 DoS finding); replacing the walk with the pattern match
//!   fixes both. `camel_case_method?` uses ASCII-uppercase to match
//!   RuboCop's `/\A[A-Z]/`. `autocorrect_incompatible_with [RedundantSelf]`
//!   is corrector-ordering metadata with no expression in Murphy's
//!   single-cop harness.
//!   Residual gap (murphy-nweq): RuboCop's pattern is `(const nil? :Java)`
//!   — nil scope strictly. Murphy's parser collapses the leading-`::` cbase
//!   scope to `None` for receiver constants, so `::Java::foo` is
//!   indistinguishable from `Java::foo` and is wrongly suppressed where
//!   RuboCop flags it. Affects only top-level-qualified `Java` interop.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

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
        let Some(recv_id) = cx.call_receiver(node).get() else {
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
        // Java interop guard: match RuboCop's `java_type_node?` pattern
        // `(send (const nil? :Java) _)` — bare `Java` const receiver AND
        // no arguments (the `Java::int` constructor shape).
        if java_interop(node, cx) {
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

/// Matches RuboCop's `java_type_node?` node pattern `(send (const nil? :Java) _)`:
/// this Send has a bare `Java` const receiver AND takes no arguments. That
/// pattern is a direct AST predicate on the current Send — RuboCop does not
/// walk the receiver chain — so the check is O(1). The nil-scope match is
/// deliberate: `cx.is_global_const` also accepts cbase (`::Java`), which
/// RuboCop's `nil?` predicate rejects. Murphy's parser collapses the cbase
/// scope to `None`, so `::Java::foo` still slips through (murphy-nweq).
fn java_interop(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.has_call_arguments(node) {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    matches!(
        *cx.kind(recv),
        NodeKind::Const { scope, name }
            if scope.get().is_none() && cx.symbol_str(name) == "Java"
    )
}

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
        // `Java::foo` — the Send matches `(send (const nil? :Java) _)`:
        // bare `Java` const receiver, no args. RuboCop's `java_type_node?`
        // matches and leaves the `::` alone.
        test::<ColonMethodCall>().expect_no_offenses("Java::foo\n");
    }

    #[test]
    fn flags_java_call_with_arguments() {
        // `Java::foo(x)` — receiver is the bare `Java` const, but the Send
        // takes an argument. `java_type_node?`'s trailing `_` matches only
        // arg-less sends, so RuboCop flags this. Pins the args-emptiness
        // check that distinguishes the correct match from a naive
        // receiver-only check.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java::foo(x)
                    ^^ Do not use `::` for method calls.
            "},
            "Java.foo(x)\n",
        );
    }

    #[test]
    fn accepts_java_interop_constructor() {
        // `Java::int.new(1)` — the inner `Java::int` Send matches
        // `java_type_node?` (bare `Java`, no args) and is suppressed.
        // The outer `.new(1)` uses `.` not `::` so it isn't checked.
        test::<ColonMethodCall>().expect_no_offenses("Java::int.new(1)\n");
    }

    #[test]
    fn flags_java_interop_chain_outer() {
        // `Java::foo::bar` — the inner `Java::foo` matches `java_type_node?`
        // and is suppressed. The outer `::bar` has a Send receiver
        // (`Java::foo`), not `(const nil? :Java)`, so it does NOT match and
        // RuboCop flags it. This is the discriminator against the historical
        // walk-based check that (wrongly) suppressed the outer link too.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java::foo::bar
                         ^^ Do not use `::` for method calls.
            "},
            "Java::foo.bar\n",
        );
    }

    #[test]
    fn flags_java_interop_deep_chain() {
        // `Java::a::b::c` — inner `Java::a` suppressed; `::b` and `::c` both
        // flagged because their receivers are Sends, not bare `Java` consts.
        // Also serves as a functional smoke test for the O(N²) DoS fix
        // (codex-2026-07-02): dispatching the cop on every Send in a chain
        // used to walk the receiver chain each time; with the direct
        // `java_type_node?` predicate it is O(1) per Send.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java::a::b::c
                       ^^ Do not use `::` for method calls.
                          ^^ Do not use `::` for method calls.
            "},
            "Java::a.b.c\n",
        );
    }

    #[test]
    fn flags_non_java_capitalized_receiver() {
        // `Object::foo` — receiver is a Const but not bare `Java`; flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Object::foo
                      ^^ Do not use `::` for method calls.
            "},
            "Object.foo\n",
        );
    }

    #[test]
    fn flags_identifier_starting_with_java_prefix() {
        // `Javanese::foo` — receiver is `Javanese`, not `Java`, so
        // `java_type_node?` does not match. Flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Javanese::foo
                        ^^ Do not use `::` for method calls.
            "},
            "Javanese.foo\n",
        );
    }

    #[test]
    fn flags_java_as_namespaced_const() {
        // `SomeMod::Java::foo` — receiver Const is `SomeMod::Java`; its
        // scope is `Some(SomeMod)`, not `nil`, so `(const nil? :Java)` does
        // not match. Flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                SomeMod::Java::foo
                             ^^ Do not use `::` for method calls.
            "},
            "SomeMod::Java.foo\n",
        );
    }

    #[test]
    fn flags_java_indexed_root() {
        // `Java[0]::foo` — outer `::foo`'s receiver is a Send (the `[]`
        // call), not a Const, so `java_type_node?` does not match. Flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java[0]::foo
                       ^^ Do not use `::` for method calls.
            "},
            "Java[0].foo\n",
        );
    }

    #[test]
    fn flags_java_paren_method_call() {
        // `Java(1)::foo` — `Java(1)` is a nil-receiver method call named
        // `Java`, not the bare `Java` const, so the outer `::foo`'s receiver
        // is a Send, not `(const nil? :Java)`. Flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java(1)::foo
                       ^^ Do not use `::` for method calls.
            "},
            "Java(1).foo\n",
        );
    }

    #[test]
    fn cbase_java_receiver_parser_limited() {
        // RuboCop's pattern is `(const nil? :Java)` — nil scope strictly, so
        // `::Java::foo` (an explicit top-level `::Java`) has cbase scope and
        // does NOT match; RuboCop flags it. Murphy's parser collapses the
        // leading-`::` cbase scope to `None` for receiver constants, so
        // `::Java` is indistinguishable from a bare `Java` const here and
        // the Java-interop guard suppresses the offense.
        // Residual parity gap tracked in murphy-nweq.
        test::<ColonMethodCall>().expect_no_offenses("::Java::foo\n");
    }
}
murphy_plugin_api::submit_cop!(ColonMethodCall);
