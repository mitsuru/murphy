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
//!   Verbatim port of the call head `(call _ {:=~ :=== :!~ :match :match?} ...)`
//!   (murphy-s1yc.12): `call` covers safe-navigation (`string&.match?`),
//!   mirroring RuboCop `alias on_csend on_send` plus `RESTRICT_ON_SEND`; the
//!   wildcard receiver binds an absent or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list. The inner
//!   `(regexp (str $_) (regopt))` child is not expressible in murphy
//!   NodePattern v1, so the regexp-shape plus anchor plus metachar guards
//!   below apply separately, mirroring upstream `return unless node.receiver`
//!   plus `parse_regexp` plus `exact_match_pattern?`.
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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.12):
// `(call _ {:=~ :=== :!~ :match :match?} ...)` — `call` = `{send csend}`
// covers safe-navigation (`string&.match?`), mirroring RuboCop `alias
// on_csend on_send` plus `RESTRICT_ON_SEND`. The `_` receiver binds an
// absent or present receiver per murphy-if9y; trailing `...` absorbs any
// argument list, so the receiver plus regexp-shape plus anchor plus
// metachar guards below apply separately (mirroring upstream `return
// unless node.receiver` plus `parse_regexp` plus `exact_match_pattern?`).
// The inner `(regexp (str $_) (regopt))` child is not expressible in
// murphy NodePattern v1 (`regexp`/`str`/`regopt` are outside
// SUPPORTED_TAGS), so it stays a hand-rolled complementary guard.
def_node_matcher!(
    exact_regexp_match_call,
    "(call _ {:=~ :=== :!~ :match :match?} ...)"
);

const MSG: &str = "Use `%<prefer>s`.";

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
    // Verbatim `(call _ {:=~ :=== :!~ :match :match?} ...)` head: filters to
    // the five flagged methods on either send or csend (safe-navigation),
    // with any receiver (absent or present). Without this, an unrelated
    // call over a regexp argument (e.g. `string.search(/\\Atest\\z/)`)
    // would run check on every call node instead of being rejected by the
    // method set up front.
    if !exact_regexp_match_call(node, cx) {
        return;
    }

    // Must have a receiver (the string being tested). Mirrors upstream
    // `return unless node.receiver`; `_` binds an absent receiver, so bare
    // `match(/.../)` is accepted here.
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };

    let method_str = cx.method_name(node).unwrap_or_default();

    // Find the regexp argument. For all supported methods, it's arg[0].
    // Trailing `...` absorbs any argument list, so the no-argument case
    // (`string.match`) is accepted here.
    let arg_list = cx.call_arguments(node);
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
    // Escape single quotes in the literal so the single-quoted Ruby string
    // remains syntactically valid (e.g., "it's" -> 'it\'s').
    let escaped_literal = literal.replace('\'', "\\'");
    let prefer = format!("{receiver_src} {new_method} '{escaped_literal}'");
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

    // --- Characterization (murphy-s1yc.12): pin the exact node set the
    // hand-rolled send dispatch matches, so the verbatim
    // `(call _ {:=~ :=== :!~ :match :match?} ...)` port can be proven
    // byte-identical. `call` covers safe-navigation (mirroring upstream
    // `alias on_csend on_send`); trailing `...` absorbs any argument list,
    // so the receiver plus regexp-shape plus anchor plus metachar guards
    // below apply separately (mirroring upstream `return unless receiver`
    // plus `parse_regexp` plus `exact_match_pattern?`).

    #[test]
    fn s1yc12_flags_csend_match_predicate_corrects() {
        // Safe navigation: `call` covers `csend` per murphy-if9y, mirroring
        // upstream `alias on_csend on_send`. (Pre-port the `csend` handler
        // is dead code: `check` destructures `NodeKind::Send` only.)
        test::<ExactRegexpMatch>().expect_correction(
            indoc! {r#"
                string&.match?(/\Atest\z/)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `string == 'test'`.
            "#},
            "string == 'test'\n",
        );
    }

    #[test]
    fn s1yc12_accepts_bare_match() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard (mirroring
        // upstream `return unless node.receiver`) accepts.
        test::<ExactRegexpMatch>().expect_no_offenses("match(/\\Atest\\z/)\n");
    }

    #[test]
    fn s1yc12_accepts_no_arg_match() {
        // No arguments: trailing `...` absorbs the empty list, so the head
        // matches and the complementary regexp-arg guard accepts.
        test::<ExactRegexpMatch>().expect_no_offenses("string.match\n");
    }

    #[test]
    fn s1yc12_accepts_unrelated_method() {
        // `search` is outside the verbatim method set, so the head rejects.
        test::<ExactRegexpMatch>().expect_no_offenses("string.search(/\\Atest\\z/)\n");
    }
}

murphy_plugin_api::submit_cop!(ExactRegexpMatch);
