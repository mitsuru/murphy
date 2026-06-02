//! `Style/EmptyStringInsideInterpolation` — avoid empty strings in interpolation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyStringInsideInterpolation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements both `trailing_conditional` (default) and `ternary` styles.
//!
//!   `trailing_conditional` mode flags ternaries inside interpolation whose
//!   if-branch or else-branch is an empty string, and autocorrects to a
//!   modifier `if`/`unless` form:
//!   - `"#{a ? 'foo' : ''}"` → `"#{'foo' if a}"`
//!   - `"#{a ? '' : 'foo'}"` → `"#{'foo' unless a}"`
//!
//!   `ternary` mode flags modifier-form `if`/`unless` inside interpolation
//!   that have a non-nil non-empty body, and autocorrects to ternary form:
//!   - `"#{'foo' if a}"` → `"#{a ? 'foo' : ''}"`
//!   - `"#{'foo' unless a}"` → `"#{a ? '' : 'foo'}"`
//!
//!   The cop is `Enabled: pending` upstream — `default_enabled = false` here.
//!   Empty branch is nil or an empty-string `Str` node.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

const MSG_TRAILING_CONDITIONAL: &str =
    "Do not use trailing conditionals in string interpolation.";
const MSG_TERNARY: &str = "Do not return empty strings in string interpolation.";

#[derive(Default)]
pub struct EmptyStringInsideInterpolation;

/// Enforced style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InterpolationStyle {
    #[default]
    #[option(value = "trailing_conditional")]
    TrailingConditional,
    #[option(value = "ternary")]
    Ternary,
}

#[derive(CopOptions)]
pub struct EmptyStringInsideInterpolationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "trailing_conditional",
        description = "Preferred style: `trailing_conditional` (default) or `ternary`."
    )]
    pub enforced_style: InterpolationStyle,
}

#[cop(
    name = "Style/EmptyStringInsideInterpolation",
    description = "Checks for empty strings being assigned inside string interpolation.",
    default_severity = "warning",
    default_enabled = false,
    options = EmptyStringInsideInterpolationOptions,
)]
impl EmptyStringInsideInterpolation {
    #[on_node(kind = "if")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        if !inside_interpolation(node, cx) {
            return;
        }
        let opts = cx.options_or_default::<EmptyStringInsideInterpolationOptions>();
        match opts.enforced_style {
            InterpolationStyle::TrailingConditional => check_trailing_conditional(node, cx),
            InterpolationStyle::Ternary => check_ternary(node, cx),
        }
    }
}

/// Returns `true` if `node` is inside a string interpolation (`begin` inside `dstr`).
fn inside_interpolation(node: NodeId, cx: &Cx<'_>) -> bool {
    // The `if` must be inside a `begin` node that is inside a `dstr`/`dsym`/`xstr`.
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Begin(_)) {
        return false;
    }
    let Some(grandparent) = cx.parent(parent).get() else {
        return false;
    };
    matches!(
        *cx.kind(grandparent),
        NodeKind::Dstr(_) | NodeKind::Dsym(_) | NodeKind::Xstr(_)
    )
}

/// Returns `true` if `opt_node` is nil (OptNodeId::NONE) or an empty `Str` node.
fn is_empty_branch(opt_node: OptNodeId, cx: &Cx<'_>) -> bool {
    match opt_node.get() {
        None => true,
        Some(n) => {
            if let NodeKind::Str(s) = *cx.kind(n) {
                cx.string_str(s).is_empty()
            } else {
                false
            }
        }
    }
}

/// Check in `trailing_conditional` mode: flag ternaries with empty if/else branch.
fn check_trailing_conditional(node: NodeId, cx: &Cx<'_>) {
    if !cx.is_ternary(node) {
        return;
    }
    let NodeKind::If {
        cond,
        then_: then_body,
        else_: else_body,
    } = *cx.kind(node)
    else {
        return;
    };

    if is_empty_branch(else_body, cx) && then_body.get().is_some() {
        // `a ? 'foo' : ''` → `'foo' if a`
        cx.emit_offense(cx.range(node), MSG_TERNARY, None);
        let outcome = then_body.get().unwrap();
        let outcome_src = cx.raw_source(cx.range(outcome));
        let cond_src = cx.raw_source(cx.range(cond));
        cx.emit_edit(cx.range(node), &format!("{} if {}", outcome_src, cond_src));
    } else if is_empty_branch(then_body, cx) && else_body.get().is_some() {
        // `a ? '' : 'foo'` → `'foo' unless a`
        cx.emit_offense(cx.range(node), MSG_TERNARY, None);
        let outcome = else_body.get().unwrap();
        let outcome_src = cx.raw_source(cx.range(outcome));
        let cond_src = cx.raw_source(cx.range(cond));
        cx.emit_edit(
            cx.range(node),
            &format!("{} unless {}", outcome_src, cond_src),
        );
    }
}

/// Check in `ternary` mode: flag modifier-form `if`/`unless` inside interpolation.
fn check_ternary(node: NodeId, cx: &Cx<'_>) {
    if !cx.is_modifier_form(node) {
        return;
    }
    let NodeKind::If {
        cond,
        then_: then_body,
        else_: else_body,
    } = *cx.kind(node)
    else {
        return;
    };

    let is_unless = cx.is_unless(node);

    if is_unless {
        // `'foo' unless cond` → `cond ? '' : 'foo'`
        // body is in the else slot for `unless`.
        let outcome_opt = else_body;
        let Some(outcome) = outcome_opt.get() else {
            return;
        };
        if is_empty_node(outcome, cx) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG_TRAILING_CONDITIONAL, None);
        let outcome_src = cx.raw_source(cx.range(outcome));
        let cond_src = cx.raw_source(cx.range(cond));
        cx.emit_edit(
            cx.range(node),
            &format!("{} ? '' : {}", cond_src, outcome_src),
        );
    } else {
        // `'foo' if cond` → `cond ? 'foo' : ''`
        let Some(outcome) = then_body.get() else {
            return;
        };
        if is_empty_node(outcome, cx) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG_TRAILING_CONDITIONAL, None);
        let outcome_src = cx.raw_source(cx.range(outcome));
        let cond_src = cx.raw_source(cx.range(cond));
        cx.emit_edit(
            cx.range(node),
            &format!("{} ? {} : ''", cond_src, outcome_src),
        );
    }
}

/// Returns `true` if `node` is an empty string node.
fn is_empty_node(node: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Str(s) = *cx.kind(node) {
        cx.string_str(s).is_empty()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{EmptyStringInsideInterpolation, EmptyStringInsideInterpolationOptions, InterpolationStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- trailing_conditional mode (default) ---

    #[test]
    fn flags_ternary_with_empty_else_in_interpolation() {
        test::<EmptyStringInsideInterpolation>().expect_offense(indoc! {r##"
            x = "#{a ? 'foo' : ''}"
                   ^^^^^^^^^^^^^^ Do not return empty strings in string interpolation.
        "##});
    }

    #[test]
    fn flags_ternary_with_empty_if_in_interpolation() {
        test::<EmptyStringInsideInterpolation>().expect_offense(indoc! {r##"
            x = "#{a ? '' : 'foo'}"
                   ^^^^^^^^^^^^^^ Do not return empty strings in string interpolation.
        "##});
    }

    #[test]
    fn autocorrects_ternary_empty_else_to_trailing_if() {
        test::<EmptyStringInsideInterpolation>().expect_correction(
            indoc! {r##"
                x = "#{a ? 'foo' : ''}"
                       ^^^^^^^^^^^^^^ Do not return empty strings in string interpolation.
            "##},
            "x = \"#{'foo' if a}\"\n",
        );
    }

    #[test]
    fn autocorrects_ternary_empty_if_to_trailing_unless() {
        test::<EmptyStringInsideInterpolation>().expect_correction(
            indoc! {r##"
                x = "#{a ? '' : 'foo'}"
                       ^^^^^^^^^^^^^^ Do not return empty strings in string interpolation.
            "##},
            "x = \"#{'foo' unless a}\"\n",
        );
    }

    #[test]
    fn accepts_non_empty_ternary_in_interpolation() {
        test::<EmptyStringInsideInterpolation>()
            .expect_no_offenses("x = \"#{a ? 'foo' : 'bar'}\"\n");
    }

    #[test]
    fn accepts_trailing_if_in_trailing_conditional_mode() {
        test::<EmptyStringInsideInterpolation>()
            .expect_no_offenses("x = \"#{'foo' if a}\"\n");
    }

    #[test]
    fn accepts_ternary_outside_interpolation() {
        test::<EmptyStringInsideInterpolation>()
            .expect_no_offenses("x = a ? 'foo' : ''\n");
    }

    // --- ternary mode ---

    #[test]
    fn flags_trailing_if_in_ternary_mode() {
        test::<EmptyStringInsideInterpolation>()
            .with_options(&EmptyStringInsideInterpolationOptions {
                enforced_style: InterpolationStyle::Ternary,
            })
            .expect_offense(indoc! {r##"
                x = "#{'foo' if a}"
                       ^^^^^^^^^^ Do not use trailing conditionals in string interpolation.
            "##});
    }

    #[test]
    fn autocorrects_trailing_if_to_ternary() {
        test::<EmptyStringInsideInterpolation>()
            .with_options(&EmptyStringInsideInterpolationOptions {
                enforced_style: InterpolationStyle::Ternary,
            })
            .expect_correction(
                indoc! {r##"
                    x = "#{'foo' if a}"
                           ^^^^^^^^^^ Do not use trailing conditionals in string interpolation.
                "##},
                "x = \"#{a ? 'foo' : ''}\"\n",
            );
    }

    #[test]
    fn flags_trailing_unless_in_ternary_mode() {
        test::<EmptyStringInsideInterpolation>()
            .with_options(&EmptyStringInsideInterpolationOptions {
                enforced_style: InterpolationStyle::Ternary,
            })
            .expect_offense(indoc! {r##"
                x = "#{'foo' unless a}"
                       ^^^^^^^^^^^^^^ Do not use trailing conditionals in string interpolation.
            "##});
    }

    #[test]
    fn accepts_ternary_in_ternary_mode() {
        test::<EmptyStringInsideInterpolation>()
            .with_options(&EmptyStringInsideInterpolationOptions {
                enforced_style: InterpolationStyle::Ternary,
            })
            .expect_no_offenses("x = \"#{a ? 'foo' : ''}\"\n");
    }
}

murphy_plugin_api::submit_cop!(EmptyStringInsideInterpolation);
