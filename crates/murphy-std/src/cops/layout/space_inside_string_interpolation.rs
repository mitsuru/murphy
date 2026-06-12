//! `Layout/SpaceInsideStringInterpolation` — enforces consistent padding
//! inside `#{...}` string interpolation delimiters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideStringInterpolation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop: subscribe to interpolation `begin` parts of
//!   dstr/dsym/xstr/regexp nodes. Multiline interpolations and empty `#{}` are
//!   skipped. `no_space` (default) removes whitespace runs (spaces and tabs)
//!   directly after `#{` and before `}`; `space` ensures exactly one space on
//!   each side, inserting where missing. The offense ranges and autocorrects
//!   match RuboCop's `SurroundingSpace`/`SpaceCorrector`: the leading run is
//!   highlighted for the after-`#{` side, the trailing run for the before-`}`
//!   side, and missing-space offenses highlight the `#{`/`}` delimiter token.
//!   Literal spacing inside the interpolated expression is untouched.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct SpaceInsideStringInterpolation;

#[derive(CopOptions)]
pub struct SpaceInsideStringInterpolationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "no_space",
        description = "String interpolation padding style."
    )]
    pub enforced_style: SpaceInsideStringInterpolationStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideStringInterpolationStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceInsideStringInterpolation",
    description = "Checks for padding/surrounding spaces inside string interpolation.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideStringInterpolationOptions,
)]
impl SpaceInsideStringInterpolation {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_interpolation_part(node, cx) {
            return;
        }
        let node_range = cx.range(node);
        // `#{` opener (2 bytes) … `}` closer (1 byte).
        if node_range.end < node_range.start + 3 {
            return;
        }
        let content_start = node_range.start + 2;
        let content_end = node_range.end - 1;
        let content = cx.raw_source(Range {
            start: content_start,
            end: content_end,
        });

        // Skip multiline interpolations (RuboCop: `begin_node.multiline?`).
        if content.contains('\n') || content.contains('\r') {
            return;
        }
        // Skip empty interpolation `#{}` (RuboCop: `empty_brackets?`).
        if content.is_empty() {
            return;
        }

        let opts = cx.options_or_default::<SpaceInsideStringInterpolationOptions>();
        match opts.enforced_style {
            SpaceInsideStringInterpolationStyle::NoSpace => {
                no_space(content, content_start, content_end, cx);
            }
            SpaceInsideStringInterpolationStyle::Space => {
                space(node_range, content, content_start, content_end, cx);
            }
        }
    }
}

const MSG_NO_SPACE: &str = "Do not use space inside string interpolation.";
const MSG_SPACE: &str = "Use space inside string interpolation.";

fn is_interpolation_part(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        cx.kind(parent),
        NodeKind::Dstr(_) | NodeKind::Dsym(_) | NodeKind::Xstr(_) | NodeKind::Regexp { .. }
    )
}

/// `no_space` style: flag and remove the whitespace run (spaces/tabs) directly
/// after `#{` and directly before `}`.
fn no_space(content: &str, content_start: u32, content_end: u32, cx: &Cx<'_>) {
    let bytes = content.as_bytes();
    let lead = bytes.iter().take_while(|&&b| is_space(b)).count() as u32;
    if lead > 0 {
        let range = Range {
            start: content_start,
            end: content_start + lead,
        };
        cx.emit_offense(range, MSG_NO_SPACE, None);
        cx.emit_edit(range, "");
    }

    let trail = bytes.iter().rev().take_while(|&&b| is_space(b)).count() as u32;
    // Avoid double-flagging when the body is entirely whitespace — but empty
    // and pure-whitespace bodies cannot occur here: empty `#{}` is skipped, and
    // a pure-whitespace body means `lead == content.len()`, in which case the
    // trailing run overlaps the leading one. Guard against the overlap.
    if trail > 0 && content_end - trail >= content_start + lead {
        let range = Range {
            start: content_end - trail,
            end: content_end,
        };
        cx.emit_offense(range, MSG_NO_SPACE, None);
        cx.emit_edit(range, "");
    }
}

/// `space` style: ensure exactly one space after `#{` and before `}`. Missing
/// space offenses highlight the `#{` / `}` delimiter token; extra whitespace is
/// left for the layout/extra-space cop.
fn space(node: Range, content: &str, _content_start: u32, content_end: u32, cx: &Cx<'_>) {
    let bytes = content.as_bytes();
    let has_lead = bytes.first().is_some_and(|&b| is_space(b));
    if !has_lead {
        // Highlight the `#{` opener (2 bytes) and insert a space after it.
        let delim = Range {
            start: node.start,
            end: node.start + 2,
        };
        cx.emit_offense(delim, MSG_SPACE, None);
        cx.emit_edit(
            Range {
                start: node.start + 2,
                end: node.start + 2,
            },
            " ",
        );
    }

    let has_trail = bytes.last().is_some_and(|&b| is_space(b));
    if !has_trail {
        // Highlight the `}` closer (1 byte) and insert a space before it.
        let delim = Range {
            start: content_end,
            end: content_end + 1,
        };
        cx.emit_offense(delim, MSG_SPACE, None);
        cx.emit_edit(
            Range {
                start: content_end,
                end: content_end,
            },
            " ",
        );
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

murphy_plugin_api::submit_cop!(SpaceInsideStringInterpolation);

#[cfg(test)]
mod tests {
    use super::{
        SpaceInsideStringInterpolation as Cop, SpaceInsideStringInterpolationOptions as Opts,
        SpaceInsideStringInterpolationStyle as Style,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    // ── no_space style ──────────────────────────────────────────────────────

    #[test]
    fn no_space_removes_leading_space() {
        test::<Cop>().expect_correction(
            indoc! {r##"
                "#{ var}"
                   ^ Do not use space inside string interpolation.
            "##},
            "\"#{var}\"\n",
        );
    }

    #[test]
    fn no_space_removes_trailing_space() {
        test::<Cop>().expect_correction(
            indoc! {r##"
                "#{var }"
                      ^ Do not use space inside string interpolation.
            "##},
            "\"#{var}\"\n",
        );
    }

    #[test]
    fn no_space_removes_both_runs() {
        test::<Cop>().expect_correction(
            indoc! {r##"
                "#{   var   }"
                   ^^^ Do not use space inside string interpolation.
                         ^^^ Do not use space inside string interpolation.
            "##},
            "\"#{var}\"\n",
        );
    }

    #[test]
    fn no_space_treats_tab_as_space() {
        // A trailing tab before `}` is flagged like a space. Verified via the
        // offense+edits helper since a literal tab in a caret annotation is
        // brittle.
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let result = run_cop_with_edits::<Cop>("\"#{var\t}\"\n");
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(result.offenses[0].message, super::MSG_NO_SPACE);
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, "");
    }

    #[test]
    fn no_space_finds_interpolation_in_regexp_xstr_dsym() {
        test::<Cop>()
            .expect_correction(
                indoc! {r##"
                    /regexp #{ var}/
                              ^ Do not use space inside string interpolation.
                "##},
                "/regexp #{var}/\n",
            )
            .expect_correction(
                indoc! {r##"
                    `backticks #{ var}`
                                 ^ Do not use space inside string interpolation.
                "##},
                "`backticks #{var}`\n",
            )
            .expect_correction(
                indoc! {r##"
                    :"symbol #{ var}"
                               ^ Do not use space inside string interpolation.
                "##},
                ":\"symbol #{var}\"\n",
            );
    }

    #[test]
    fn no_space_does_not_touch_inner_expression_spaces() {
        test::<Cop>().expect_correction(
            indoc! {r##"
                "#{ a; b }"
                   ^ Do not use space inside string interpolation.
                        ^ Do not use space inside string interpolation.
            "##},
            "\"#{a; b}\"\n",
        );
    }

    #[test]
    fn no_space_accepts_clean_and_excess_literal_spacing() {
        test::<Cop>()
            .expect_no_offenses("\"#{var}\"\n")
            .expect_no_offenses("\"Variable is    #{var}      \"\n")
            .expect_no_offenses("\"  Variable is  #{var}\"\n");
    }

    #[test]
    fn no_space_accepts_empty_interpolation() {
        test::<Cop>().expect_no_offenses("\"#{}\"\n");
    }

    #[test]
    fn no_space_accepts_multiline_interpolation() {
        test::<Cop>().expect_no_offenses(indoc! {r##"
            "#{
              code
            }"
        "##});
    }

    // ── space style ─────────────────────────────────────────────────────────

    #[test]
    fn space_inserts_both_spaces() {
        let opts = Opts {
            enforced_style: Style::Space,
        };
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r##"
                "#{var}"
                 ^^ Use space inside string interpolation.
                      ^ Use space inside string interpolation.
            "##},
            "\"#{ var }\"\n",
        );
    }

    #[test]
    fn space_inserts_missing_trailing_space() {
        let opts = Opts {
            enforced_style: Style::Space,
        };
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r##"
                "#{ var}"
                       ^ Use space inside string interpolation.
            "##},
            "\"#{ var }\"\n",
        );
    }

    #[test]
    fn space_inserts_missing_leading_space() {
        let opts = Opts {
            enforced_style: Style::Space,
        };
        test::<Cop>().with_options(&opts).expect_correction(
            indoc! {r##"
                "#{var }"
                 ^^ Use space inside string interpolation.
            "##},
            "\"#{ var }\"\n",
        );
    }

    #[test]
    fn space_accepts_well_formatted() {
        let opts = Opts {
            enforced_style: Style::Space,
        };
        test::<Cop>()
            .with_options(&opts)
            .expect_no_offenses("\"Variable is    #{ var }      \"\n")
            .expect_no_offenses("\"  Variable is  #{ var }\"\n");
    }

    #[test]
    fn space_accepts_empty_interpolation() {
        let opts = Opts {
            enforced_style: Style::Space,
        };
        test::<Cop>()
            .with_options(&opts)
            .expect_no_offenses("\"#{}\"\n");
    }
}
