//! `Style/StringLiterals` — enforces a single quote style for plain
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringLiterals
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Fixed: double_quotes_required? guard (false-positive offenses), runtime
//!   option wiring via cx.options_or_default, message text parity, and
//!   EnforcedStyle/single_quotes/double_quotes config key alignment.
//!   ConsistentQuotesInMultiline defaults to false; deferred (non-default feature).
//!   The `Dstr` node is never subscribed. Its literal `Str` segments (which ARE
//!   `Str` nodes) are skipped in `check_str` unless the segment carries its own
//!   opening quote delimiter, reconstructing RuboCop's `StringHelp#on_str`
//!   `loc?(:begin)` guard positionally (`dstr_segment_is_own_literal`): a bare
//!   content segment of an interpolated / heredoc / percent-literal `Dstr` is
//!   skipped, while each independently-quoted child of an adjacent
//!   concatenation (`"a" "b"`, `%Q[a] "b"`, `"a" "#{b}"`) is flagged per
//!   RuboCop.
//! ```
//!
//! string literals. Mirrors RuboCop's same-named cop.
//!
//! Subscribes to `NodeKind::Str` (plain literal). The `NodeKind::Dstr` node
//! itself (`"a#{b}"`) is not subscribed — it cannot be a single-quoted string.
//! `check_str` additionally skips a `Str` that is a bare content segment of an
//! interpolated / heredoc / percent-literal `Dstr` (and of `Dsym`/`Regexp`/
//! `Xstr`), because those bodies can hold an unescaped quote that is literal
//! content (e.g. HTML attribute quotes in a heredoc, or `'foo'` in
//! `"'foo'#{bar}"`) rather than a string delimiter. Only segments carrying
//! their own quote delimiter — adjacent concatenation — are flagged.
//!
//! ## Option (`EnforcedStyle`)
//!
//! Declared via `#[derive(CopOptions)]` and wired through the cop's
//! `Cop::Options` associated type. v1 ships the default
//! `EnforcedStyle = "single_quotes"` (matching RuboCop). The host-side
//! config-validation gate (murphy-9cr.9) consumes the generated
//! `SCHEMA` to enforce the enum at config-load time; the runtime
//! behaviour uses `cx.options_or_default` so `EnforcedStyle = "double_quotes"`
//! is now reachable via `.murphy.yml`.
//!
//! ## Offense guard (`double_quotes_required?` parity)
//!
//! In `single` mode Murphy only flags a double-quoted string when double
//! quotes are NOT required — i.e. the string body could have been written
//! with single quotes without changing meaning. The guard mirrors
//! RuboCop's `double_quotes_required?` / `wrong_quotes?` logic
//! (rubocop/cop/util.rb:133): double quotes are required when the source
//! contains a single-quote **or** a meaningful backslash escape (an
//! odd-length run of backslashes whose following character is not `\` or
//! `"`).  Similarly, in `double` mode single quotes are required when the
//! source contains `"`, a non-trivial backslash escape `\[^'\\]`, or
//! an interpolation anchor `#@`, `#{`, `#$`.
//!
//! ## Autocorrect
//!
//! Range-edit replacing the surrounding quotes. The cop only emits an
//! autocorrect when the body content is unambiguously safe to swap:
//!
//! - **No backslashes** in the body (any `\` is an escape that means
//!   different things between `'…'` and `"…"`, e.g. `'\n'` = backslash-n
//!   vs `"\n"` = newline).
//! - **No `#`** in the body when converting *to* double quotes — `#{`
//!   in a double-quoted literal becomes interpolation rather than a
//!   literal `#`. The conservative rule is "any `#`" to keep the gate
//!   trivially correct.
//! - **No matching quote character** that would have to be re-escaped
//!   in the target style.
//!
//! When any of those fail the cop still emits the offense (the style
//! violation stands) but skips the edit so the user can hand-fix without
//! risk of a wrong autocorrect.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct StringLiterals;

/// Preferred quote style for plain string literals.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "single_quotes")]
    SingleQuotes,
    #[option(value = "double_quotes")]
    DoubleQuotes,
}

/// Cop options for [`StringLiterals`]. Values and key name match
/// RuboCop's `Style/StringLiterals` cop. Using [`CopOptionEnum`] for
/// `EnforcedStyle` makes the struct `Copy`-friendly, eliminating heap
/// allocations in the hot `check_str` path.
#[derive(CopOptions)]
pub struct StringLiteralsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "single_quotes",
        description = "Preferred quote style for plain string literals."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/StringLiterals",
    description = "Prefer one quote style (single / double) for plain string literals.",
    default_severity = "warning",
    default_enabled = true,
    options = StringLiteralsOptions
)]
impl StringLiterals {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Only standalone string literals (those with their own quote
        // delimiters) are subject to the quote-style check — RuboCop's
        // `StringHelp#on_str` guards on `node.loc?(:begin)`. A `Str` that is a
        // bare content segment of an interpolated string, a heredoc, or a
        // percent-literal (`%Q[…]`, `%(…)`, …) has no quote delimiter of its
        // own: its raw source can carry an *unescaped* `"`/`'` that is literal
        // content — HTML attribute quotes between two `#{…}` interpolations, or
        // a quote-like run such as `'foo'` in `"'foo'#{bar}"` — which
        // `parse_quote_form` would misread as a delimiter. Skip those.
        //
        // Adjacent string concatenation (`"a" "b"`, `%Q[a] "b"`, `"a" "#{b}"`)
        // also parses as a `Dstr`, but each child carries its OWN quote
        // delimiter and stays flagged, matching RuboCop (which sees a `begin`
        // loc on each). `dstr_segment_is_own_literal` distinguishes the two.
        if let Some(parent) = cx.parent(node).get() {
            match *cx.kind(parent) {
                // A `Dstr` segment that is NOT its own quoted literal (a bare
                // content segment) is skipped; an independently-quoted child
                // (concatenation) falls through to the quote-style check.
                NodeKind::Dstr(list)
                    if !dstr_segment_is_own_literal(node, parent, cx.list(list), cx) =>
                {
                    return;
                }
                NodeKind::Dsym(_) | NodeKind::Regexp { .. } | NodeKind::Xstr(_) => return,
                _ => {}
            }
        }

        let opts = cx.options_or_default::<StringLiteralsOptions>();
        let prefer_single = opts.enforced_style == EnforcedStyle::SingleQuotes;

        let range = cx.range(node);
        let src = cx.raw_source(range);
        let Some((actual, body)) = parse_quote_form(src) else {
            // %q / %Q / `?` char literal / similar — not a basic Str
            // literal even though the translator dropped it here.
            // Skip rather than guess.
            return;
        };

        let preferred = if prefer_single {
            QuoteStyle::Single
        } else {
            QuoteStyle::Double
        };
        if actual == preferred {
            return;
        }

        // In single_quotes mode: only flag a double-quoted string when double
        // quotes are NOT required. Mirrors RuboCop's double_quotes_required?
        // guard (rubocop/cop/util.rb:133).
        if prefer_single && actual == QuoteStyle::Double && double_quotes_required(src) {
            return;
        }

        // In double_quotes mode: only flag a single-quoted string when single
        // quotes are NOT required. Mirrors RuboCop's reverse guard.
        if !prefer_single && actual == QuoteStyle::Single && single_quotes_required(src) {
            return;
        }

        let (message, replacement) = match preferred {
            QuoteStyle::Single => (
                "Prefer single-quoted strings when you don't need string interpolation or special symbols.",
                safe_swap(body, b'\'', b'"').map(|s| format!("'{s}'")),
            ),
            QuoteStyle::Double => (
                "Prefer double-quoted strings unless you need single quotes to avoid extra backslashes for escaping.",
                safe_swap(body, b'"', b'\'').map(|s| format!("\"{s}\"")),
            ),
        };

        cx.emit_offense(range, message, None);
        if let Some(text) = replacement {
            cx.emit_edit(range, &text);
        }
        // Touch `Range` so the use stays load-bearing if a refactor drops it.
        let _ = std::mem::size_of::<Range>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteStyle {
    Single,
    Double,
}

/// Recognise `'…'` and `"…"` raw forms and split off the body. Returns
/// `None` for any other source shape (`%q[…]`, `?x`, heredoc head, …).
pub(crate) fn parse_quote_form(src: &str) -> Option<(QuoteStyle, &str)> {
    let bytes = src.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    match (first, last) {
        (b'\'', b'\'') => Some((QuoteStyle::Single, &src[1..src.len() - 1])),
        (b'"', b'"') => Some((QuoteStyle::Double, &src[1..src.len() - 1])),
        _ => None,
    }
}

/// Does this `Str` child of a `Dstr` carry its OWN opening quote delimiter
/// (RuboCop's `node.loc?(:begin)`), making it an independently-quoted literal
/// that the quote-style check applies to — as opposed to a bare content
/// segment of an interpolated / heredoc / percent-literal string?
///
/// Murphy's `NodeLoc` stores no `begin` delimiter range, so the predicate is
/// reconstructed positionally:
///
/// * The **first** child is an independent literal only when it begins exactly
///   where the `Dstr` begins — i.e. it owns the `Dstr`'s opening delimiter. A
///   content segment instead starts *after* the opening delimiter: `"a#{b}"`
///   → `a` starts one byte past the `"`; `%Q[a#{b}]` → past `%Q[`; a heredoc
///   segment sits on a later line entirely. This is what keeps a quote-like
///   leading segment such as `'foo'` in `"'foo'#{bar}"` from being mistaken
///   for an independent single-quoted literal.
/// * A **later** child is an independent literal when its previous sibling is
///   itself a string literal (`Str`/`Dstr`) — adjacent concatenation, e.g.
///   `%Q[foo] "bar"` or `"a" "b"` (even with no separating space). A bare
///   content segment instead follows an interpolation node (a `Begin` for
///   `#{…}`, or a variable node for `#@ivar` / `#$gvar`).
fn dstr_segment_is_own_literal(
    node: NodeId,
    parent: NodeId,
    children: &[NodeId],
    cx: &Cx<'_>,
) -> bool {
    let Some(idx) = children.iter().position(|&c| c == node) else {
        return false;
    };
    match idx {
        0 => cx.range(node).start == cx.range(parent).start,
        _ => matches!(
            *cx.kind(children[idx - 1]),
            NodeKind::Str(_) | NodeKind::Dstr(_)
        ),
    }
}

/// Conservative quote-swap predicate. Returns the body unchanged when
/// safe to re-wrap with the *target* quote; `None` otherwise. Safety
/// rules are intentionally tight — see the module doc comment for the
/// invariants we are protecting.
pub(crate) fn safe_swap(body: &str, target_quote: u8, source_quote: u8) -> Option<&str> {
    // Any backslash: escapes have different meanings between the two
    // quote styles. Don't try to be clever.
    if body.as_bytes().contains(&b'\\') {
        return None;
    }
    // The target quote would have to be re-escaped if it appears in the
    // body, but we just decided to disallow backslashes — so the only
    // way to keep the swap byte-for-byte is to rule out the target
    // quote character entirely.
    if body.as_bytes().contains(&target_quote) {
        return None;
    }
    // `#` in the body when going to double quotes means risking
    // interpolation (`#{`, `#@…`, `#$…`). Conservatively forbid any
    // `#` so the rule is one line.
    if target_quote == b'"' && body.as_bytes().contains(&b'#') {
        return None;
    }
    // The source quote (about to vanish) was already a literal in the
    // body — if it appears the resulting target form would now contain
    // a bare quote. Rule it out: `'foo"bar'` → not a clean swap to
    // `"foo"bar"`.
    if body.as_bytes().contains(&source_quote) {
        return None;
    }
    Some(body)
}

/// Returns `true` when double quotes are semantically required — i.e. the
/// source string (including surrounding quotes) either contains a
/// single-quote or a meaningful backslash escape (an odd-length run of
/// backslashes followed by a character that is neither `\` nor `"`).
///
/// Mirrors RuboCop's `double_quotes_required?` from rubocop/cop/util.rb:133:
/// `/'|(?<!\\)(?:\\{2})*\\(?![\\"])/x`
pub(crate) fn double_quotes_required(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' {
            return true;
        }
        if b[i] == b'\\' {
            // Count the run of consecutive backslashes.
            let start = i;
            while i < b.len() && b[i] == b'\\' {
                i += 1;
            }
            let run = i - start;
            // An odd-length run means the last backslash is an escape.
            // If what follows is not `\` or `"` it's a meaningful escape
            // (e.g. `\n`, `\t`, `\u`, `\x`, `\'`, ...).
            if run % 2 == 1 {
                let next = if i < b.len() { b[i] } else { 0 };
                if next != b'"' {
                    return true;
                }
            }
            continue;
        }
        i += 1;
    }
    false
}

/// Returns `true` when single quotes are semantically required — i.e. the
/// source string contains `"`, a backslash escape that is not `\'` or
/// `\\`, or an interpolation anchor (`#@`, `#{`, `#$`).
///
/// Mirrors RuboCop's reverse guard: `/" | \\[^'\\] | \#[@{$]/x`
pub(crate) fn single_quotes_required(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => return true,
            b'\\' => {
                let next = if i + 1 < b.len() { b[i + 1] } else { 0 };
                // A backslash followed by anything other than `'` or `\`
                // is a non-trivial escape.
                if next != b'\'' && next != b'\\' {
                    return true;
                }
                i += 2;
                continue;
            }
            b'#' => {
                let next = if i + 1 < b.len() { b[i + 1] } else { 0 };
                if matches!(next, b'@' | b'{' | b'$') {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- double_quotes_required tests ---

    #[test]
    fn dqr_simple_double_quoted_plain_string() {
        // `"hello"` -- no single quote, no meaningful escape -> false (offense fires)
        assert!(!double_quotes_required("\"hello\""));
    }

    #[test]
    fn dqr_contains_single_quote() {
        // `"'"` -- source contains a literal single quote -> true (no offense)
        assert!(double_quotes_required("\"'\""));
    }

    #[test]
    fn dqr_newline_escape() {
        // `"\n"` -- odd backslash run followed by `n` -> true (no offense)
        assert!(double_quotes_required("\"\\n\""));
    }

    #[test]
    fn dqr_escape_esc() {
        // `"\e"` -- odd backslash run followed by `e` -> true (no offense)
        assert!(double_quotes_required("\"\\e\""));
    }

    #[test]
    fn dqr_unicode_escape() {
        // `"ñ"` -- odd backslash run followed by `u` -> true (no offense)
        assert!(double_quotes_required("\"\\u00f1\""));
    }

    #[test]
    fn dqr_hex_escape() {
        // `"\xf9"` -- odd backslash run followed by `x` -> true (no offense)
        assert!(double_quotes_required("\"\\xf9\""));
    }

    #[test]
    fn dqr_even_backslash_run_not_an_escape() {
        // `"\\"` -- even run, the two backslashes form a literal `\` -> false
        // (single quotes could represent this as `'\\'`, so offense should fire)
        assert!(!double_quotes_required("\"\\\\\""));
    }

    #[test]
    fn dqr_escaped_double_quote() {
        // `"\""` -- odd run followed by `"`, exempted by the `!= '"'` guard -> false
        // (offense fires, autocorrect blocked by safe_swap)
        assert!(!double_quotes_required("\"\\\"\""));
    }

    #[test]
    fn dqr_backslash_continuation() {
        // Three backslashes followed by `n`: odd run + n -> true
        assert!(double_quotes_required("\"foo\\\\\\nbar\""));
    }

    // --- single_quotes_required tests ---

    #[test]
    fn sqr_plain_single_quoted() {
        // `'hello'` -- nothing that requires single quotes -> false (offense fires)
        assert!(!single_quotes_required("'hello'"));
    }

    #[test]
    fn sqr_contains_double_quote() {
        // `'say "hi"'` -- contains `"` -> true (single quotes required, no offense)
        assert!(single_quotes_required("'say \"hi\"'"));
    }

    #[test]
    fn sqr_interpolation_anchor_hash_brace() {
        // `'#{foo}'` -- contains `#{` -> true
        assert!(single_quotes_required("'#{foo}'"));
    }

    #[test]
    fn sqr_escaped_backslash_not_required() {
        // `'\\'` -- `\\` escape (next byte is `\`) -> false
        assert!(!single_quotes_required("'\\\\'"));
    }

    #[test]
    fn sqr_escaped_single_quote_not_required() {
        // `'\''` -- `\'` escape (next byte is `'`) -> false
        assert!(!single_quotes_required("'\\''"));
    }

    // --- RuboCop parity: EnforcedStyle config key and enum values ---

    #[test]
    fn enforced_style_double_quotes_from_config_json() {
        // Config JSON uses RuboCop's `EnforcedStyle` key and `double_quotes` value.
        use murphy_plugin_api::CopOptions;
        let opts =
            StringLiteralsOptions::from_config_json(br#"{"EnforcedStyle": "double_quotes"}"#)
                .expect("valid config");
        assert_eq!(opts.enforced_style, EnforcedStyle::DoubleQuotes);
    }

    #[test]
    fn enforced_style_single_quotes_from_config_json() {
        // Config JSON uses RuboCop's `EnforcedStyle` key and `single_quotes` value.
        use murphy_plugin_api::CopOptions;
        let opts =
            StringLiteralsOptions::from_config_json(br#"{"EnforcedStyle": "single_quotes"}"#)
                .expect("valid config");
        assert_eq!(opts.enforced_style, EnforcedStyle::SingleQuotes);
    }

    #[test]
    fn enforced_style_default_is_single_quotes() {
        // Default must be `single_quotes` to match RuboCop.
        let opts = StringLiteralsOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::SingleQuotes);
    }

    #[test]
    fn double_quotes_mode_flags_single_quoted_string() {
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>()
            .with_options(&StringLiteralsOptions {
                enforced_style: EnforcedStyle::DoubleQuotes,
            })
            .expect_offense(indoc! {r#"
                x = 'hello'
                    ^^^^^^^ Prefer double-quoted strings unless you need single quotes to avoid extra backslashes for escaping.
            "#});
    }

    #[test]
    fn dstr_not_flagged_in_single_quotes_mode() {
        // Interpolated strings (`dstr`) must never be flagged in single mode —
        // they cannot be expressed with single quotes.
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>().expect_no_offenses("x = \"hello #{name}\"\n");
    }

    #[test]
    fn dstr_not_flagged_in_double_quotes_mode() {
        // Interpolated strings must also not be flagged in double mode.
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>()
            .with_options(&StringLiteralsOptions {
                enforced_style: EnforcedStyle::DoubleQuotes,
            })
            .expect_no_offenses("x = \"hello #{name}\"\n");
    }

    #[test]
    fn heredoc_interpolation_segments_not_flagged() {
        // Literal `"` between `#{…}` interpolations in a heredoc body is HTML
        // content, not a string-literal delimiter; the `str` segment must not be
        // treated as a double-quoted literal.
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>().expect_no_offenses(indoc! {r##"
            def html
              <<~HTML
                <a href="#{url}" title="#{name}">link</a>
              HTML
            end
        "##});
    }

    #[test]
    fn interpolation_string_segments_not_flagged() {
        // Segments of an ordinary interpolated string have no quote delimiter
        // of their own and must never be flagged.
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>().expect_no_offenses("x = \"a#{b}c\"\n");
    }

    #[test]
    fn adjacent_string_concatenation_flagged() {
        // `"foo" "bar"` parses as a `dstr` of two independently-quoted `str`
        // children; each is a real double-quoted literal and is flagged
        // (matching RuboCop, which sees a `begin` loc on each).
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>().expect_correction(
            indoc! {r#"
                x = "foo" "bar"
                    ^^^^^ Prefer single-quoted strings when you don't need string interpolation or special symbols.
                          ^^^^^ Prefer single-quoted strings when you don't need string interpolation or special symbols.
            "#},
            "x = 'foo' 'bar'\n",
        );
    }

    #[test]
    fn dsym_interpolation_segment_not_flagged() {
        // `:"a#{b}"` — the `str` segment inside an interpolated dynamic symbol
        // is not a string literal.
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>().expect_no_offenses("x = :\"sym#{b}\"\n");
    }

    #[test]
    fn mixed_adjacent_concatenation_flags_quoted_literal() {
        // `"foo" "#{bar}"` — the leading `"foo"` is an independently-quoted
        // literal (flagged), even though its sibling is an interpolation.
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>().expect_offense(indoc! {r##"
            x = "foo" "#{bar}"
                ^^^^^ Prefer single-quoted strings when you don't need string interpolation or special symbols.
        "##});
    }

    #[test]
    fn percent_q_interpolation_segment_with_quote_not_flagged() {
        // `%Q[…"x"…#{y}]` — the literal `"` inside a percent-literal body is
        // content, not a delimiter; the segment must not be flagged.
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>().expect_no_offenses("x = %Q[a \"b\" #{c}]\n");
    }

    #[test]
    fn concatenation_with_interpolated_part_flags_leading_literal() {
        // `"foo" "bar #{baz}"` — the leading `"foo"` is an independently-quoted
        // literal and is flagged/corrected; the interpolated sibling's content
        // segment (`bar `) carries no delimiter of its own and is left intact.
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>().expect_correction(
            indoc! {r#"
                x = "foo" "bar #{baz}"
                    ^^^^^ Prefer single-quoted strings when you don't need string interpolation or special symbols.
            "#},
            "x = 'foo' \"bar #{baz}\"\n",
        );
    }

    #[test]
    fn mixed_percent_and_quoted_literal_flags_quoted_literal() {
        // `%Q[foo] "bar"` — the whole expression is a `dstr`, but the second
        // child `"bar"` carries its own double quotes (a previous-sibling
        // literal makes it adjacent concatenation), so it is flagged even
        // though the percent-literal first piece is not.
        use murphy_plugin_api::test_support::{indoc, test};
        test::<StringLiterals>().expect_correction(
            indoc! {r#"
                x = %Q[foo] "bar"
                            ^^^^^ Prefer single-quoted strings when you don't need string interpolation or special symbols.
            "#},
            "x = %Q[foo] 'bar'\n",
        );
    }

    #[test]
    fn quote_like_segment_in_interpolation_not_corrupted_double_mode() {
        // `"'foo'#{bar}"` — the leading content segment `'foo'` looks like a
        // single-quoted literal but has no delimiter of its own. In
        // double_quotes mode it must NOT be treated as an independent literal
        // (which would autocorrect inside the interpolation and corrupt it).
        use murphy_plugin_api::test_support::test;
        test::<StringLiterals>()
            .with_options(&StringLiteralsOptions {
                enforced_style: EnforcedStyle::DoubleQuotes,
            })
            .expect_no_offenses("x = \"'foo'#{bar}\"\n");
    }
}
murphy_plugin_api::submit_cop!(StringLiterals);
