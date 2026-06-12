//! `Layout/SpaceInsideReferenceBrackets` — flags spacing inside reference
//! (`foo[...]` / `foo[...] = x`) brackets.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideReferenceBrackets
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Subscribes to `send` nodes whose method is `[]`/`[]=` (Murphy parses
//!   `foo[i]` and `foo[i] = x` as those sends). The send's `loc.name` is the
//!   whole `[...]` bracket span, so the opening `[` and closing `]` are read
//!   directly without depth-scanning. Reference brackets are distinguished from
//!   explicit method-call syntax (`subject.[](0)`, `def Vector.[]`) by the
//!   call-operator (`.`) being absent: a true reference bracket has no dot.
//!   Array literals (`[1, 2]`) are `array` nodes and so are never reached.
//!
//!   EnforcedStyle (no_space/space) governs non-empty brackets;
//!   EnforcedStyleForEmptyBrackets (no_space/space) governs empty/whitespace-only
//!   brackets and is checked before the multiline guard (so `a[\n]` is still
//!   flagged). Non-empty multiline brackets are accepted.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct SpaceInsideReferenceBrackets;

#[derive(CopOptions)]
pub struct SpaceInsideReferenceBracketsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "no_space",
        description = "Spacing style for non-empty reference brackets."
    )]
    pub enforced_style: ReferenceBracketStyle,
    #[option(
        name = "EnforcedStyleForEmptyBrackets",
        default = "no_space",
        description = "Spacing style for empty reference brackets."
    )]
    pub enforced_style_for_empty_brackets: ReferenceBracketStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceBracketStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceInsideReferenceBrackets",
    description = "Checks the spacing inside referential brackets.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideReferenceBracketsOptions,
)]
impl SpaceInsideReferenceBrackets {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG_NO_SPACE: &str = "Do not use space inside reference brackets.";
const MSG_SPACE: &str = "Use space inside reference brackets.";
const EMPTY_MSG_NO_SPACE: &str = "Do not use space inside empty reference brackets.";
const EMPTY_MSG_SPACE: &str = "Use one space inside empty reference brackets.";

fn check(node: NodeId, cx: &Cx<'_>) {
    // Reference brackets are `foo[...]` / `foo[...] = x`, parsed as sends with
    // method `[]`/`[]=`. Explicit method-call syntax (`subject.[](0)`) carries a
    // call operator (`.`); a true reference bracket has none.
    let Some(method) = cx.method_name(node) else {
        return;
    };
    if !matches!(method, "[]" | "[]=") {
        return;
    }
    let dot = cx.loc(node).dot();
    if dot.start != dot.end {
        return; // explicit `.[]` method-call syntax — not a reference bracket
    }

    // `loc.name` spans the whole `[...]` bracket region (for `[]=` it covers the
    // brackets only, not the RHS). The `[` and `]` are its first/last bytes.
    let name = cx.loc(node).name;
    if name.end < name.start + 2 {
        return;
    }
    let src = cx.source().as_bytes();
    if src.get(name.start as usize) != Some(&b'[') || src.get((name.end - 1) as usize) != Some(&b']')
    {
        return;
    }
    let left = Range {
        start: name.start,
        end: name.start + 1,
    };
    let right = Range {
        start: name.end - 1,
        end: name.end,
    };

    // Inner content between `[` and `]`.
    let inner = Range {
        start: left.end,
        end: right.start,
    };
    if inner.start > inner.end {
        return;
    }
    let inner_src = cx.raw_source(inner);

    let opts = cx.options_or_default::<SpaceInsideReferenceBracketsOptions>();

    // Empty-brackets path runs first (before any multiline guard): an empty or
    // whitespace-only body (including newlines) is governed by
    // EnforcedStyleForEmptyBrackets.
    if inner_src.is_empty() || inner_src.trim().is_empty() {
        empty_offense(
            left,
            right,
            inner,
            inner_src,
            opts.enforced_style_for_empty_brackets,
            cx,
        );
        return;
    }

    // Non-empty multiline brackets are accepted.
    if inner_src.contains('\n') || inner_src.contains('\r') {
        return;
    }

    match opts.enforced_style {
        ReferenceBracketStyle::NoSpace => no_space(left, right, cx),
        ReferenceBracketStyle::Space => space(left, right, cx),
    }
}

fn empty_offense(
    left: Range,
    right: Range,
    inner: Range,
    inner_src: &str,
    style: ReferenceBracketStyle,
    cx: &Cx<'_>,
) {
    match style {
        ReferenceBracketStyle::NoSpace => {
            if inner_src.is_empty() {
                return; // `[]` already correct
            }
            // Highlight `[ … ]` inclusive; remove the inner whitespace.
            let range = Range {
                start: left.start,
                end: right.end,
            };
            cx.emit_offense(range, EMPTY_MSG_NO_SPACE, None);
            cx.emit_edit(inner, "");
        }
        ReferenceBracketStyle::Space => {
            if inner_src == " " {
                return; // `[ ]` already correct
            }
            // Highlight `[ … ]` inclusive; normalise the inner to a single space.
            let range = Range {
                start: left.start,
                end: right.end,
            };
            cx.emit_offense(range, EMPTY_MSG_SPACE, None);
            cx.emit_edit(inner, " ");
        }
    }
}

/// `no_space`: flag and remove the whitespace run directly after `[` and before
/// `]`.
fn no_space(left: Range, right: Range, cx: &Cx<'_>) {
    let src = cx.source().as_bytes();

    let lead_end = whitespace_after(src, left.end, right.start);
    if lead_end > left.end {
        let range = Range {
            start: left.end,
            end: lead_end,
        };
        cx.emit_offense(range, MSG_NO_SPACE, None);
        cx.emit_edit(range, "");
    }

    let trail_start = whitespace_before(src, right.start, left.end);
    if trail_start < right.start {
        let range = Range {
            start: trail_start,
            end: right.start,
        };
        cx.emit_offense(range, MSG_NO_SPACE, None);
        cx.emit_edit(range, "");
    }
}

/// `space`: ensure exactly one space after `[` and before `]`. Missing-space
/// offenses highlight the `[`/`]` delimiter token.
fn space(left: Range, right: Range, cx: &Cx<'_>) {
    let src = cx.source().as_bytes();

    let has_lead = (left.end as usize) < src.len() && is_space(src[left.end as usize]);
    if !has_lead {
        cx.emit_offense(left, MSG_SPACE, None);
        cx.emit_edit(
            Range {
                start: left.end,
                end: left.end,
            },
            " ",
        );
    }

    let has_trail = right.start > 0 && is_space(src[(right.start - 1) as usize]);
    if !has_trail {
        cx.emit_offense(right, MSG_SPACE, None);
        cx.emit_edit(
            Range {
                start: right.start,
                end: right.start,
            },
            " ",
        );
    }
}

fn whitespace_after(src: &[u8], pos: u32, ceil: u32) -> u32 {
    let mut end = pos;
    while end < ceil && is_space(src[end as usize]) {
        end += 1;
    }
    end
}

fn whitespace_before(src: &[u8], pos: u32, floor: u32) -> u32 {
    let mut start = pos;
    while start > floor && is_space(src[(start - 1) as usize]) {
        start -= 1;
    }
    start
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

murphy_plugin_api::submit_cop!(SpaceInsideReferenceBrackets);

#[cfg(test)]
mod tests {
    use super::{
        ReferenceBracketStyle as Style,
        SpaceInsideReferenceBrackets as Cop,
        SpaceInsideReferenceBracketsOptions as Opts,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn space_opts() -> Opts {
        Opts {
            enforced_style: Style::Space,
            enforced_style_for_empty_brackets: Style::Space,
        }
    }

    fn empty_space_opts() -> Opts {
        Opts {
            enforced_style: Style::NoSpace,
            enforced_style_for_empty_brackets: Style::Space,
        }
    }

    // ── no_space (default) ──────────────────────────────────────────────────

    #[test]
    fn no_space_ignores_array_literals() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            a = [1, 2 ]
            b = [ 3, 4]
            c = [5, 6]
            d = [ 7, 8 ]
        "#});
    }

    #[test]
    fn no_space_accepts_tight_reference_brackets() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            a[1]
            b[index, 2]
            c["foo"]
            d[:bar]
            e[]
            a[1] = 2
            e[] = foo
        "#});
    }

    #[test]
    fn no_space_corrects_leading_whitespace() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                a[  :key]
                  ^^ Do not use space inside reference brackets.
            "#},
            "a[:key]\n",
        );
    }

    #[test]
    fn no_space_corrects_trailing_whitespace() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                b[:key ]
                      ^ Do not use space inside reference brackets.
            "#},
            "b[:key]\n",
        );
    }

    #[test]
    fn no_space_corrects_both_sides() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                b[ 89  ]
                  ^ Do not use space inside reference brackets.
                     ^^ Do not use space inside reference brackets.
            "#},
            "b[89]\n",
        );
    }

    #[test]
    fn no_space_corrects_second_brackets() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                a[:key][ "key"]
                        ^ Do not use space inside reference brackets.
            "#},
            "a[:key][\"key\"]\n",
        );
    }

    #[test]
    fn no_space_corrects_nested_outer_brackets_only() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                record[ options[:attribute] ]
                       ^ Do not use space inside reference brackets.
                                           ^ Do not use space inside reference brackets.
            "#},
            "record[options[:attribute]]\n",
        );
    }

    #[test]
    fn no_space_accepts_array_as_reference_object() {
        test::<Cop>().expect_no_offenses("a[[ 1, 2 ]]\n");
    }

    #[test]
    fn no_space_accepts_method_call_syntax() {
        test::<Cop>()
            .expect_no_offenses("subject.[](0)\n")
            .expect_no_offenses("def Vector.[](*array)\nend\n");
    }

    #[test]
    fn no_space_accepts_multiline_non_empty() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            foo[
              bar
            ]
        "#});
    }

    #[test]
    fn no_space_corrects_assignment_brackets() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                a[ "foo"] = b["something"]
                  ^ Do not use space inside reference brackets.
            "#},
            "a[\"foo\"] = b[\"something\"]\n",
        );
    }

    // ── empty brackets ──────────────────────────────────────────────────────

    #[test]
    fn empty_no_space_corrects_single_space() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                foo[ ]
                   ^^^ Do not use space inside empty reference brackets.
            "#},
            "foo[]\n",
        );
    }

    #[test]
    fn empty_no_space_corrects_multiple_spaces() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                a[     ]
                 ^^^^^^^ Do not use space inside empty reference brackets.
            "#},
            "a[]\n",
        );
    }

    #[test]
    fn empty_no_space_accepts_tight() {
        test::<Cop>().expect_no_offenses("a[]\n");
    }

    #[test]
    fn empty_space_corrects_no_space() {
        let opts = empty_space_opts();
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r#"
                foo[]
                   ^^ Use one space inside empty reference brackets.
            "#},
            "foo[ ]\n",
        );
    }

    #[test]
    fn empty_space_corrects_multiple_spaces() {
        let opts = empty_space_opts();
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r#"
                a[      ]
                 ^^^^^^^^ Use one space inside empty reference brackets.
            "#},
            "a[ ]\n",
        );
    }

    #[test]
    fn empty_space_accepts_single_space() {
        let opts = empty_space_opts();
        test::<Cop>().with_options(&opts).expect_no_offenses("a[ ]\n");
    }

    // ── space style ─────────────────────────────────────────────────────────

    #[test]
    fn space_accepts_well_spaced() {
        let opts = space_opts();
        test::<Cop>().with_options(&opts).expect_no_offenses(indoc! {r#"
            a[ 1 ]
            b[ index, 3 ]
            c[ "foo" ]
            d[ :bar ]
            e[ ]
        "#});
    }

    #[test]
    fn space_ignores_array_literals() {
        let opts = space_opts();
        test::<Cop>().with_options(&opts).expect_no_offenses(indoc! {r#"
            a = [1, 2 ]
            b = [ 3, 4]
        "#});
    }

    #[test]
    fn space_corrects_missing_leading_space() {
        let opts = space_opts();
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r#"
                a[:key ]
                 ^ Use space inside reference brackets.
            "#},
            "a[ :key ]\n",
        );
    }

    #[test]
    fn space_corrects_missing_trailing_space() {
        let opts = space_opts();
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r#"
                a[ "foo"] = b[ "something" ]
                        ^ Use space inside reference brackets.
            "#},
            "a[ \"foo\" ] = b[ \"something\" ]\n",
        );
    }

    #[test]
    fn space_corrects_both_missing() {
        let opts = space_opts();
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r#"
                a[1]
                 ^ Use space inside reference brackets.
                   ^ Use space inside reference brackets.
            "#},
            "a[ 1 ]\n",
        );
    }
}
