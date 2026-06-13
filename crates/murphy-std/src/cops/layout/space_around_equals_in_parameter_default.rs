//! `Layout/SpaceAroundEqualsInParameterDefault` — checks the spacing around
//! the `=` in an optional parameter default (`def f(a = 1)`), enforcing either
//! surrounding space (default) or no surrounding space depending on
//! `EnforcedStyle`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundEqualsInParameterDefault
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-8zw3
//!   - murphy-ilrx
//! notes: >
//!   Dispatches on `Optarg` (`def f(a = 1)`). `Kwoptarg` (`def f(k: 1)`) uses a
//!   `:` not an `=`, so RuboCop's cop never matches it — Murphy follows suit and
//!   does not dispatch there. The `=` token is located between the parameter
//!   name and its default value; spacing is checked on both sides. Offense range
//!   spans from the end of the parameter name to the start of the default value,
//!   matching RuboCop's `range_between(arg.end_pos, value.begin_pos)`.
//!
//!   murphy-8zw3 (no observable gap — architecturally non-portable): RuboCop's
//!   `ConfigurableEnforcedStyle` `correct_style_detected` /
//!   `opposite_style_detected` calls feed two RuboCop-only subsystems —
//!   `--auto-gen-config` TODO-file generation and cross-run/cross-file style
//!   ambiguity tracking — neither of which exists in Murphy. They do NOT change
//!   the offenses or autocorrections RuboCop reports for any given file:
//!   `incorrect_style_detected` always adds the offense, and the autocorrect
//!   always runs. A mixed-style file (`def a(x=1, y = 2); end` under the default
//!   `space` style) reports exactly one offense in both RuboCop and Murphy (the
//!   `x=1`), so there is no input that distinguishes the two and thus no
//!   TDD-drivable behavior to port. Wiring this would require a cross-cop /
//!   cross-investigation style-tracking subsystem in murphy-core (an ABI-level
//!   change), not a cop-body change; it is intentionally not attempted here and
//!   would not alter parity on emitted offenses.
//! ```
//!
//! ## Options
//!
//! - `EnforcedStyle` (`space` | `no_space`, default `space`) — `space` requires
//!   ` = ` around the default `=`; `no_space` requires `=` with no surrounding
//!   space.
//!
//! ## Autocorrect
//!
//! Replaces the `name<gap>=<gap>value` region with `name = value` (space style)
//! or `name=value` (no_space style), preserving the value source byte-for-byte.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAroundEqualsInParameterDefault;

#[derive(CopOptions)]
pub struct SpaceAroundEqualsInParameterDefaultOptions {
    #[option(
        name = "EnforcedStyle",
        default = "space",
        description = "Whether the default-value `=` should be surrounded by space."
    )]
    pub enforced_style: ParameterDefaultStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParameterDefaultStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
}

#[cop(
    name = "Layout/SpaceAroundEqualsInParameterDefault",
    description = "Check the spacing around the `=` in optional parameter defaults.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceAroundEqualsInParameterDefaultOptions,
)]
impl SpaceAroundEqualsInParameterDefault {
    #[on_node(kind = "optarg")]
    fn check_optarg(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Optarg { default, .. } = *cx.kind(node) else {
            return;
        };
        let style = cx
            .options_or_default::<SpaceAroundEqualsInParameterDefaultOptions>()
            .enforced_style;

        // The parameter-name range is the optarg node start through the `=`.
        // Find the `=` token in the span [optarg.start, default.start).
        let arg_start = cx.range(node).start;
        let value_start = cx.range(default).start;
        if value_start <= arg_start {
            return;
        }
        let search = Range {
            start: arg_start,
            end: value_start,
        };
        let Some(eq) = cx
            .tokens_in(search)
            .iter()
            .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "=")
        else {
            return;
        };

        // RuboCop's `arg` is the parameter name; its end is the byte right
        // before any space before `=`. We bound the offense range from the end
        // of the name to the start of the value. The name ends at the first
        // non-space byte walking back from `=`.
        let src = cx.source().as_bytes();
        let mut name_end = eq.range.start as usize;
        while name_end > arg_start as usize && matches!(src[name_end - 1], b' ' | b'\t') {
            name_end -= 1;
        }
        let name_end = name_end as u32;

        let space_before = eq.range.start > name_end;
        let space_after = value_start > eq.range.end;

        let offense_range = Range {
            start: name_end,
            end: value_start,
        };

        match style {
            ParameterDefaultStyle::Space => {
                if space_before && space_after {
                    return;
                }
                cx.emit_offense(
                    offense_range,
                    "Surrounding space missing in default value assignment.",
                    None,
                );
                emit_value_preserving_edit(cx, name_end, value_start, " = ");
            }
            ParameterDefaultStyle::NoSpace => {
                if !space_before && !space_after {
                    return;
                }
                cx.emit_offense(
                    offense_range,
                    "Surrounding space detected in default value assignment.",
                    None,
                );
                emit_value_preserving_edit(cx, name_end, value_start, "=");
            }
        }
    }
}

/// Replace the `name_end..value_start` region with `replacement`. The value
/// source is untouched (the edit ends exactly at `value_start`).
fn emit_value_preserving_edit(cx: &Cx<'_>, name_end: u32, value_start: u32, replacement: &str) {
    cx.emit_edit(
        Range {
            start: name_end,
            end: value_start,
        },
        replacement,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        ParameterDefaultStyle, SpaceAroundEqualsInParameterDefault,
        SpaceAroundEqualsInParameterDefaultOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn options_default_is_space() {
        let d = SpaceAroundEqualsInParameterDefaultOptions::default();
        assert_eq!(d.enforced_style, ParameterDefaultStyle::Space);
    }

    // ---------- space style (default) ----------

    #[test]
    fn flags_and_corrects_missing_space_default_style() {
        test::<SpaceAroundEqualsInParameterDefault>()
            .expect_offense(indoc! {r#"
                def f(x=0); end
                       ^ Surrounding space missing in default value assignment.
            "#})
            .expect_correction(
                indoc! {r#"
                    def f(x=0); end
                           ^ Surrounding space missing in default value assignment.
                "#},
                "def f(x = 0); end\n",
            );
    }

    #[test]
    fn flags_missing_space_on_one_side_only() {
        test::<SpaceAroundEqualsInParameterDefault>().expect_offense(indoc! {r#"
            def f(x =0); end
                   ^^ Surrounding space missing in default value assignment.
        "#});
    }

    #[test]
    fn flags_missing_space_after_equals_only() {
        test::<SpaceAroundEqualsInParameterDefault>().expect_offense(indoc! {r#"
            def f(x= 0); end
                   ^^ Surrounding space missing in default value assignment.
        "#});
    }

    #[test]
    fn accepts_well_spaced_default() {
        test::<SpaceAroundEqualsInParameterDefault>().expect_no_offenses("def f(x = 0); end\n");
    }

    #[test]
    fn corrects_preserving_value_source() {
        test::<SpaceAroundEqualsInParameterDefault>().expect_correction(
            indoc! {r#"
                def f(name="default"); end
                          ^ Surrounding space missing in default value assignment.
            "#},
            "def f(name = \"default\"); end\n",
        );
    }

    #[test]
    fn handles_multiple_optargs() {
        test::<SpaceAroundEqualsInParameterDefault>().expect_correction(
            indoc! {r#"
                def f(a=1, b=2); end
                       ^ Surrounding space missing in default value assignment.
                            ^ Surrounding space missing in default value assignment.
            "#},
            "def f(a = 1, b = 2); end\n",
        );
    }

    #[test]
    fn preserves_negative_default_value_sign() {
        // The replacement ends exactly at the value start; Prism folds the sign
        // into the `(int -1)` literal, so the `-` is preserved byte-for-byte.
        test::<SpaceAroundEqualsInParameterDefault>().expect_correction(
            indoc! {r#"
                def f(x=-1); end
                       ^ Surrounding space missing in default value assignment.
            "#},
            "def f(x = -1); end\n",
        );
    }

    #[test]
    fn ignores_keyword_optional_parameters() {
        // `def f(k: 1)` uses a colon, not an `=` — out of scope, like RuboCop.
        test::<SpaceAroundEqualsInParameterDefault>().expect_no_offenses("def f(k: 1); end\n");
    }

    #[test]
    fn ignores_required_and_splat_params() {
        test::<SpaceAroundEqualsInParameterDefault>()
            .expect_no_offenses("def f(a, *b, **c, &d); end\n");
    }

    // ---------- no_space style ----------

    #[test]
    fn no_space_style_flags_and_corrects_surrounding_space() {
        test::<SpaceAroundEqualsInParameterDefault>()
            .with_options(&SpaceAroundEqualsInParameterDefaultOptions {
                enforced_style: ParameterDefaultStyle::NoSpace,
            })
            .expect_offense(indoc! {r#"
                def f(x = 0); end
                       ^^^ Surrounding space detected in default value assignment.
            "#})
            .expect_correction(
                indoc! {r#"
                    def f(x = 0); end
                           ^^^ Surrounding space detected in default value assignment.
                "#},
                "def f(x=0); end\n",
            );
    }

    #[test]
    fn no_space_style_accepts_no_space() {
        test::<SpaceAroundEqualsInParameterDefault>()
            .with_options(&SpaceAroundEqualsInParameterDefaultOptions {
                enforced_style: ParameterDefaultStyle::NoSpace,
            })
            .expect_no_offenses("def f(x=0); end\n");
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAroundEqualsInParameterDefault>()
            .expect_no_corrections("def f(x = 0, y = 1); end\n");
    }
}

murphy_plugin_api::submit_cop!(SpaceAroundEqualsInParameterDefault);
