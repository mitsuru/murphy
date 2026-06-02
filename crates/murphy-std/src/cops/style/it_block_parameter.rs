//! `Style/ItBlockParameter` — checks for blocks where the `it` block
//! parameter (Ruby 3.4+) can or should be used.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ItBlockParameter
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered styles:
//!     - `allow_single_line` (default): flags single-line numblocks (`_1`) with USE_IT;
//!       flags multi-line itblocks with AVOID_MULTILINE (whole first-line range).
//!     - `only_numbered_parameters`: flags single-line numblocks (`_1`) with USE_IT only.
//!     - `always`: flags numblocks and single-arg named blocks with USE_IT.
//!     - `disallow`: flags each `it` lvar in itblocks with AVOID_IT.
//!   Autocorrect: numblock `_1` → `it` (replace lvar). No autocorrect for
//!   disallow/always named-block or multiline-avoid (matching RuboCop).
//!   `Enabled: pending` in default.yml.
//!   Ruby version gate: `it` blocks require Ruby 3.4+. Murphy does not
//!   enforce a target-ruby gate; documented here per NumberedParameters precedent.
//!   `find_block_variables` mirrors RuboCop: naive descendant lvar search
//!   (no scope awareness for nested blocks).
//! ```
//!
//! ## Style matrix
//!
//! | style | numblock(`_1`) | block(1 plain arg) | itblock |
//! |---|---|---|---|
//! | `allow_single_line` | flag each `_1` lvar → USE_IT | — | multi-line: flag first line → AVOID_MULTILINE |
//! | `only_numbered_parameters` | flag each `_1` lvar → USE_IT | — | — |
//! | `always` | flag each `_1` lvar → USE_IT | flag each named lvar → USE_IT | — |
//! | `disallow` | — | — | flag each `it` lvar → AVOID_IT |

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_USE_IT: &str = "Use `it` block parameter.";
const MSG_AVOID_IT: &str = "Avoid using `it` block parameter.";
const MSG_AVOID_IT_MULTILINE: &str = "Avoid using `it` block parameter for multi-line blocks.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ItBlockParameter;

/// Enforced style for the `it` block parameter cop.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Allow `it` only in single-line blocks; flag `_1` usage → USE_IT,
    /// flag multi-line itblocks → AVOID_MULTILINE. (Default.)
    #[default]
    #[option(value = "allow_single_line")]
    AllowSingleLine,
    /// Only flag numbered parameters (`_1`) → USE_IT.
    #[option(value = "only_numbered_parameters")]
    OnlyNumberedParameters,
    /// Always use `it`; also flag single-argument named blocks → USE_IT.
    #[option(value = "always")]
    Always,
    /// Disallow `it`; flag each `it` usage → AVOID_IT.
    #[option(value = "disallow")]
    Disallow,
}

/// Options for `Style/ItBlockParameter`.
#[derive(CopOptions, Debug)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "allow_single_line",
        description = "Control when the `it` block parameter may be used."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/ItBlockParameter",
    description = "Checks for blocks with one argument where `it` block parameter can be used.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl ItBlockParameter {
    /// Numbered-parameter block (`foo { _1 }`).
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if opts.enforced_style == EnforcedStyle::Disallow {
            return;
        }
        // Only _1 (max_n == 1) is flagged, matching RuboCop's `node.children[1] == 1`.
        if cx.numblock_max(node) != Some(1) {
            return;
        }
        let NodeKind::Numblock { body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        // Find all `_1` lvar descendants and flag each one.
        let sym_1 = find_lvar_sym(cx, body_id, "_1");
        for lvar_id in sym_1 {
            cx.emit_offense(cx.range(lvar_id), MSG_USE_IT, None);
        }
    }

    /// `it`-parameter block (`foo { it }`, Ruby 3.4+).
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        match opts.enforced_style {
            EnforcedStyle::AllowSingleLine if cx.is_multiline(node) => {
                // Offense on the first line of the block only.
                let range = first_line_range(cx.range(node), cx.source().as_bytes());
                cx.emit_offense(range, MSG_AVOID_IT_MULTILINE, None);
            }
            EnforcedStyle::Disallow => {
                let NodeKind::Itblock { body, .. } = *cx.kind(node) else {
                    return;
                };
                let Some(body_id) = body.get() else {
                    return;
                };
                let lvars = find_lvar_sym(cx, body_id, "it");
                for lvar_id in lvars {
                    cx.emit_offense(cx.range(lvar_id), MSG_AVOID_IT, None);
                }
            }
            // only_numbered_parameters / always: no offense for itblocks
            _ => {}
        }
    }

    /// Ordinary block with one named argument (`foo { |x| ... }`).
    /// Only flagged under the `always` style.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if opts.enforced_style != EnforcedStyle::Always {
            return;
        }
        // Must have exactly one plain `arg` argument.
        let Some(args_id) = cx.block_arguments(node).get() else {
            return;
        };
        let NodeKind::Args(list) = *cx.kind(args_id) else {
            return;
        };
        let args = cx.list(list);
        if args.len() != 1 {
            return;
        }
        let NodeKind::Arg(name_sym) = *cx.kind(args[0]) else {
            return;
        };
        let name = cx.symbol_str(name_sym);
        let Some(body_id) = cx.block_body(node).get() else {
            return;
        };
        let lvars = find_lvar_sym(cx, body_id, name);
        for lvar_id in lvars {
            cx.emit_offense(cx.range(lvar_id), MSG_USE_IT, None);
        }
    }
}

/// Find all `Lvar` nodes in the subtree of `body` whose symbol text equals `target`.
fn find_lvar_sym<'a>(cx: &'a Cx<'a>, body: NodeId, target: &str) -> Vec<NodeId> {
    let mut result = Vec::new();
    // Include body itself in case it is directly an lvar.
    let candidates = std::iter::once(body).chain(cx.descendants(body));
    for id in candidates {
        if let NodeKind::Lvar(sym) = *cx.kind(id)
            && cx.symbol_str(sym) == target
        {
            result.push(id);
        }
    }
    result
}

/// Restrict `range` to the first line.
fn first_line_range(range: Range, source: &[u8]) -> Range {
    let start = range.start as usize;
    let end = range.end as usize;
    let line_end = source[start..end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|pos| start + pos)
        .unwrap_or(end);
    Range { start: range.start, end: line_end as u32 }
}


#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, ItBlockParameter, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(style: EnforcedStyle) -> Options {
        Options { enforced_style: style }
    }

    // ──────────────────────────────────────────────────────────
    // allow_single_line (default)
    // ──────────────────────────────────────────────────────────

    #[test]
    fn allow_single_line_flags_numblock_single_line() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::AllowSingleLine))
            .expect_offense(indoc! {"
                block { do_something(_1) }
                                     ^^ Use `it` block parameter.
            "});
    }

    #[test]
    fn allow_single_line_accepts_itblock_single_line() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::AllowSingleLine))
            .expect_no_offenses("block { do_something(it) }\n");
    }

    #[test]
    fn allow_single_line_flags_itblock_multiline() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::AllowSingleLine))
            .expect_offense(indoc! {"
                block do
                ^^^^^^^^ Avoid using `it` block parameter for multi-line blocks.
                  do_something(it)
                end
            "});
    }

    #[test]
    fn allow_single_line_accepts_named_param_block() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::AllowSingleLine))
            .expect_no_offenses("block { |named_param| do_something(named_param) }\n");
    }

    // ──────────────────────────────────────────────────────────
    // only_numbered_parameters
    // ──────────────────────────────────────────────────────────

    #[test]
    fn only_numbered_flags_numblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::OnlyNumberedParameters))
            .expect_offense(indoc! {"
                block { do_something(_1) }
                                     ^^ Use `it` block parameter.
            "});
    }

    #[test]
    fn only_numbered_accepts_itblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::OnlyNumberedParameters))
            .expect_no_offenses("block { do_something(it) }\n");
    }

    #[test]
    fn only_numbered_accepts_named_block() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::OnlyNumberedParameters))
            .expect_no_offenses("block { |x| do_something(x) }\n");
    }

    // ──────────────────────────────────────────────────────────
    // always
    // ──────────────────────────────────────────────────────────

    #[test]
    fn always_flags_numblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Always))
            .expect_offense(indoc! {"
                block { do_something(_1) }
                                     ^^ Use `it` block parameter.
            "});
    }

    #[test]
    fn always_flags_named_single_arg_block() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Always))
            .expect_offense(indoc! {"
                block { |named_param| do_something(named_param) }
                                                   ^^^^^^^^^^^ Use `it` block parameter.
            "});
    }

    #[test]
    fn always_accepts_itblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Always))
            .expect_no_offenses("block { do_something(it) }\n");
    }

    #[test]
    fn always_accepts_multi_arg_block() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Always))
            .expect_no_offenses("block { |a, b| do_something(a, b) }\n");
    }

    // ──────────────────────────────────────────────────────────
    // disallow
    // ──────────────────────────────────────────────────────────

    #[test]
    fn disallow_flags_itblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Disallow))
            .expect_offense(indoc! {"
                block { do_something(it) }
                                     ^^ Avoid using `it` block parameter.
            "});
    }

    #[test]
    fn disallow_accepts_numblock() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Disallow))
            .expect_no_offenses("block { do_something(_1) }\n");
    }

    #[test]
    fn disallow_accepts_named_block() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::Disallow))
            .expect_no_offenses("block { |x| do_something(x) }\n");
    }

    // ──────────────────────────────────────────────────────────
    // numblock with max_n > 1 — not flagged
    // ──────────────────────────────────────────────────────────

    #[test]
    fn numblock_max_n_gt_1_not_flagged() {
        test::<ItBlockParameter>()
            .with_options(&opts(EnforcedStyle::AllowSingleLine))
            .expect_no_offenses("block { do_something(_1, _2) }\n");
    }

    // ──────────────────────────────────────────────────────────
    // options parsing
    // ──────────────────────────────────────────────────────────

    #[test]
    fn options_parse_error_not_an_object() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(b"[]")
            .expect_err("array root should be invalid");
        assert_eq!(err.kind(), &ConfigErrorKind::NotAnObject);
    }
}
murphy_plugin_api::submit_cop!(ItBlockParameter);
