//! `Lint/AmbiguousRange` — flags ranges with ambiguous boundaries.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousRange
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags range boundaries that are complex enough to read ambiguously
//!   (binary operations, calls on basic-literal receivers, operator method
//!   calls other than `[]`) and wraps them in parentheses. `unary_operation?`
//!   is reimplemented as `operator_method? && selector starts at expression`.
//!   Bare identifiers parse as nil-receiver sends in Murphy (not `lvar`), so
//!   they are accepted through the `receiver.nil?` branch of `acceptable_call?`,
//!   matching RuboCop's behavior for variable boundaries.
//! ```
//!
//! ## Matched shapes
//! - `x || 1..2` → `(x || 1)..2` — boundary is a binary operation
//! - `x - 1..2` → `(x - 1)..2`
//! - `1..2.to_a` → `1..(2.to_a)` — call on a basic-literal receiver
//!
//! ## Accepted shapes (no offense)
//! - `1..2`, `'a'..'z'`, `:foo..:bar` — literal boundaries
//! - `a..b`, `@a..@b`, `MIN..MAX` — variable / constant boundaries
//! - `-a..b` — unary operations
//! - `a.foo..b.bar` — method chains, unless `RequireParenthesesForMethodChains`

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Wrap complex range boundaries with parentheses to avoid ambiguity.";

#[derive(Default)]
pub struct AmbiguousRange;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "RequireParenthesesForMethodChains",
        default = false,
        description = "When true, require parentheses around method chains used as range boundaries."
    )]
    pub require_parentheses_for_method_chains: bool,
}

#[cop(
    name = "Lint/AmbiguousRange",
    description = "Checks for ranges with ambiguous boundaries.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl AmbiguousRange {
    #[on_node(kind = "range")]
    fn check_range(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::RangeExpr { begin_, end_, .. } = *cx.kind(node) else {
            return;
        };
        let opts = cx.options_or_default::<Options>();

        for boundary in [begin_.get(), end_.get()].into_iter().flatten() {
            if acceptable(boundary, &opts, cx) {
                continue;
            }
            cx.emit_offense(cx.range(boundary), MSG, None);
            // `corrector.wrap(boundary, '(', ')')` — two non-overlapping edits.
            let r = cx.range(boundary);
            cx.emit_edit(Range { start: r.start, end: r.start }, "(");
            cx.emit_edit(Range { start: r.end, end: r.end }, ")");
        }
    }
}

/// RuboCop's `acceptable?`:
/// `begin_type? || literal? || rational_literal? || variable? ||
///  const_type? || self_type? || (call_type? && acceptable_call?(node))`.
fn acceptable(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    // `begin_type?` — already parenthesized (`(...)`). `begin...end` blocks
    // are a separate kwbegin shape and must fall through.
    if crate::cops::util::is_parenthesized(node, cx) {
        return true;
    }
    if cx.is_literal(node) {
        return true;
    }
    if is_rational_literal(node, cx) {
        return true;
    }
    if cx.is_variable(node) {
        return true;
    }
    if matches!(*cx.kind(node), NodeKind::Const { .. }) {
        return true;
    }
    if matches!(*cx.kind(node), NodeKind::SelfExpr) {
        return true;
    }
    if matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return acceptable_call(node, opts, cx);
    }
    false
}

/// RuboCop's `acceptable_call?`:
/// ```ruby
/// return true if node.unary_operation?
/// return false if node.receiver&.basic_literal?
/// return false if node.operator_method? && !node.method?(:[])
/// require_parentheses_for_method_chain? || node.receiver.nil?
/// ```
fn acceptable_call(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if is_unary_operation(node, cx) {
        return true;
    }
    if let Some(recv) = cx.call_receiver(node).get()
        && cx.is_basic_literal(recv)
    {
        return false;
    }
    if cx.is_operator_method(node) && cx.method_name(node) != Some("[]") {
        return false;
    }
    // `require_parentheses_for_method_chain?` is `!cop_config[...]`.
    !opts.require_parentheses_for_method_chains || cx.call_receiver(node).get().is_none()
}

/// `unary_operation?` = `operator_method? && loc.expression.begin_pos ==
/// selector.begin_pos` — the operator selector sits at the very start of the
/// expression (e.g. `-a`, `!b`).
fn is_unary_operation(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_operator_method(node) && cx.loc(node).name.start == cx.range(node).start
}

/// RuboCop's `rational_literal?`: `(send (int _) :/ (rational _))`, e.g.
/// `1/10r`.
fn is_rational_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("/") {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(recv), NodeKind::Int(..)) {
        return false;
    }
    match cx.call_arguments(node) {
        [arg] => matches!(*cx.kind(*arg), NodeKind::Rational(..)),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(AmbiguousRange);

#[cfg(test)]
mod tests {
    use super::{AmbiguousRange, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_binary_operation_begin_boundary() {
        test::<AmbiguousRange>()
            .expect_offense(indoc! {r#"
                x || 1..2
                ^^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    x || 1..2
                    ^^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
                "#},
                "(x || 1)..2\n",
            );
    }

    #[test]
    fn flags_subtraction_begin_boundary() {
        test::<AmbiguousRange>()
            .expect_offense(indoc! {r#"
                x - 1..2
                ^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    x - 1..2
                    ^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
                "#},
                "(x - 1)..2\n",
            );
    }

    #[test]
    fn flags_call_on_literal_receiver_end_boundary() {
        test::<AmbiguousRange>()
            .expect_offense(indoc! {r#"
                1..2.to_a
                   ^^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    1..2.to_a
                       ^^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
                "#},
                "1..(2.to_a)\n",
            );
    }

    #[test]
    fn accepts_literal_range() {
        test::<AmbiguousRange>().expect_no_offenses("1..2\n");
    }

    #[test]
    fn accepts_string_literal_range() {
        test::<AmbiguousRange>().expect_no_offenses("'a'..'z'\n");
    }

    #[test]
    fn accepts_symbol_range() {
        test::<AmbiguousRange>().expect_no_offenses(":bar..:baz\n");
    }

    #[test]
    fn accepts_bare_variable_range() {
        test::<AmbiguousRange>().expect_no_offenses("a..b\n");
    }

    #[test]
    fn accepts_instance_variable_range() {
        test::<AmbiguousRange>().expect_no_offenses("@a..@b\n");
    }

    #[test]
    fn accepts_constant_range() {
        test::<AmbiguousRange>().expect_no_offenses("MyClass::MIN..MyClass::MAX\n");
    }

    #[test]
    fn accepts_unary_minus_operation_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("-a..10\n");
    }

    #[test]
    fn accepts_unary_plus_operation_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("+a..10\n");
    }

    #[test]
    fn accepts_rational_literal_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("1/10r..1/3r\n");
    }

    #[test]
    fn accepts_self_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("self..42\n");
    }

    #[test]
    fn accepts_element_reference_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("x[1]..2\n");
    }

    #[test]
    fn accepts_string_interpolation_literal_boundary() {
        test::<AmbiguousRange>().expect_no_offenses("\"#{foo}-#{bar}\"..'123-4567'\n");
    }

    #[test]
    fn accepts_method_chain_by_default() {
        test::<AmbiguousRange>().expect_no_offenses("a.foo..b.bar\n");
    }

    #[test]
    fn accepts_index_call_boundary() {
        // `[]` is the one operator method call that is acceptable.
        test::<AmbiguousRange>().expect_no_offenses("a[1]..b[2]\n");
    }

    #[test]
    fn flags_method_chain_when_option_enabled() {
        let opts = Options { require_parentheses_for_method_chains: true };
        test::<AmbiguousRange>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                a.foo..b.bar
                ^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
                       ^^^^^ Wrap complex range boundaries with parentheses to avoid ambiguity.
            "#});
    }
}
