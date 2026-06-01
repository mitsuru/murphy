//! `Style/TrailingCommaInHashLiteral` — checks for trailing comma in hash
//! literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingCommaInHashLiteral
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyleForMultiline: `no_comma` (default), `comma`, and
//!   `consistent_comma` are implemented. `diff_comma` is not implemented.
//!   Braceless hashes (bare hash method arguments) are skipped — only
//!   braced hash literals are checked (consistent with RuboCop).
//!   `comma` style: requires a trailing comma only when every pair AND the
//!   closing `}` are each on their own lines.
//!   `consistent_comma` style: requires a trailing comma whenever the hash
//!   is multiline (unless the only pair and the closing `}` are on the same
//!   line — the allowed-multiline-argument exception).
//!   Autocorrect for `no_comma`: removes the trailing comma.
//!   Autocorrect for `comma`/`consistent_comma`: inserts a comma after the
//!   last pair.
//! ```
//!
//! ## Offense conditions
//!
//! `no_comma` (default): A trailing comma exists after the last hash element.
//!
//! `comma`: The hash is multiline AND every element is on its own line AND
//! the closing `}` is on its own line, but no trailing comma is present.
//!
//! `consistent_comma`: The hash is multiline (and not the allowed single-pair
//! exception), but no trailing comma is present.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TrailingCommaInHashLiteral;

/// Cop options for [`TrailingCommaInHashLiteral`].
#[derive(CopOptions)]
pub struct TrailingCommaInHashLiteralOptions {
    #[option(
        name = "EnforcedStyleForMultiline",
        default = "no_comma",
        description = "Trailing comma style for multiline hash literals."
    )]
    pub enforced_style_for_multiline: TrailingCommaStyle,
}

/// The enforced style for trailing commas in multiline hash literals.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailingCommaStyle {
    /// Never allow a trailing comma. (Default)
    #[option(value = "no_comma")]
    NoComma,
    /// Require a trailing comma when each item and the closing bracket are on
    /// their own line.
    #[option(value = "comma")]
    Comma,
    /// Require a trailing comma whenever the hash is multiline (ignoring the
    /// single-pair, same-closing-brace exception).
    #[option(value = "consistent_comma")]
    ConsistentComma,
}

#[cop(
    name = "Style/TrailingCommaInHashLiteral",
    description = "Checks for trailing comma in hash literals.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingCommaInHashLiteralOptions,
)]
impl TrailingCommaInHashLiteral {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Hash(list) = *cx.kind(node) else {
            return;
        };
        let pairs = cx.list(list);

        // Empty hash: nothing to check.
        if pairs.is_empty() {
            return;
        }

        // Braceless hash (bare method argument) — skip.
        // A braced hash starts with a `{` token at the hash node's own start
        // position. Braceless hashes start at the first pair, not at `{`.
        if !is_braced_hash(node, cx) {
            return;
        }

        let opts = cx.options_or_default::<TrailingCommaInHashLiteralOptions>();
        let last_pair = *pairs.last().unwrap();

        // Determine if there is a trailing comma (a Comma token between the
        // last pair end and the closing `}`).
        let trailing_comma = find_trailing_comma(node, last_pair, cx);

        match opts.enforced_style_for_multiline {
            TrailingCommaStyle::NoComma => {
                if let Some(comma_range) = trailing_comma {
                    cx.emit_offense(
                        comma_range,
                        "Avoid trailing comma after the last item of a hash.",
                        None,
                    );
                    // Autocorrect: delete the comma.
                    cx.emit_edit(comma_range, "");
                }
            }
            TrailingCommaStyle::Comma => {
                if trailing_comma.is_some() {
                    // Already has a trailing comma — only flag if it shouldn't
                    // have one (e.g. single-line hash), but `no_comma` handles
                    // that. For `comma`, an existing comma is fine.
                    return;
                }
                // Flag missing trailing comma only when every pair and the
                // closing brace are each on their own lines.
                if should_have_comma_comma_style(node, pairs, last_pair, cx) {
                    let after_last_pair = cx.range(last_pair).end;
                    cx.emit_offense(
                        cx.range(last_pair),
                        "Put a comma after the last item of a multiline hash.",
                        None,
                    );
                    // Autocorrect: insert comma after the last pair.
                    cx.emit_edit(
                        Range {
                            start: after_last_pair,
                            end: after_last_pair,
                        },
                        ",",
                    );
                }
            }
            TrailingCommaStyle::ConsistentComma => {
                if trailing_comma.is_some() {
                    // Already has a trailing comma — fine for consistent_comma.
                    return;
                }
                // Flag missing trailing comma when multiline (with the single-
                // element-same-closing-brace exception).
                if should_have_comma_consistent_style(node, last_pair, pairs.len(), cx) {
                    let after_last_pair = cx.range(last_pair).end;
                    cx.emit_offense(
                        cx.range(last_pair),
                        "Put a comma after the last item of a multiline hash.",
                        None,
                    );
                    cx.emit_edit(
                        Range {
                            start: after_last_pair,
                            end: after_last_pair,
                        },
                        ",",
                    );
                }
            }
        }
    }
}

/// Returns true if the hash node is a braced hash literal (starts with `{`).
///
/// A braceless hash (bare method argument) has its range starting at the first
/// pair, not at `{`. A braced hash literal has a `LeftBrace` token exactly at
/// the hash node's start offset.
fn is_braced_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    let hash_start = cx.range(node).start;
    cx.token_after(hash_start)
        .map(|tok| tok.range.start == hash_start && tok.kind == SourceTokenKind::LeftBrace)
        .unwrap_or(false)
}

/// Returns the range of the trailing comma token if one exists between the
/// last pair's end and the closing `}`.
fn find_trailing_comma(node: NodeId, last_pair: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let last_pair_end = cx.range(last_pair).end;
    let hash_end = cx.range(node).end;

    // Look for a Comma token between last_pair_end and hash_end.
    // The comma must come before any non-whitespace/comment content.
    let search_range = Range {
        start: last_pair_end,
        end: hash_end,
    };
    let toks = cx.tokens_in(search_range);
    for tok in toks {
        match tok.kind {
            SourceTokenKind::Comma => return Some(tok.range),
            SourceTokenKind::Comment
            | SourceTokenKind::Newline
            | SourceTokenKind::IgnoredNewline => {
                continue;
            }
            SourceTokenKind::RightBrace => break,
            _ => break,
        }
    }
    None
}

/// Returns true when `comma` style should require a trailing comma:
/// every pair is on its own line AND the closing `}` is on its own line.
fn should_have_comma_comma_style(
    node: NodeId,
    pairs: &[NodeId],
    last_pair: NodeId,
    cx: &Cx<'_>,
) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }

    let source = cx.source().as_bytes();

    // Find closing `}` position.
    let closing_brace_start = find_closing_brace_start(node, cx);

    // The closing `}` must be on its own line.
    if !is_at_line_start(closing_brace_start, source) {
        return false;
    }

    // Every pair must be on its own line.
    for &pair in pairs {
        if !is_at_line_start(cx.range(pair).start, source) {
            return false;
        }
    }

    // The last pair must end before the closing brace line.
    let last_pair_range = cx.range(last_pair);
    let between = Range {
        start: last_pair_range.end,
        end: closing_brace_start,
    };
    if !cx.raw_source(between).contains('\n') {
        // Last pair and `}` are on the same line.
        return false;
    }

    true
}

/// Returns true when `consistent_comma` style should require a trailing comma:
/// multiline hash, excluding the single-pair same-closing-brace exception.
///
/// The exception applies only when there is exactly one pair AND the closing
/// `}` is on the same line as the last pair end.
fn should_have_comma_consistent_style(
    node: NodeId,
    last_pair: NodeId,
    pair_count: usize,
    cx: &Cx<'_>,
) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }

    // Single-pair exception: when there is exactly one pair AND the closing
    // `}` is on the same line as the last pair end, skip.
    if pair_count == 1 {
        let closing_brace_start = find_closing_brace_start(node, cx);
        let last_pair_end = cx.range(last_pair).end;
        let between = Range {
            start: last_pair_end,
            end: closing_brace_start,
        };
        let between_src = cx.raw_source(between);
        // If there's no newline between the last pair and `}`, it's the
        // "allowed multiline argument" exception.
        if !between_src.contains('\n') {
            return false;
        }
    }

    true
}

/// Returns the byte offset of the closing `}` of the hash.
fn find_closing_brace_start(node: NodeId, cx: &Cx<'_>) -> u32 {
    let hash_range = cx.range(node);
    // The closing `}` is the last RightBrace token in the hash range.
    let toks = cx.tokens_in(hash_range);
    for tok in toks.iter().rev() {
        if tok.kind == SourceTokenKind::RightBrace {
            return tok.range.start;
        }
    }
    // Fallback: use hash end - 1 (should not normally happen for braced hashes).
    hash_range.end.saturating_sub(1)
}

/// Returns true if position `pos` is at the start of a line (only whitespace
/// between the last newline and `pos`).
fn is_at_line_start(pos: u32, source: &[u8]) -> bool {
    let pos = pos as usize;
    let line_start = source[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    source[line_start..pos]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

#[cfg(test)]
mod tests {
    use super::{
        TrailingCommaInHashLiteral, TrailingCommaInHashLiteralOptions, TrailingCommaStyle,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    // --- no_comma (default): flag trailing comma ---

    #[test]
    fn no_comma_flags_trailing_comma_single_line() {
        test::<TrailingCommaInHashLiteral>().expect_offense(indoc! {r#"
            a = { foo: 1, bar: 2, }
                                ^ Avoid trailing comma after the last item of a hash.
        "#});
    }

    #[test]
    fn no_comma_flags_trailing_comma_multiline() {
        test::<TrailingCommaInHashLiteral>().expect_offense(indoc! {r#"
            a = {
              foo: 1,
              bar: 2,
                    ^ Avoid trailing comma after the last item of a hash.
            }
        "#});
    }

    #[test]
    fn no_comma_accepts_no_trailing_comma() {
        test::<TrailingCommaInHashLiteral>().expect_no_offenses(indoc! {r#"
            a = {
              foo: 1,
              bar: 2
            }
        "#});
    }

    #[test]
    fn no_comma_accepts_empty_hash() {
        test::<TrailingCommaInHashLiteral>().expect_no_offenses("a = {}\n");
    }

    #[test]
    fn no_comma_accepts_single_line_no_trailing_comma() {
        test::<TrailingCommaInHashLiteral>().expect_no_offenses("a = { foo: 1, bar: 2 }\n");
    }

    // --- no_comma autocorrect ---

    #[test]
    fn no_comma_corrects_trailing_comma_single_line() {
        test::<TrailingCommaInHashLiteral>().expect_correction(
            indoc! {r#"
                a = { foo: 1, bar: 2, }
                                    ^ Avoid trailing comma after the last item of a hash.
            "#},
            "a = { foo: 1, bar: 2 }\n",
        );
    }

    #[test]
    fn no_comma_corrects_trailing_comma_multiline() {
        test::<TrailingCommaInHashLiteral>().expect_correction(
            indoc! {r#"
                a = {
                  foo: 1,
                  bar: 2,
                        ^ Avoid trailing comma after the last item of a hash.
                }
            "#},
            indoc! {r#"
                a = {
                  foo: 1,
                  bar: 2
                }
            "#},
        );
    }

    // --- comma style: require trailing comma when each item on own line ---

    #[test]
    fn comma_style_accepts_no_trailing_comma_single_line() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses("a = { foo: 1, bar: 2 }\n");
    }

    #[test]
    fn comma_style_flags_missing_comma_when_each_item_on_own_line() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_offense(indoc! {r#"
                a = {
                  foo: 1,
                  bar: 2
                  ^^^^^^ Put a comma after the last item of a multiline hash.
                }
            "#});
    }

    #[test]
    fn comma_style_accepts_trailing_comma_when_each_item_on_own_line() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {r#"
                a = {
                  foo: 1,
                  bar: 2,
                }
            "#});
    }

    #[test]
    fn comma_style_accepts_when_items_not_each_on_own_line() {
        // When items share a line, no trailing comma required for `comma` style.
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {r#"
                a = {
                  foo: 1, bar: 2
                }
            "#});
    }

    // --- comma style autocorrect ---

    #[test]
    fn comma_style_corrects_adds_trailing_comma() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_correction(
                indoc! {r#"
                    a = {
                      foo: 1,
                      bar: 2
                      ^^^^^^ Put a comma after the last item of a multiline hash.
                    }
                "#},
                indoc! {r#"
                    a = {
                      foo: 1,
                      bar: 2,
                    }
                "#},
            );
    }

    // --- consistent_comma style ---

    #[test]
    fn consistent_comma_flags_missing_trailing_comma_multiline() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
            })
            .expect_offense(indoc! {r#"
                a = {
                  foo: 1, bar: 2,
                  qux: 3
                  ^^^^^^ Put a comma after the last item of a multiline hash.
                }
            "#});
    }

    #[test]
    fn consistent_comma_accepts_trailing_comma() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
            })
            .expect_no_offenses(indoc! {r#"
                a = {
                  foo: 1, bar: 2,
                  qux: 3,
                }
            "#});
    }

    #[test]
    fn consistent_comma_accepts_single_line() {
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
            })
            .expect_no_offenses("a = { foo: 1, bar: 2 }\n");
    }

    #[test]
    fn consistent_comma_flags_multi_pair_when_last_pair_on_same_line_as_brace() {
        // Multi-pair hash where last pair and `}` share a line — the single-pair
        // exception must NOT apply. The hash still needs a trailing comma.
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
            })
            .expect_offense(indoc! {r#"
                a = {
                  foo: 1,
                  bar: 2 }
                  ^^^^^^ Put a comma after the last item of a multiline hash.
            "#});
    }

    #[test]
    fn consistent_comma_accepts_single_pair_when_closing_brace_same_line() {
        // Single-pair exception: exactly one pair AND closing `}` on same line.
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
            })
            .expect_no_offenses(indoc! {r#"
                a = {
                  foo: 1 }
            "#});
    }

    #[test]
    fn no_comma_skips_braceless_hash_argument() {
        // Braceless hash argument: the outer Hash has no `{` at its start.
        test::<TrailingCommaInHashLiteral>().expect_no_offenses(
            "foo(a: 1, b: 2,)
",
        );
    }

    #[test]
    fn no_comma_flags_only_braced_hash_not_braceless_containing_braced() {
        // Braceless hash containing a braced hash value — the outer hash must
        // not be flagged; only the inner braced hash with trailing comma is.
        test::<TrailingCommaInHashLiteral>().expect_offense(indoc! {r#"
                foo(a: { b: 1, },)
                             ^ Avoid trailing comma after the last item of a hash.
            "#});
    }

    #[test]
    fn comma_style_accepts_when_first_pair_not_on_own_line() {
        // `comma` style: first pair is on the same line as `{` → no trailing
        // comma required, even if last pair and `}` are on separate lines.
        test::<TrailingCommaInHashLiteral>()
            .with_options(&TrailingCommaInHashLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {r#"
                a = { foo: 1,
                  bar: 2
                }
            "#});
    }

    // --- Config JSON tests ---

    #[test]
    fn default_style_is_no_comma() {
        let opts = TrailingCommaInHashLiteralOptions::default();
        assert_eq!(
            opts.enforced_style_for_multiline,
            TrailingCommaStyle::NoComma
        );
    }
}
murphy_plugin_api::submit_cop!(TrailingCommaInHashLiteral);
