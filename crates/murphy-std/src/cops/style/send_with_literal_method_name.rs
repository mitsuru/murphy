//! `Style/SendWithLiteralMethodName` — flags `send`/`public_send`/`__send__`
//! calls with a literal method name argument and suggests a direct call.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SendWithLiteralMethodName
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   AllowSend: true (default) — only `public_send` is flagged by default.
//!   When AllowSend: false, `send` and `__send__` are also flagged.
//!   Writer methods ending in `=` are always allowed (their behavior
//!   differs from direct assignment).
//!   Method name must match /\A[a-zA-Z_][a-zA-Z0-9_]*[!?]?\z/ and must
//!   not be a Ruby reserved word.
//!   Both sym and str literal first arguments are accepted.
//!   Autocorrect: single-arg → replace selector-to-end with method name;
//!   multi-arg → rename selector + remove first-arg range up to second arg.
//!   Both send and csend (safe-navigation) are handled.
//!   This cop is marked Safe: false upstream (cannot detect private method calls).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! obj.public_send(:foo)
//! obj.public_send('foo')
//!
//! # good
//! obj.foo
//!
//! # also bad (with AllowSend: false)
//! obj.send(:foo)
//! obj.__send__(:foo)
//!
//! # always allowed
//! obj.send(:foo=, val)   # writer method
//! obj.public_send(:if)   # reserved word
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SendWithLiteralMethodName;

/// Ruby reserved words that cannot be used as bare method calls.
const RESERVED_WORDS: &[&str] = &[
    "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "defined?",
    "do", "else", "elsif", "end", "ensure", "false", "for", "if", "in", "module", "next",
    "nil", "not", "or", "redo", "rescue", "retry", "return", "self", "super", "then",
    "true", "undef", "unless", "until", "when", "while", "yield",
];

/// Valid method name pattern: starts with letter or `_`, followed by word chars,
/// optionally ending with `!` or `?`.
fn is_valid_method_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    // First char: letter or underscore
    if !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_' {
        return false;
    }
    // Middle chars: word chars
    let end = if matches!(bytes.last(), Some(b'!' | b'?')) {
        bytes.len() - 1
    } else {
        bytes.len()
    };
    for &b in &bytes[1..end] {
        if !b.is_ascii_alphanumeric() && b != b'_' {
            return false;
        }
    }
    // Not a reserved word
    if RESERVED_WORDS.contains(&name) {
        return false;
    }
    true
}

#[derive(CopOptions)]
pub struct SendWithLiteralMethodNameOptions {
    #[option(
        name = "AllowSend",
        default = true,
        description = "When `true` (default), only `public_send` is checked. \
                       When `false`, `send` and `__send__` are also checked."
    )]
    pub allow_send: bool,
}

#[cop(
    name = "Style/SendWithLiteralMethodName",
    description = "Detects the use of `public_send` (and optionally `send`/`__send__`) \
                   with a literal method name argument and suggests a direct method call.",
    default_severity = "warning",
    default_enabled = true,
    options = SendWithLiteralMethodNameOptions,
)]
impl SendWithLiteralMethodName {
    #[on_node(kind = "send", methods = ["public_send", "send", "__send__"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let method = cx.method_name(node);
        if matches!(method, Some("public_send" | "send" | "__send__")) {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<SendWithLiteralMethodNameOptions>();
    let method_name = cx.method_name(node).unwrap_or("");

    // When AllowSend is true, only public_send is checked.
    if opts.allow_send && method_name != "public_send" {
        return;
    }

    // Must have at least one argument.
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }

    let first_arg = args[0];

    // First argument must be a symbol or string literal.
    let literal_name = match *cx.kind(first_arg) {
        NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => return,
    };

    // Must be a valid method name (not a writer, not a reserved word).
    if literal_name.ends_with('=') {
        return;
    }
    if !is_valid_method_name(&literal_name) {
        return;
    }

    // Offense range: from selector start to node end.
    let selector_range = cx.node(node).loc.name;
    let offense_range = Range {
        start: selector_range.start,
        end: cx.range(node).end,
    };

    let msg = format!("Use `{literal_name}` method call directly instead.");
    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect
    if args.len() == 1 {
        // Single arg: replace selector-to-end with method name only.
        cx.emit_edit(offense_range, &literal_name);
    } else {
        // Multiple args: rename selector + remove first arg up to second arg.
        cx.emit_edit(selector_range, &literal_name);
        let removal = Range {
            start: cx.range(first_arg).start,
            end: cx.range(args[1]).start,
        };
        cx.emit_edit(removal, "");
    }
}

#[cfg(test)]
mod tests {
    use super::{SendWithLiteralMethodName, SendWithLiteralMethodNameOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Default (AllowSend: true) — only public_send flagged -----

    #[test]
    fn flags_public_send_sym() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj.public_send(:foo)
                ^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
        "#});
    }

    #[test]
    fn flags_public_send_str() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj.public_send('foo')
                ^^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
        "#});
    }

    #[test]
    fn corrects_public_send_single_arg() {
        test::<SendWithLiteralMethodName>().expect_correction(
            indoc! {r#"
                obj.public_send(:foo)
                    ^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
            "#},
            "obj.foo\n",
        );
    }

    #[test]
    fn flags_public_send_multiple_args() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj.public_send(:foo, bar)
                ^^^^^^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
        "#});
    }

    #[test]
    fn corrects_public_send_multiple_args() {
        test::<SendWithLiteralMethodName>().expect_correction(
            indoc! {r#"
                obj.public_send(:foo, bar)
                    ^^^^^^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
            "#},
            "obj.foo(bar)\n",
        );
    }

    #[test]
    fn flags_public_send_bang_method() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj.public_send(:save!)
                ^^^^^^^^^^^^^^^^^^^ Use `save!` method call directly instead.
        "#});
    }

    #[test]
    fn corrects_public_send_bang_method() {
        test::<SendWithLiteralMethodName>().expect_correction(
            indoc! {r#"
                obj.public_send(:save!)
                    ^^^^^^^^^^^^^^^^^^^ Use `save!` method call directly instead.
            "#},
            "obj.save!\n",
        );
    }

    #[test]
    fn flags_public_send_predicate_method() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj.public_send(:valid?)
                ^^^^^^^^^^^^^^^^^^^^ Use `valid?` method call directly instead.
        "#});
    }

    // ----- Default: send is allowed when AllowSend: true -----

    #[test]
    fn accepts_send_with_allow_send_true() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.send(:foo)\n");
    }

    #[test]
    fn accepts_dunder_send_with_allow_send_true() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.__send__(:foo)\n");
    }

    // ----- AllowSend: false — send and __send__ also flagged -----

    #[test]
    fn flags_send_when_allow_send_false() {
        test::<SendWithLiteralMethodName>()
            .with_options(&SendWithLiteralMethodNameOptions { allow_send: false })
            .expect_offense(indoc! {r#"
                obj.send(:foo)
                    ^^^^^^^^^^ Use `foo` method call directly instead.
            "#});
    }

    #[test]
    fn corrects_send_when_allow_send_false() {
        test::<SendWithLiteralMethodName>()
            .with_options(&SendWithLiteralMethodNameOptions { allow_send: false })
            .expect_correction(
                indoc! {r#"
                    obj.send(:foo)
                        ^^^^^^^^^^ Use `foo` method call directly instead.
                "#},
                "obj.foo\n",
            );
    }

    #[test]
    fn flags_dunder_send_when_allow_send_false() {
        test::<SendWithLiteralMethodName>()
            .with_options(&SendWithLiteralMethodNameOptions { allow_send: false })
            .expect_offense(indoc! {r#"
                obj.__send__(:foo)
                    ^^^^^^^^^^^^^^ Use `foo` method call directly instead.
            "#});
    }

    // ----- Writer methods always allowed -----

    #[test]
    fn accepts_writer_method() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.public_send(:foo=, val)\n");
    }

    // ----- Reserved words always allowed -----

    #[test]
    fn accepts_reserved_word_if() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.public_send(:if)\n");
    }

    #[test]
    fn accepts_reserved_word_class() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.public_send(:class)\n");
    }

    // ----- No argument -----

    #[test]
    fn accepts_no_argument() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.public_send\n");
    }

    // ----- Non-literal first argument -----

    #[test]
    fn accepts_variable_method_name() {
        test::<SendWithLiteralMethodName>().expect_no_offenses("obj.public_send(meth)\n");
    }

    // ----- csend -----

    #[test]
    fn flags_csend_public_send() {
        test::<SendWithLiteralMethodName>().expect_offense(indoc! {r#"
            obj&.public_send(:foo)
                 ^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
        "#});
    }

    #[test]
    fn corrects_csend_public_send() {
        test::<SendWithLiteralMethodName>().expect_correction(
            indoc! {r#"
                obj&.public_send(:foo)
                     ^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
            "#},
            "obj&.foo\n",
        );
    }

    // ----- Multiple args with csend -----

    #[test]
    fn corrects_csend_public_send_multiple_args() {
        test::<SendWithLiteralMethodName>().expect_correction(
            indoc! {r#"
                obj&.public_send(:foo, bar, baz)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `foo` method call directly instead.
            "#},
            "obj&.foo(bar, baz)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(SendWithLiteralMethodName);
