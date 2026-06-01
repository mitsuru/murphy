//! `Style/AndOr` — flags `and`/`or` keyword operators.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AndOr
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: conditionals (default) — flags `and`/`or` only inside
//!   if/unless/while/until/for conditions. EnforcedStyle: always — flags
//!   everywhere. Autocorrect replaces keyword with symbolic operator; skipped
//!   when parent is an assignment (OpAsgn/OrAsgn/AndAsgn/Lvasgn/Ivasgn etc.)
//!   to avoid silent precedence changes (RuboCop's safety annotation).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct AndOr;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "conditionals")]
    Conditionals,
    #[option(value = "always")]
    Always,
}

#[derive(CopOptions)]
pub struct AndOrOptions {
    #[option(
        name = "EnforcedStyle",
        default = "conditionals",
        description = "When set to `conditionals`, flags `and`/`or` only inside conditionals. When set to `always`, flags everywhere."
    )]
    pub enforced_style: EnforcedStyle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Keyword {
    And,
    Or,
}

impl Keyword {
    fn symbolic(self) -> &'static str {
        match self {
            Keyword::And => "&&",
            Keyword::Or => "||",
        }
    }

    fn text(self) -> &'static str {
        match self {
            Keyword::And => "and",
            Keyword::Or => "or",
        }
    }
}

#[cop(
    name = "Style/AndOr",
    description = "Use `&&` and `||` instead of `and` and `or`.",
    default_severity = "warning",
    default_enabled = true,
    options = AndOrOptions
)]
impl AndOr {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Always {
            flag_if_keyword(node, Keyword::And, cx);
        }
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Always {
            flag_if_keyword(node, Keyword::Or, cx);
        }
    }

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Conditionals {
            check_conditional_condition(node, cx);
        }
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Conditionals {
            check_conditional_condition(node, cx);
        }
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Conditionals {
            check_conditional_condition(node, cx);
        }
    }

    #[on_node(kind = "for")]
    fn check_for(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AndOrOptions>();
        if opts.enforced_style == EnforcedStyle::Conditionals {
            check_conditional_condition(node, cx);
        }
    }
}

fn check_conditional_condition(node: NodeId, cx: &Cx<'_>) {
    let cond_id = match cx.kind(node) {
        NodeKind::If { cond, .. } => *cond,
        NodeKind::While { cond, .. } => *cond,
        NodeKind::Until { cond, .. } => *cond,
        NodeKind::For { iter, .. } => *iter,
        _ => return,
    };
    check_operator_keywords_recursive(cond_id, cx);
}

fn check_operator_keywords_recursive(id: NodeId, cx: &Cx<'_>) {
    match cx.kind(id) {
        NodeKind::And { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            flag_if_keyword(id, Keyword::And, cx);
            check_operator_keywords_recursive(lhs, cx);
            check_operator_keywords_recursive(rhs, cx);
        }
        NodeKind::Or { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            flag_if_keyword(id, Keyword::Or, cx);
            check_operator_keywords_recursive(lhs, cx);
            check_operator_keywords_recursive(rhs, cx);
        }
        // Do not descend into nested conditionals — they are visited by
        // their own `on_node` dispatch.
        NodeKind::If { .. } | NodeKind::While { .. } | NodeKind::Until { .. } => {}
        _ => {
            for &child in cx.children(id).iter() {
                check_operator_keywords_recursive(child, cx);
            }
        }
    }
}

fn flag_if_keyword(id: NodeId, kw: Keyword, cx: &Cx<'_>) {
    let (lhs, rhs) = match cx.kind(id) {
        NodeKind::And { lhs, rhs } => (*lhs, *rhs),
        NodeKind::Or { lhs, rhs } => (*lhs, *rhs),
        _ => return,
    };

    let gap = Range {
        start: cx.range(lhs).end,
        end: cx.range(rhs).start,
    };
    let Some(op_range) = find_keyword_op_in_gap(cx, gap, kw.text()) else {
        return;
    };

    let msg = format!("Use `{}` instead of `{}`.", kw.symbolic(), kw.text());
    cx.emit_offense(op_range, &msg, None);

    if !parent_is_assignment(id, cx) {
        cx.emit_edit(op_range, kw.symbolic());
    }
}

fn find_keyword_op_in_gap(cx: &Cx<'_>, gap: Range, kw: &str) -> Option<Range> {
    if gap.start >= gap.end {
        return None;
    }
    let gap_text = cx.raw_source(gap);
    let gap_bytes = gap_text.as_bytes();
    let kw_bytes = kw.as_bytes();
    if kw_bytes.len() > gap_bytes.len() {
        return None;
    }
    let upper = gap_bytes.len() - kw_bytes.len() + 1;
    for i in 0..upper {
        if &gap_bytes[i..i + kw_bytes.len()] != kw_bytes {
            continue;
        }
        let before_ok = i == 0 || !is_word_char(gap_bytes[i - 1]);
        let after_ok =
            i + kw_bytes.len() >= gap_bytes.len() || !is_word_char(gap_bytes[i + kw_bytes.len()]);
        if before_ok && after_ok {
            return Some(Range {
                start: gap.start + i as u32,
                end: gap.start + (i + kw_bytes.len()) as u32,
            });
        }
    }
    None
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Returns `true` when the parent of `id` is an assignment node, OR when
/// one of the direct children of the `And`/`Or` is an assignment-like node.
///
/// This guards the autocorrect: replacing `and`/`or` with `&&`/`||`
/// changes operator precedence, which can silently break programs.
/// For example, `x = y and z` means `(x = y) and z` — the `&&` form
/// would need parentheses: `(x = y) && z`. The conservative approach
/// skips autocorrect whenever assignment is present in the expression.
///
/// Covers standard assignment nodes (lvasgn, ivasgn, gvasgn, cvasgn, casgn,
/// op_asgn, or_asgn, and_asgn, masgn) plus setter method calls such as
/// `self.foo = bar` (which parse as `Send` with a setter selector) to handle
/// the `self.foo = bar and baz` → `self.foo = (bar && baz)` precedence trap.
fn parent_is_assignment(id: NodeId, cx: &Cx<'_>) -> bool {
    let is_asgn = |node_id: NodeId| {
        matches!(
            cx.kind(node_id),
            NodeKind::Lvasgn { .. }
                | NodeKind::Ivasgn { .. }
                | NodeKind::Gvasgn { .. }
                | NodeKind::Cvasgn { .. }
                | NodeKind::Casgn { .. }
                | NodeKind::OpAsgn { .. }
                | NodeKind::OrAsgn { .. }
                | NodeKind::AndAsgn { .. }
                | NodeKind::Masgn { .. }
        ) || cx.is_setter_method(node_id)
    };

    // Check immediate parent.
    if cx.parent(id).get().is_some_and(is_asgn) {
        return true;
    }

    // Check direct children (lhs / rhs).
    match cx.kind(id) {
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => is_asgn(*lhs) || is_asgn(*rhs),
        _ => false,
    }
}

// Keep OptNodeId in scope so the `use` is load-bearing.
const _: () = {
    let _ = std::mem::size_of::<OptNodeId>();
};

#[cfg(test)]
mod tests {
    use super::{AndOr, AndOrOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_in_if_condition() {
        test::<AndOr>().expect_offense(indoc! {r#"
            if foo and bar
                   ^^^ Use `&&` instead of `and`.
            end
        "#});
    }

    #[test]
    fn flags_or_in_if_condition() {
        test::<AndOr>().expect_offense(indoc! {r#"
            if foo or bar
                   ^^ Use `||` instead of `or`.
            end
        "#});
    }

    #[test]
    fn flags_and_in_while_condition() {
        test::<AndOr>().expect_offense(indoc! {r#"
            while foo and bar
                      ^^^ Use `&&` instead of `and`.
            end
        "#});
    }

    #[test]
    fn flags_and_in_until_condition() {
        test::<AndOr>().expect_offense(indoc! {r#"
            until foo and bar
                      ^^^ Use `&&` instead of `and`.
            end
        "#});
    }

    #[test]
    fn accepts_symbolic_and_in_if() {
        test::<AndOr>().expect_no_offenses("if foo && bar\nend\n");
    }

    #[test]
    fn accepts_symbolic_or_in_if() {
        test::<AndOr>().expect_no_offenses("if foo || bar\nend\n");
    }

    #[test]
    fn accepts_keyword_and_outside_conditional() {
        test::<AndOr>().expect_no_offenses("foo.save and return\n");
    }

    #[test]
    fn accepts_keyword_or_outside_conditional() {
        test::<AndOr>().expect_no_offenses("foo.save or return\n");
    }

    #[test]
    fn corrects_and_to_symbolic_in_if() {
        test::<AndOr>().expect_correction(
            indoc! {r#"
                if foo and bar
                       ^^^ Use `&&` instead of `and`.
                end
            "#},
            "if foo && bar\nend\n",
        );
    }

    #[test]
    fn corrects_or_to_symbolic_in_if() {
        test::<AndOr>().expect_correction(
            indoc! {r#"
                if foo or bar
                       ^^ Use `||` instead of `or`.
                end
            "#},
            "if foo || bar\nend\n",
        );
    }

    #[test]
    fn always_flags_and_outside_conditional() {
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_offense(indoc! {r#"
                foo.save and return
                         ^^^ Use `&&` instead of `and`.
            "#});
    }

    #[test]
    fn always_flags_or_outside_conditional() {
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_offense(indoc! {r#"
                foo.save or return
                         ^^ Use `||` instead of `or`.
            "#});
    }

    #[test]
    fn always_flags_and_in_if() {
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_offense(indoc! {r#"
                if foo and bar
                       ^^^ Use `&&` instead of `and`.
                end
            "#});
    }

    #[test]
    fn always_accepts_symbolic_and() {
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_no_offenses("foo.save && return\n");
    }

    #[test]
    fn always_no_autocorrect_when_parent_is_assignment() {
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_no_corrections("x = y and z\n");
    }

    #[test]
    fn always_no_autocorrect_when_lhs_is_setter_method() {
        // `self.foo = bar and baz` — lhs is a setter Send; autocorrect
        // would silently change precedence to `self.foo = (bar && baz)`.
        test::<AndOr>()
            .with_options(&AndOrOptions {
                enforced_style: EnforcedStyle::Always,
            })
            .expect_no_corrections("self.foo = bar and baz\n");
    }

    #[test]
    fn always_style_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts =
            AndOrOptions::from_config_json(br#"{"EnforcedStyle": "always"}"#).expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::Always);
    }

    #[test]
    fn conditionals_style_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts =
            AndOrOptions::from_config_json(br#"{"EnforcedStyle": "conditionals"}"#).expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::Conditionals);
    }

    #[test]
    fn default_style_is_conditionals() {
        let opts = AndOrOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Conditionals);
    }
}
murphy_plugin_api::submit_cop!(AndOr);
