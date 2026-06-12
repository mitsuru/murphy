//! `Layout/SpaceInLambdaLiteral` — enforces consistent spacing between the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInLambdaLiteral
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_send` for stabby lambdas with a parenthesized
//!   argument list. Murphy lowers `-> (params) { body }` to
//!   `Block { call: Lambda, args, body }` where the `Lambda` node's range is
//!   the `->` operator. The args node's range is the whole block (not the
//!   `(params)` span), so the `(` position is found by scanning the source
//!   bytes between the `->` end and the first `(`. `EnforcedStyle`
//!   (`require_no_space` default / `require_space`) matches RuboCop verbatim;
//!   `-> { }` and `-> () { }` (no parenthesized arguments) are skipped.
//! ```
//!
//! lambda arrow (`->`) and its parenthesized argument list. Mirrors
//! RuboCop's same-named cop; `EnforcedStyle: require_no_space` (default)
//! or `require_space`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceInLambdaLiteral;

#[derive(CopOptions)]
pub struct SpaceInLambdaLiteralOptions {
    #[option(
        name = "EnforcedStyle",
        default = "require_no_space",
        description = "Whether a space is required between `->` and `(`."
    )]
    pub enforced_style: SpaceInLambdaLiteralStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInLambdaLiteralStyle {
    #[option(value = "require_no_space")]
    RequireNoSpace,
    #[option(value = "require_space")]
    RequireSpace,
}

const MSG_REQUIRE_SPACE: &str = "Use a space between `->` and `(` in lambda literals.";
const MSG_REQUIRE_NO_SPACE: &str = "Do not use spaces between `->` and `(` in lambda literals.";

#[cop(
    name = "Layout/SpaceInLambdaLiteral",
    description = "Enforce spacing between `->` and `(` in lambda literals.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInLambdaLiteralOptions,
)]
impl SpaceInLambdaLiteral {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // The block must wrap a stabby-lambda `->` (`call` is `Lambda`).
    let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
        return;
    };
    if !matches!(*cx.kind(call), NodeKind::Lambda) {
        return;
    }
    // `arrow_lambda_with_args?`: the lambda must have a parenthesized
    // argument list. An empty `Args` list (`-> { }`) is skipped.
    let NodeKind::Args(list) = *cx.kind(args) else {
        return;
    };
    if cx.list(list).is_empty() {
        return;
    }

    let arrow_end = cx.range(call).end as usize;
    let src = cx.source().as_bytes();

    // Locate the `(` after the arrow. The bytes between the arrow and the
    // paren are the "space after arrow" (RuboCop's `arrow.end.join(parens.begin)`).
    let Some(paren_offset) = find_open_paren(src, arrow_end) else {
        // `-> proc_without_parens` is not valid stabby-lambda syntax, but guard
        // anyway: with no `(` there is nothing to check.
        return;
    };

    let opts = cx.options_or_default::<SpaceInLambdaLiteralOptions>();
    let space = Range {
        start: arrow_end as u32,
        end: paren_offset as u32,
    };
    let has_space = space.end > space.start;

    match opts.enforced_style {
        SpaceInLambdaLiteralStyle::RequireNoSpace => {
            if has_space {
                cx.emit_offense(space, MSG_REQUIRE_NO_SPACE, None);
                cx.emit_edit(space, "");
            }
        }
        SpaceInLambdaLiteralStyle::RequireSpace => {
            if !has_space {
                // Offense range mirrors RuboCop's `range_of_offense`
                // (`->` through the argument list); the autocorrect inserts a
                // single space before `(`.
                let arg_end = arg_list_end(cx, paren_offset);
                let offense = Range {
                    start: cx.range(call).start,
                    end: arg_end as u32,
                };
                let insert = Range {
                    start: paren_offset as u32,
                    end: paren_offset as u32,
                };
                cx.emit_offense(offense, MSG_REQUIRE_SPACE, None);
                cx.emit_edit(insert, " ");
            }
        }
    }
}

/// Scan forward from `from` over spaces/tabs to the first `(`. Returns the
/// byte offset of the `(`, or `None` if a non-space, non-paren byte is hit
/// first.
fn find_open_paren(src: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < src.len() {
        match src[i] {
            b' ' | b'\t' => i += 1,
            b'(' => return Some(i),
            _ => return None,
        }
    }
    None
}

/// Find the byte offset just past the matching `)` of the parenthesized
/// argument list opening at `paren_offset`. Used to size the `require_space`
/// offense range. Matches parens on lexed tokens (via `cx.sorted_tokens()`)
/// so parentheses inside string literals / symbols / regexps in a default
/// value (`->(x = "foo)") { }`) do not throw off the depth count. Falls back
/// to `paren_offset + 1` if the opener is not found or is unbalanced.
fn arg_list_end(cx: &Cx<'_>, paren_offset: usize) -> usize {
    let tokens = cx.sorted_tokens();
    let Some(start_idx) = tokens
        .iter()
        .position(|t| t.range.start == paren_offset as u32)
    else {
        return paren_offset + 1;
    };
    let mut depth = 0usize;
    for tok in &tokens[start_idx..] {
        // The `(` after `->` may lex as `Other`, so match on source bytes
        // rather than `SourceTokenKind::LeftParen`/`RightParen`.
        match cx.raw_source(tok.range) {
            "(" => depth += 1,
            // The `depth > 0` guard prevents `usize` underflow: error-tolerant
            // parsing of incomplete source can surface a `)` while `depth == 0`,
            // which then falls through to the unbalanced `paren_offset + 1`
            // fallback below.
            ")" if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return tok.range.end as usize;
                }
            }
            _ => {}
        }
    }
    paren_offset + 1
}

#[cfg(test)]
mod tests {
    use super::{SpaceInLambdaLiteral, SpaceInLambdaLiteralOptions, SpaceInLambdaLiteralStyle};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options_and_edits, test};

    // ── require_no_space (default) ────────────────────────────────────────────

    #[test]
    fn default_flags_space_after_arrow() {
        test::<SpaceInLambdaLiteral>().expect_correction(
            indoc! {r#"
                f = -> (x) { x }
                      ^ Do not use spaces between `->` and `(` in lambda literals.
            "#},
            "f = ->(x) { x }\n",
        );
    }

    #[test]
    fn default_accepts_no_space_after_arrow() {
        test::<SpaceInLambdaLiteral>().expect_no_offenses("f = ->(x) { x }\n");
    }

    #[test]
    fn default_accepts_lambda_without_args() {
        test::<SpaceInLambdaLiteral>().expect_no_offenses("f = -> { 1 }\n");
    }

    #[test]
    fn default_accepts_lambda_with_empty_parens() {
        // `-> () { }` has no arguments, so RuboCop's `arguments?` is false and
        // the cop does not fire.
        test::<SpaceInLambdaLiteral>().expect_no_offenses("f = -> () { 1 }\n");
    }

    #[test]
    fn default_flags_multiple_spaces_after_arrow() {
        test::<SpaceInLambdaLiteral>().expect_correction(
            indoc! {r#"
                f = ->   (x) { x }
                      ^^^ Do not use spaces between `->` and `(` in lambda literals.
            "#},
            "f = ->(x) { x }\n",
        );
    }

    // ── require_space ─────────────────────────────────────────────────────────

    #[test]
    fn require_space_accepts_space_after_arrow() {
        let opts = SpaceInLambdaLiteralOptions {
            enforced_style: SpaceInLambdaLiteralStyle::RequireSpace,
        };
        test::<SpaceInLambdaLiteral>()
            .with_options(&opts)
            .expect_no_offenses("f = -> (x) { x }\n");
    }

    /// `require_space` with no space inserts one. The autocorrect edit is a
    /// zero-length insert point; verify via run_cop + edits.
    #[test]
    fn require_space_flags_missing_space() {
        let opts = SpaceInLambdaLiteralOptions {
            enforced_style: SpaceInLambdaLiteralStyle::RequireSpace,
        };
        let result =
            run_cop_with_options_and_edits::<SpaceInLambdaLiteral>("f = ->(x) { x }\n", &opts);
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            result.offenses[0].message,
            "Use a space between `->` and `(` in lambda literals."
        );
        assert_eq!(result.edits.len(), 1, "expected 1 edit");
        assert_eq!(result.edits[0].replacement, " ");
    }

    /// `require_space` offense range must span the full argument list. A `)`
    /// inside a default-value string literal (`->(x = "foo)") { }`) must not
    /// truncate the range — `arg_list_end` matches parens on lexed tokens, so
    /// the inner `)` is part of one `Str` token and is skipped.
    #[test]
    fn require_space_offense_range_spans_string_literal_paren() {
        let opts = SpaceInLambdaLiteralOptions {
            enforced_style: SpaceInLambdaLiteralStyle::RequireSpace,
        };
        let source = "f = ->(x = \"foo)\") { x }\n";
        let result = run_cop_with_options_and_edits::<SpaceInLambdaLiteral>(source, &opts);
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        // Offense range runs from `->` through the real closing `)` of the
        // argument list, i.e. it includes `(x = "foo)")` in full.
        let end = result.offenses[0].range.end as usize;
        let real_close = source.rfind(')').expect("closing paren present");
        assert_eq!(
            end,
            real_close + 1,
            "offense should end just past the real `)`, got byte {end} in {source:?}"
        );
    }

    #[test]
    fn require_space_correction_roundtrips() {
        let opts = SpaceInLambdaLiteralOptions {
            enforced_style: SpaceInLambdaLiteralStyle::RequireSpace,
        };
        let result =
            run_cop_with_options_and_edits::<SpaceInLambdaLiteral>("f = ->(x) { x }\n", &opts);
        // Apply the single insert edit and confirm the corrected source.
        let edit = &result.edits[0];
        let mut corrected = "f = ->(x) { x }\n".to_string();
        corrected.insert_str(edit.range.start as usize, &edit.replacement);
        assert_eq!(corrected, "f = -> (x) { x }\n");
    }
}

murphy_plugin_api::submit_cop!(SpaceInLambdaLiteral);
