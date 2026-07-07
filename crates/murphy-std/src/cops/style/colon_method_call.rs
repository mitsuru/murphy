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
//!   RuboCop's `java_interop?` but avoids walking the AST receiver chain
//!   per Send node — Ruby method chains are left-associative in the AST
//!   but written left-to-right in the source, so the chain root always
//!   sits at `receiver.range.start`. We peek at the source bytes there
//!   for a bare `Java` identifier followed by a `::` or `.` link, giving
//!   O(1) per Send instead of the O(N) walk (a linear input previously
//!   triggered O(N²) CPU — codex 2026-07-02 DoS finding). Since Murphy's
//!   parser collapses leading `::` cbase scope to `None`, the check
//!   intentionally accepts both `Java` and `::Java` at the root, matching
//!   the pre-fix suppression behavior. `camel_case_method?` uses
//!   ASCII-uppercase to match RuboCop's `/\A[A-Z]/`.
//!   `autocorrect_incompatible_with [RedundantSelf]` is corrector-ordering
//!   metadata with no expression in Murphy's single-cop harness.
//!   Residual gap (murphy-nweq): Murphy's parser collapses the leading-`::`
//!   cbase scope to `None` for receiver constants, so `::Java::foo` is
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

/// Mirrors RuboCop's `java_interop?`: the receiver chain is Java interop when
/// its root is a bare `Java` constant. Because Ruby method chains are
/// left-associative in the AST but written left-to-right in the source, the
/// root of a chain always sits at `receiver.range.start`. We peek at the
/// source bytes there for a bare `Java` identifier followed by a chain link
/// (`::`, `.`, or `[` — RuboCop's `java_receiver` recurses through `[]`
/// sends, so `Java[0]::foo` is Java interop). This is O(1) per Send instead
/// of the O(N) receiver-chain walk (the walk turned linear input into O(N²)
/// CPU — codex 2026-07-02 DoS finding).
///
/// Murphy's parser collapses the leading-`::` cbase scope to `None` for
/// receiver constants (see `translate_constant_path`), so this function
/// intentionally accepts both `Java` and `::Java` at the chain root to
/// preserve the pre-fix behavior. The residual parity gap where RuboCop
/// flags `::Java::foo` is tracked separately as murphy-nweq.
fn java_interop(receiver: NodeId, cx: &Cx<'_>) -> bool {
    let root_start = cx.range(receiver).start as usize;
    let bytes = cx.source().as_bytes();
    let after_java = if bytes.get(root_start..root_start + 4) == Some(b"Java") {
        root_start + 4
    } else if bytes.get(root_start..root_start + 6) == Some(b"::Java") {
        root_start + 6
    } else {
        return false;
    };
    // Reject longer identifiers with a `Java` prefix (`Javanese::foo`,
    // `Java_::foo`): the next byte must be a chain link — `::` or `.` for a
    // method call, or `[` for an index on the bare `Java` const (RuboCop's
    // `java_receiver` recurses through `[]` sends, so `Java[0]::foo` is Java
    // interop and must remain suppressed). A `(` explicitly is NOT a chain
    // link here: `Java(1)::foo` calls a nil-receiver method named `Java`,
    // whose root is not the bare `Java` const, and RuboCop flags it.
    matches!(bytes.get(after_java), Some(&b':') | Some(&b'.') | Some(&b'['))
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
    fn flags_identifier_starting_with_java_prefix() {
        // `Javanese::foo` — root identifier is `Javanese`, not `Java`. The
        // `Java` prefix must not be mistaken for the bare `Java` constant.
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
        // `SomeMod::Java::foo` — the receiver Const is `SomeMod::Java`. Its
        // short name is `Java` but namespace is `SomeMod`, so RuboCop's
        // `java_root?` returns false and the `::foo` is flagged.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                SomeMod::Java::foo
                             ^^ Do not use `::` for method calls.
            "},
            "SomeMod::Java.foo\n",
        );
    }

    #[test]
    fn accepts_java_interop_indexed_root() {
        // `Java[0]::foo` — the chain root is the bare `Java` const, reached
        // through an `[]` send. RuboCop's `java_receiver` recurses through
        // any Send receiver (including `[]`) so this is Java interop.
        test::<ColonMethodCall>().expect_no_offenses("Java[0]::foo\n");
    }

    #[test]
    fn flags_java_paren_method_call() {
        // `Java(1)::foo` — `Java(1)` is a nil-receiver method call named
        // `Java`, not the bare `Java` const. RuboCop's `java_root?` requires
        // a `const` node at the chain root, so this is flagged, not
        // suppressed. This pins the `(` boundary: it must NOT be treated as
        // a chain link like `[` is.
        test::<ColonMethodCall>().expect_correction(
            indoc! {"
                Java(1)::foo
                       ^^ Do not use `::` for method calls.
            "},
            "Java(1).foo\n",
        );
    }

    #[test]
    fn accepts_deep_java_interop_chain() {
        // Functional smoke test for the O(N²) DoS fix: a longer Java chain
        // must remain suppressed. Prior to the fix, each Send in the chain
        // triggered a full receiver-chain walk, so a linear input caused
        // quadratic CPU (codex-2026-07-02 DoS finding).
        test::<ColonMethodCall>().expect_no_offenses("Java::a::b::c::d::e::f\n");
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
