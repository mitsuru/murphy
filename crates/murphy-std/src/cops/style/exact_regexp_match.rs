//! `Style/ExactRegexpMatch` — use `==` / `!=` instead of regexp exact-match.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ExactRegexpMatch
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `=~`, `!~`, `===`, `match`, and `match?` calls where the regexp
//!   argument is a plain anchored literal: `/\Astring\z/` (no flags,
//!   no interpolation, no regexp metacharacters in the literal content).
//!
//!   Detection: the regexp node must have exactly one `Str` part whose raw
//!   source (the interned string content, not including delimiters) begins
//!   with `\A` and ends with `\z`, and the middle portion must contain no
//!   regexp quantifiers or metacharacters. Additionally, the regexp must
//!   have no option flags.
//!
//!   Both `send` and `csend` are handled.
//!
//!   Autocorrect: replace the whole call with `receiver == 'string'` or
//!   `receiver != 'string'`.
//!
//!   Deferred (gaps):
//!     - `/re/ === string` where regexp is the receiver (not an argument)
//!     - `/re/ =~ string` where regexp is the receiver
//!     - Interpolated regexp content (`Dstr` parts)
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! string =~ /\Astring\z/
//! string !~ /\Astring\z/
//! string === /\Astring\z/
//! string.match?(/\Astring\z/)
//! string.match(/\Astring\z/)
//!
//! # good
//! string == 'string'
//! string != 'string'
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Use `%<prefer>s`.";

/// Methods that trigger this cop.
const FLAGGED_METHODS: &[&str] = &["=~", "===", "!~", "match", "match?"];

/// Stateless unit struct.
#[derive(Default)]
pub struct ExactRegexpMatch;

#[cop(
    name = "Style/ExactRegexpMatch",
    description = "Checks for exact regexp match inside Regexp literals.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ExactRegexpMatch {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // Must have a receiver (the string being tested).
    let Some(recv_id) = receiver.get() else {
        return;
    };

    let method_str = cx.symbol_str(method);
    if !FLAGGED_METHODS.contains(&method_str) {
        return;
    }

    // Find the regexp argument. For all supported methods, it's arg[0].
    let arg_list = cx.list(args);
    let Some(&regexp_arg) = arg_list.first() else {
        return;
    };

    // The argument must be a regexp node.
    let NodeKind::Regexp { parts, opts } = *cx.kind(regexp_arg) else {
        return;
    };

    // No regexp flags allowed (no `i`, `m`, `x`, etc.).
    if !cx.symbol_str(opts).is_empty() {
        return;
    }

    // Must have exactly one part that is a plain Str (no interpolation).
    let parts_list = cx.list(parts);
    if parts_list.len() != 1 {
        return;
    }
    let NodeKind::Str(str_sym) = *cx.kind(parts_list[0]) else {
        return;
    };
    let str_content = cx.string_str(str_sym);

    // The string must start with `\A` and end with `\z`.
    if !str_content.starts_with(r"\A") || !str_content.ends_with(r"\z") {
        return;
    }

    // Extract the literal content between `\A` and `\z`.
    let literal = &str_content[2..str_content.len() - 2];

    // The literal must not contain any regexp metacharacters or quantifiers.
    if contains_regexp_metachar(literal) {
        return;
    }

    // Build the preferred expression: `receiver == 'literal'` or `receiver != 'literal'`.
    let new_method = if method_str == "!~" { "!=" } else { "==" };
    let receiver_src = cx.raw_source(cx.range(recv_id));
    // Use double quotes if the literal contains single quotes, to avoid
    // producing syntactically invalid Ruby like `x == 'it's'`.
    let quoted_literal = if literal.contains('\'') {
        format!("\"{literal}\"")
    } else {
        format!("'{literal}'")
    };
    let prefer = format!("{receiver_src} {new_method} {quoted_literal}");
    let msg = MSG.replace("%<prefer>s", &prefer);

    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(cx.range(node), &prefer);
}

/// Returns `true` if `s` contains any regexp metacharacters or quantifiers
/// that would make the pattern non-literal (i.e. not a plain string match).
///
/// The input `s` is the parser's interned string content where escape sequences
/// such as `\s` are preserved as two-byte sequences `\` + `s`, not processed
/// characters. A literal backslash in the source (e.g. `\\`) also arrives as `\`
/// here, so we conservatively reject any `\` occurrence.
///
/// Quantifiers: `+`, `*`, `?`, `{`
/// Metacharacters: `(`, `)`, `[`, `]`, `|`, `.`, `^`, `$`, `\`
fn contains_regexp_metachar(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            '+' | '*' | '?' | '{' | '(' | ')' | '[' | ']' | '|' | '.' | '^' | '$' | '\\'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::ExactRegexpMatch;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_tilde_match() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string =~ /\Atest\z/
                ^^^^^^^^^^^^^^^^^^^^ Use `string == 'test'`.
            "#},
            "string == 'test'\n",
        );
    }

    #[test]
    fn flags_negated_tilde_match() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string !~ /\Atest\z/
                ^^^^^^^^^^^^^^^^^^^^ Use `string != 'test'`.
            "#},
            "string != 'test'\n",
        );
    }

    #[test]
    fn flags_case_equality() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string === /\Atest\z/
                ^^^^^^^^^^^^^^^^^^^^^ Use `string == 'test'`.
            "#},
            "string == 'test'\n",
        );
    }

    #[test]
    fn flags_match_predicate() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string.match?(/\Atest\z/)
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `string == 'test'`.
            "#},
            "string == 'test'\n",
        );
    }

    #[test]
    fn flags_match_method() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string.match(/\Atest\z/)
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `string == 'test'`.
            "#},
            "string == 'test'\n",
        );
    }

    #[test]
    fn accepts_regexp_with_quantifier() {
        // `+` makes it non-literal.
        test::<ExactRegexpMatch>().expect_no_offenses("string =~ /\\Atest+\\z/\n");
    }

    #[test]
    fn accepts_regexp_without_anchors() {
        test::<ExactRegexpMatch>().expect_no_offenses("string =~ /test/\n");
    }

    #[test]
    fn accepts_regexp_with_flags() {
        // `i` flag → not an exact literal match.
        test::<ExactRegexpMatch>().expect_no_offenses("string =~ /\\Atest\\z/i\n");
    }

    #[test]
    fn accepts_already_string_equality() {
        test::<ExactRegexpMatch>().expect_no_offenses("string == 'test'\n");
    }

    #[test]
    fn flags_literal_with_space() {
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string =~ /\Ahello world\z/
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `string == 'hello world'`.
            "#},
            "string == 'hello world'\n",
        );
    }

    #[test]
    fn accepts_regexp_with_metachar() {
        // `.` is a metacharacter.
        test::<ExactRegexpMatch>().expect_no_offenses("string =~ /\\Ahello.world\\z/\n");
    }
}

murphy_plugin_api::submit_cop!(ExactRegexpMatch);
