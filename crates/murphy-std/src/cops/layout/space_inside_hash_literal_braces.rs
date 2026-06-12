//! `Layout/SpaceInsideHashLiteralBraces` — checks the spacing immediately
//! inside hash literal braces `{ }`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideHashLiteralBraces
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Node-driven (on_hash) like RuboCop, NOT a raw token scan: hash `{` and
//!   block `{` share `SourceTokenKind::LeftBrace`, so visiting Hash nodes is
//!   what keeps this cop off brace blocks. Mirrors RuboCop's `check`/
//!   `expect_space?`/`offense?` for the leading and trailing brace pairs, and
//!   `check_whitespace_only_hash` for whitespace-only empty braces under the
//!   `no_space` empty-braces style. Supports EnforcedStyle space(default)/
//!   no_space/compact and EnforcedStyleForEmptyBraces no_space(default)/space.
//!   Hashes passed as bare kwargs (`foo(a: 1)`) have no brace tokens and are
//!   skipped via the first/last brace guard.
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceToken, SourceTokenKind, cop,
};

#[derive(Default)]
pub struct SpaceInsideHashLiteralBraces;

#[derive(CopOptions)]
pub struct SpaceInsideHashLiteralBracesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "space",
        description = "Hash literal brace spacing style."
    )]
    pub enforced_style: HashBraceStyle,
    #[option(
        name = "EnforcedStyleForEmptyBraces",
        default = "no_space",
        description = "Spacing style for empty hash literal braces."
    )]
    pub empty_style: EmptyHashBraceStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum HashBraceStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "compact")]
    Compact,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum EmptyHashBraceStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceInsideHashLiteralBraces",
    description = "Check spacing inside hash literal braces.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideHashLiteralBracesOptions,
)]
impl SpaceInsideHashLiteralBraces {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>, options: &SpaceInsideHashLiteralBracesOptions) {
        let tokens = cx.tokens_in(cx.range(node));
        // Only brace-delimited hash literals; bare kwargs have no `{`/`}`.
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return;
        };
        if first.kind != SourceTokenKind::LeftBrace || last.kind != SourceTokenKind::RightBrace {
            return;
        }

        // Empty hash (`{}` / `{  }` / `{\n}`): handle in one place, matching
        // RuboCop's `check`(empty-pair) + `check_whitespace_only_hash`. The
        // interior between the braces is whitespace-only. This is disjoint from
        // the contents path below, so there is no double-fire.
        let interior = Range {
            start: first.range.end,
            end: last.range.start,
        };
        if cx.raw_source(interior).bytes().all(|b| b.is_ascii_whitespace()) {
            check_empty_braces(cx, options, *first, *last, interior);
            return;
        }

        // Leading brace pair: `{` and the token after it.
        if tokens.len() >= 2 {
            check(cx, options, tokens[0], tokens[1]);
        }
        // Trailing brace pair: the token before `}` and `}`.
        if tokens.len() > 2 {
            check(cx, options, tokens[tokens.len() - 2], tokens[tokens.len() - 1]);
        }
    }
}

/// Empty-hash handling: `{}` adjacent (offense only under `space` empty style),
/// or whitespace-only `{  }` / `{\n}` (offense only under `no_space` empty
/// style). Mirrors RuboCop's empty-pair `check` plus `check_whitespace_only_hash`.
fn check_empty_braces(
    cx: &Cx<'_>,
    options: &SpaceInsideHashLiteralBracesOptions,
    first: SourceToken,
    last: SourceToken,
    interior: Range,
) {
    if interior.start == interior.end {
        // `{}` — adjacent braces.
        if options.empty_style == EmptyHashBraceStyle::Space {
            let range = Range {
                start: first.range.start,
                end: last.range.end,
            };
            cx.emit_offense(range, "Space inside empty hash literal braces missing.", None);
            cx.emit_edit(
                Range {
                    start: first.range.end,
                    end: first.range.end,
                },
                " ",
            );
        }
    } else if options.empty_style == EmptyHashBraceStyle::NoSpace {
        // `{  }` / `{\n}` — whitespace-only interior.
        cx.emit_offense(
            interior,
            "Space inside empty hash literal braces detected.",
            None,
        );
        cx.emit_edit(interior, "");
    }
}

fn check(
    cx: &Cx<'_>,
    options: &SpaceInsideHashLiteralBracesOptions,
    token1: SourceToken,
    token2: SourceToken,
) {
    // Skip across line breaks — multiline hashes are exempt.
    if same_line(cx, token1, token2).is_none() {
        return;
    }
    if token2.kind == SourceTokenKind::Comment {
        return;
    }

    let expect = expect_space(options, token1, token2);
    if offense(cx, token1, expect) {
        emit(cx, token1, token2, expect);
    }
}

/// Whether a space is expected between `token1` and `token2`.
fn expect_space(
    options: &SpaceInsideHashLiteralBracesOptions,
    token1: SourceToken,
    token2: SourceToken,
) -> bool {
    let is_same_braces = token1.kind == token2.kind;
    let is_empty_braces =
        token1.kind == SourceTokenKind::LeftBrace && token2.kind == SourceTokenKind::RightBrace;

    if is_same_braces && options.enforced_style == HashBraceStyle::Compact {
        false
    } else if is_empty_braces {
        options.empty_style != EmptyHashBraceStyle::NoSpace
    } else {
        options.enforced_style != HashBraceStyle::NoSpace
    }
}

/// True when the actual spacing differs from what is expected. `token1` is the
/// brace whose trailing side faces the interior (`{` for the leading pair, or
/// the last content token before `}` for the trailing pair).
fn offense(cx: &Cx<'_>, token1: SourceToken, expect_space: bool) -> bool {
    let has_space = cx
        .source()
        .as_bytes()
        .get(token1.range.end as usize)
        .is_some_and(|&b| b == b' ' || b == b'\t');
    if expect_space {
        !has_space
    } else {
        has_space
    }
}

/// Emit a non-empty-hash brace offense. The empty-hash cases are handled
/// upstream in `check_empty_braces`, so `token1`/`token2` are never both braces.
fn emit(cx: &Cx<'_>, token1: SourceToken, token2: SourceToken, expect_space: bool) {
    // The brace involved is the left brace if token1 is `{`, else token2 is `}`.
    let brace = if token1.kind == SourceTokenKind::LeftBrace {
        token1
    } else {
        token2
    };

    if expect_space {
        // Missing space: insert one right at the brace edge.
        let at = if brace.kind == SourceTokenKind::LeftBrace {
            brace.range.end
        } else {
            brace.range.start
        };
        let range = Range { start: at, end: at };
        cx.emit_offense(range, &message(cx, brace, expect_space), None);
        cx.emit_edit(range, " ");
    } else {
        // Detected (unwanted) space: remove the surrounding whitespace.
        let range = space_range(cx, brace);
        if range.start >= range.end {
            return;
        }
        cx.emit_offense(range, &message(cx, brace, expect_space), None);
        cx.emit_edit(range, "");
    }
}

fn message(cx: &Cx<'_>, brace: SourceToken, expect_space: bool) -> String {
    let inside_what = cx.raw_source(brace.range);
    let problem = if expect_space { "missing" } else { "detected" };
    format!("Space inside {inside_what} {problem}.")
}

/// Range of the whitespace adjacent to `brace`: to the right of `{`, to the
/// left of `}`.
fn space_range(cx: &Cx<'_>, brace: SourceToken) -> Range {
    let src = cx.source().as_bytes();
    if brace.kind == SourceTokenKind::LeftBrace {
        let mut end = brace.range.end as usize;
        while src.get(end).is_some_and(|&b| b == b' ' || b == b'\t') {
            end += 1;
        }
        Range {
            start: brace.range.end,
            end: end as u32,
        }
    } else {
        let mut start = brace.range.start as usize;
        while start > 0 && src.get(start - 1).is_some_and(|&b| b == b' ' || b == b'\t') {
            start -= 1;
        }
        Range {
            start: start as u32,
            end: brace.range.start,
        }
    }
}

/// `Some(())` when the two tokens are on the same source line, else `None`.
fn same_line(cx: &Cx<'_>, token1: SourceToken, token2: SourceToken) -> Option<()> {
    let between = cx.raw_source(Range {
        start: token1.range.start,
        end: token2.range.end,
    });
    if between.bytes().any(|b| b == b'\n') {
        None
    } else {
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EmptyHashBraceStyle, HashBraceStyle, SpaceInsideHashLiteralBraces,
        SpaceInsideHashLiteralBracesOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    // ── default (space) style ───────────────────────────────────────────────

    #[test]
    fn space_style_flags_missing_leading_space() {
        let result = murphy_plugin_api::test_support::run_cop_with_edits::<
            SpaceInsideHashLiteralBraces,
        >("h = {a: 1 }\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space inside { missing.");
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn space_style_flags_missing_trailing_space() {
        let result = murphy_plugin_api::test_support::run_cop_with_edits::<
            SpaceInsideHashLiteralBraces,
        >("h = { a: 1}\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space inside } missing.");
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn space_style_accepts_spaced_hash() {
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses("h = { a: 1 }\n");
    }

    #[test]
    fn space_style_accepts_empty_braces_no_space() {
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses("h = {}\n");
    }

    #[test]
    fn space_style_flags_whitespace_only_empty_braces() {
        test::<SpaceInsideHashLiteralBraces>().expect_correction(
            indoc! {r#"
                h = {  }
                     ^^ Space inside empty hash literal braces detected.
            "#},
            "h = {}\n",
        );
    }

    #[test]
    fn default_flags_multiline_whitespace_only_empty_braces() {
        // `{\n}` is whitespace-only too — Murphy treats the newline as a token,
        // so this must still collapse to `{}` (RuboCop's check_whitespace_only_hash).
        let result = murphy_plugin_api::test_support::run_cop_with_edits::<
            SpaceInsideHashLiteralBraces,
        >("h = {\n}\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(
            result.offenses[0].message,
            "Space inside empty hash literal braces detected."
        );
    }

    // ── no_space style ──────────────────────────────────────────────────────

    #[test]
    fn no_space_style_flags_leading_space() {
        let opts = SpaceInsideHashLiteralBracesOptions {
            enforced_style: HashBraceStyle::NoSpace,
            empty_style: EmptyHashBraceStyle::NoSpace,
        };
        test::<SpaceInsideHashLiteralBraces>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    h = { a: 1}
                         ^ Space inside { detected.
                "#},
                "h = {a: 1}\n",
            );
    }

    #[test]
    fn no_space_style_flags_trailing_space() {
        let opts = SpaceInsideHashLiteralBracesOptions {
            enforced_style: HashBraceStyle::NoSpace,
            empty_style: EmptyHashBraceStyle::NoSpace,
        };
        test::<SpaceInsideHashLiteralBraces>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    h = {a: 1 }
                             ^ Space inside } detected.
                "#},
                "h = {a: 1}\n",
            );
    }

    #[test]
    fn no_space_style_accepts_tight_hash() {
        let opts = SpaceInsideHashLiteralBracesOptions {
            enforced_style: HashBraceStyle::NoSpace,
            empty_style: EmptyHashBraceStyle::NoSpace,
        };
        test::<SpaceInsideHashLiteralBraces>()
            .with_options(&opts)
            .expect_no_offenses("h = {a: 1}\n");
    }

    // ── compact style ───────────────────────────────────────────────────────

    #[test]
    fn compact_style_accepts_nested_tight_outer() {
        // compact: nested hashes collapse outer braces but keep inner spaced.
        let opts = SpaceInsideHashLiteralBracesOptions {
            enforced_style: HashBraceStyle::Compact,
            empty_style: EmptyHashBraceStyle::NoSpace,
        };
        test::<SpaceInsideHashLiteralBraces>()
            .with_options(&opts)
            .expect_no_offenses("h = { a: { b: 1 }}\n");
    }

    // ── empty braces style ──────────────────────────────────────────────────

    #[test]
    fn empty_space_style_flags_tight_empty_braces() {
        let opts = SpaceInsideHashLiteralBracesOptions {
            enforced_style: HashBraceStyle::Space,
            empty_style: EmptyHashBraceStyle::Space,
        };
        let result = murphy_plugin_api::test_support::run_cop_with_options_and_edits::<
            SpaceInsideHashLiteralBraces,
        >("h = {}\n", &opts);
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(
            result.offenses[0].message,
            "Space inside empty hash literal braces missing."
        );
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    // ── cross-cop: must NOT fire on brace blocks ────────────────────────────

    #[test]
    fn does_not_flag_brace_block() {
        // A brace block `{ x }` is NOT a hash literal; this cop must ignore it.
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses("foo { x }\n");
    }

    #[test]
    fn does_not_flag_tight_brace_block() {
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses("foo {x}\n");
    }

    #[test]
    fn does_not_flag_kwargs() {
        // Bare keyword args have no brace tokens.
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses("foo(a: 1, b: 2)\n");
    }

    #[test]
    fn accepts_multiline_hash() {
        test::<SpaceInsideHashLiteralBraces>().expect_no_offenses(indoc! {r#"
            h = {
              a: 1,
              b: 2,
            }
        "#});
    }
}
murphy_plugin_api::submit_cop!(SpaceInsideHashLiteralBraces);
