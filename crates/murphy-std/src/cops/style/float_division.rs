//! `Style/FloatDivision` -- enforce coercing one side only in float division.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FloatDivision
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: single_coerce (default), left_coerce, right_coerce, fdiv.
//!   Detection:
//!     - `single_coerce`: fires when both sides have `.to_f` (bad: a.to_f / b.to_f)
//!     - `left_coerce`: fires when only the right side has `.to_f` (bad: a / b.to_f)
//!     - `right_coerce`: fires when only the left side has `.to_f` (bad: a.to_f / b)
//!     - `fdiv`: fires when any side has `.to_f`
//!   Autocorrect: implemented for all four styles.
//!   Gaps vs RuboCop:
//!     - RuboCop's `regexp_last_match?` guard (allowing `.to_f` on Regexp.last_match
//!       or nth-refs like $1) is not implemented. Minor: affects edge cases only.
//!     - This cop is unsafe (removing `.to_f` from a string variable causes TypeError).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (single_coerce mode, default)
//! a.to_f / b.to_f
//!
//! # good
//! a.to_f / b
//! a / b.to_f
//! a.fdiv(b)
//! ```
//!
//! ## Autocorrect
//!
//! - `single_coerce`: removes `.to_f` from one side (right side preferred).
//! - `left_coerce`: adds `.to_f` to left, removes from right.
//! - `right_coerce`: removes from left, adds to right.
//! - `fdiv`: rewrites `a.to_f / b` or `a / b.to_f` as `a.fdiv(b)`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FloatDivision;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FloatDivisionStyle {
    /// Prefer coercing one side only (default). Flags when both sides have `.to_f`.
    #[default]
    #[option(value = "single_coerce")]
    SingleCoerce,
    /// Prefer coercing the left side. Flags when only the right has `.to_f`.
    #[option(value = "left_coerce")]
    LeftCoerce,
    /// Prefer coercing the right side. Flags when only the left has `.to_f`.
    #[option(value = "right_coerce")]
    RightCoerce,
    /// Prefer `fdiv`. Flags when either side has `.to_f`.
    #[option(value = "fdiv")]
    Fdiv,
}

#[derive(CopOptions)]
pub struct FloatDivisionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "single_coerce",
        description = "Preferred float division style."
    )]
    pub enforced_style: FloatDivisionStyle,
}

const MSG_SINGLE_COERCE: &str = "Prefer using `.to_f` on one side only.";
const MSG_LEFT_COERCE: &str = "Prefer using `.to_f` on the left side.";
const MSG_RIGHT_COERCE: &str = "Prefer using `.to_f` on the right side.";
const MSG_FDIV: &str = "Prefer using `fdiv` for float divisions.";

#[cop(
    name = "Style/FloatDivision",
    description = "For performing float division, coerce one side only.",
    default_severity = "warning",
    default_enabled = true,
    options = FloatDivisionOptions,
)]
impl FloatDivision {
    #[on_node(kind = "send", methods = ["/"])]
    fn check_division(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns true if `node` is `<expr>.to_f` (a send with method `to_f` and a non-nil receiver).
fn is_to_f(node: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Send { receiver, method, .. } = *cx.kind(node)
        && cx.symbol_str(method) == "to_f"
        && receiver.get().is_some()
    {
        return true;
    }
    false
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<FloatDivisionOptions>();

    // Get receiver (left side) and first arg (right side) of `/`.
    let lhs = match cx.call_receiver(node).get() {
        Some(n) => n,
        None => return,
    };
    let args = cx.call_arguments(node);
    let rhs = match args.first() {
        Some(&n) => n,
        None => return,
    };

    let lhs_is_to_f = is_to_f(lhs, cx);
    let rhs_is_to_f = is_to_f(rhs, cx);

    let offense = match opts.enforced_style {
        FloatDivisionStyle::SingleCoerce => lhs_is_to_f && rhs_is_to_f,
        FloatDivisionStyle::LeftCoerce => rhs_is_to_f && !lhs_is_to_f,
        FloatDivisionStyle::RightCoerce => lhs_is_to_f && !rhs_is_to_f,
        FloatDivisionStyle::Fdiv => lhs_is_to_f || rhs_is_to_f,
    };

    if !offense {
        return;
    }

    let msg = match opts.enforced_style {
        FloatDivisionStyle::SingleCoerce => MSG_SINGLE_COERCE,
        FloatDivisionStyle::LeftCoerce => MSG_LEFT_COERCE,
        FloatDivisionStyle::RightCoerce => MSG_RIGHT_COERCE,
        FloatDivisionStyle::Fdiv => MSG_FDIV,
    };

    cx.emit_offense(cx.range(node), msg, None);
    autocorrect(node, lhs, rhs, lhs_is_to_f, rhs_is_to_f, opts.enforced_style, cx);
}

/// Get the receiver of a `.to_f` call (the inner expression before `.to_f`).
fn to_f_receiver(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if let NodeKind::Send { receiver, method, .. } = *cx.kind(node)
        && cx.symbol_str(method) == "to_f"
    {
        return receiver.get();
    }
    None
}

fn autocorrect(
    node: NodeId,
    lhs: NodeId,
    rhs: NodeId,
    lhs_is_to_f: bool,
    rhs_is_to_f: bool,
    style: FloatDivisionStyle,
    cx: &Cx<'_>,
) {
    match style {
        FloatDivisionStyle::SingleCoerce => {
            // Remove .to_f from the right side (preferred side to keep one).
            if rhs_is_to_f {
                remove_to_f(rhs, cx);
            } else if lhs_is_to_f {
                remove_to_f(lhs, cx);
            }
        }
        FloatDivisionStyle::LeftCoerce => {
            // Add .to_f to left if it doesn't have it; remove from right.
            if !lhs_is_to_f {
                add_to_f(lhs, cx);
            }
            if rhs_is_to_f {
                remove_to_f(rhs, cx);
            }
        }
        FloatDivisionStyle::RightCoerce => {
            // Remove from left; add to right if it doesn't have it.
            if lhs_is_to_f {
                remove_to_f(lhs, cx);
            }
            if !rhs_is_to_f {
                add_to_f(rhs, cx);
            }
        }
        FloatDivisionStyle::Fdiv => {
            // Rewrite `a.to_f / b.to_f` (or mixed) as `a.fdiv(b)`.
            let lhs_src = if lhs_is_to_f {
                to_f_receiver(lhs, cx)
                    .map(|r| cx.raw_source(cx.range(r)).to_string())
                    .unwrap_or_else(|| cx.raw_source(cx.range(lhs)).to_string())
            } else {
                cx.raw_source(cx.range(lhs)).to_string()
            };
            let rhs_src = if rhs_is_to_f {
                to_f_receiver(rhs, cx)
                    .map(|r| cx.raw_source(cx.range(r)).to_string())
                    .unwrap_or_else(|| cx.raw_source(cx.range(rhs)).to_string())
            } else {
                cx.raw_source(cx.range(rhs)).to_string()
            };
            let replacement = format!("{lhs_src}.fdiv({rhs_src})");
            cx.emit_edit(cx.range(node), &replacement);
        }
    }
}

/// Remove the `.to_f` selector and dot from a `recv.to_f` node.
fn remove_to_f(node: NodeId, cx: &Cx<'_>) {
    // The node is `recv.to_f`. We want to delete `.to_f` (dot + selector).
    // Strategy: the node range ends at the end of `to_f`, and the receiver
    // range ends just before the dot. Delete from receiver end to node end.
    if let Some(recv) = to_f_receiver(node, cx) {
        let recv_end = cx.range(recv).end;
        let node_end = cx.range(node).end;
        let dot_to_end = Range {
            start: recv_end,
            end: node_end,
        };
        cx.emit_edit(dot_to_end, "");
    }
}

/// Append `.to_f` after a node.
fn add_to_f(node: NodeId, cx: &Cx<'_>) {
    let end = cx.range(node).end;
    let insert_range = Range { start: end, end };
    cx.emit_edit(insert_range, ".to_f");
}

#[cfg(test)]
mod tests {
    use super::{FloatDivision, FloatDivisionOptions, FloatDivisionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- single_coerce mode (default) ---

    #[test]
    fn no_offense_integer_division() {
        test::<FloatDivision>().expect_no_offenses("a / b\n");
    }

    #[test]
    fn no_offense_one_side_to_f_single_coerce() {
        test::<FloatDivision>().expect_no_offenses("a.to_f / b\n");
        test::<FloatDivision>().expect_no_offenses("a / b.to_f\n");
    }

    #[test]
    fn flags_both_sides_to_f_single_coerce() {
        test::<FloatDivision>().expect_offense(indoc! {r#"
            a.to_f / b.to_f
            ^^^^^^^^^^^^^^^ Prefer using `.to_f` on one side only.
        "#});
    }

    #[test]
    fn corrects_both_to_f_in_single_coerce_mode() {
        test::<FloatDivision>().expect_correction(
            indoc! {r#"
                a.to_f / b.to_f
                ^^^^^^^^^^^^^^^ Prefer using `.to_f` on one side only.
            "#},
            "a.to_f / b\n",
        );
    }

    // --- left_coerce mode ---

    #[test]
    fn no_offense_left_has_to_f_in_left_coerce_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::LeftCoerce,
            })
            .expect_no_offenses("a.to_f / b\n");
    }

    #[test]
    fn flags_right_only_to_f_in_left_coerce_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::LeftCoerce,
            })
            .expect_offense(indoc! {r#"
                a / b.to_f
                ^^^^^^^^^^ Prefer using `.to_f` on the left side.
            "#});
    }

    #[test]
    fn corrects_right_to_f_to_left_coerce() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::LeftCoerce,
            })
            .expect_correction(
                indoc! {r#"
                    a / b.to_f
                    ^^^^^^^^^^ Prefer using `.to_f` on the left side.
                "#},
                "a.to_f / b\n",
            );
    }

    // --- right_coerce mode ---

    #[test]
    fn no_offense_right_has_to_f_in_right_coerce_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::RightCoerce,
            })
            .expect_no_offenses("a / b.to_f\n");
    }

    #[test]
    fn flags_left_only_to_f_in_right_coerce_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::RightCoerce,
            })
            .expect_offense(indoc! {r#"
                a.to_f / b
                ^^^^^^^^^^ Prefer using `.to_f` on the right side.
            "#});
    }

    #[test]
    fn corrects_left_to_f_to_right_coerce() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::RightCoerce,
            })
            .expect_correction(
                indoc! {r#"
                    a.to_f / b
                    ^^^^^^^^^^ Prefer using `.to_f` on the right side.
                "#},
                "a / b.to_f\n",
            );
    }

    // --- fdiv mode ---

    #[test]
    fn no_offense_fdiv_already_used() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_no_offenses("a.fdiv(b)\n");
    }

    #[test]
    fn flags_left_to_f_in_fdiv_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_offense(indoc! {r#"
                a.to_f / b
                ^^^^^^^^^^ Prefer using `fdiv` for float divisions.
            "#});
    }

    #[test]
    fn flags_right_to_f_in_fdiv_mode() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_offense(indoc! {r#"
                a / b.to_f
                ^^^^^^^^^^ Prefer using `fdiv` for float divisions.
            "#});
    }

    #[test]
    fn corrects_to_fdiv_from_left_to_f() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_correction(
                indoc! {r#"
                    a.to_f / b
                    ^^^^^^^^^^ Prefer using `fdiv` for float divisions.
                "#},
                "a.fdiv(b)\n",
            );
    }

    #[test]
    fn corrects_to_fdiv_from_right_to_f() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_correction(
                indoc! {r#"
                    a / b.to_f
                    ^^^^^^^^^^ Prefer using `fdiv` for float divisions.
                "#},
                "a.fdiv(b)\n",
            );
    }

    #[test]
    fn corrects_to_fdiv_from_both_to_f() {
        test::<FloatDivision>()
            .with_options(&FloatDivisionOptions {
                enforced_style: FloatDivisionStyle::Fdiv,
            })
            .expect_correction(
                indoc! {r#"
                    a.to_f / b.to_f
                    ^^^^^^^^^^^^^^^ Prefer using `fdiv` for float divisions.
                "#},
                "a.fdiv(b)\n",
            );
    }

    // --- no crash on plain integer division ---

    #[test]
    fn no_crash_on_integer_literal_division() {
        test::<FloatDivision>().expect_no_offenses("1 / 2\n");
    }
}

murphy_plugin_api::submit_cop!(FloatDivision);
