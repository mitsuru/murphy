//! `Lint/DisjunctiveAssignmentInConstructor` — flags a leading run of
//! instance-variable disjunctive assignments (`@x ||= value`) in an
//! `initialize` method, where plain assignment (`@x = value`) is enough.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DisjunctiveAssignmentInConstructor
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-ss0f
//! notes: >
//!   Ported from RuboCop Lint/DisjunctiveAssignmentInConstructor. Dispatches
//!   on `def` only (`def self.initialize` / defs is not checked, matching
//!   RuboCop's `on_def`). Walks the leading run of `or_asgn` statements in the
//!   method body and stops (`break`) at the first non-`or_asgn` line, exactly
//!   like RuboCop's `check_body_lines`. Only an instance-variable LHS
//!   (`@x ||= …`) is flagged — local/class/global-variable and constant
//!   targets are skipped. The offense range and autocorrect target the `||=`
//!   operator only, replacing it with `=`. Known divergence: RuboCop's
//!   `check_body` only descends into an *implicit* multi-statement body
//!   (parser `:begin`); an explicit `begin … end` body is `:kwbegin` and is
//!   skipped. Murphy lowers both to `NodeKind::Begin`, so a constructor whose
//!   body is an explicit `begin … end` block is descended into here and its
//!   leading `@x ||= …` is flagged where RuboCop would not (a false positive).
//!   This shape is contrived and not exercised by RuboCop's specs; tracked in
//!   murphy-ss0f.
//! ```
//!
//! ## Matched shapes
//!
//! - `def initialize; @x ||= value; …; end` — the offense fires on each
//!   `@x ||= value` in the **leading run** of `or_asgn` statements. The
//!   first statement that is not an `or_asgn` (a method call, `super`, a
//!   plain assignment, etc.) ends the run; assignments after it are not
//!   examined.
//!
//! ## Why this shape
//!
//! Instance variables are `nil` until assigned, so in a constructor the
//! disjunction in `@x ||= value` can never read a pre-existing value — it is
//! equivalent to `@x = value`. RuboCop only trusts this for the leading run
//! because any preceding statement (e.g. `super`) could already have set the
//! ivar.
//!
//! ## Autocorrect
//!
//! Replaces `||=` with `=`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct DisjunctiveAssignmentInConstructor;

const MSG: &str = "Unnecessary disjunctive assignment. Use plain assignment.";

#[cop(
    name = "Lint/DisjunctiveAssignmentInConstructor",
    description = "In constructor, plain assignment is preferred over disjunctive.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DisjunctiveAssignmentInConstructor {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop's `on_def` does not visit singleton method definitions
        // (`def self.initialize`). Murphy lowers `def self.foo` to a `Def`
        // node carrying a `self` receiver rather than a distinct `Defs`
        // node, so the singleton case is excluded by the receiver check.
        if cx.def_receiver(node).get().is_some() {
            return;
        }
        if cx.method_name(node) != Some("initialize") {
            return;
        }
        let Some(body) = cx.def_body(node).get() else {
            return;
        };
        check_body(body, cx);
    }
}

/// Walk the method body's leading statements. A multi-statement body is a
/// `Begin`; a single-statement body is the statement node itself.
fn check_body(body: NodeId, cx: &Cx<'_>) {
    match *cx.kind(body) {
        NodeKind::Begin(list) => check_body_lines(cx.list(list), cx),
        _ => check_body_lines(&[body], cx),
    }
}

/// Flag the **leading run** of `or_asgn` statements; stop at the first line
/// that is not an `or_asgn` (mirrors RuboCop's `break`).
fn check_body_lines(lines: &[NodeId], cx: &Cx<'_>) {
    for &line in lines {
        let NodeKind::OrAsgn { target, value } = *cx.kind(line) else {
            break;
        };
        check_disjunctive_assignment(line, target, value, cx);
    }
}

/// Flag a single `@x ||= value`. Only an instance-variable LHS qualifies.
fn check_disjunctive_assignment(node: NodeId, target: NodeId, value: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(target), NodeKind::Ivasgn { .. }) {
        return;
    }
    let gap = Range {
        start: cx.range(target).end,
        end: cx.range(value).start,
    };
    let Some(op_range) = find_op_in_gap(cx, gap, "||=") else {
        // Defensive: if the operator token can't be located (unexpected
        // shape), fall back to the whole node range so the offense is not
        // silently dropped.
        cx.emit_offense(cx.range(node), MSG, None);
        return;
    };
    cx.emit_offense(op_range, MSG, None);
    cx.emit_edit(op_range, "=");
}

/// Finds `op` in the gap text, returning its byte range.
fn find_op_in_gap(cx: &Cx<'_>, gap: Range, op: &str) -> Option<Range> {
    if gap.start >= gap.end {
        return None;
    }
    let gap_text = cx.raw_source(gap);
    let pos = gap_text.find(op)?;
    Some(Range {
        start: gap.start + pos as u32,
        end: gap.start + (pos + op.len()) as u32,
    })
}

murphy_plugin_api::submit_cop!(DisjunctiveAssignmentInConstructor);

#[cfg(test)]
mod tests {
    use super::DisjunctiveAssignmentInConstructor;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_instance_variable_disjunctive_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_offense(indoc! {r#"
            class Banana
              def initialize
                @delicious ||= true
                           ^^^ Unnecessary disjunctive assignment. Use plain assignment.
              end
            end
        "#});
    }

    #[test]
    fn autocorrects_to_plain_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_correction(
            indoc! {r#"
                class Banana
                  def initialize
                    @delicious ||= true
                               ^^^ Unnecessary disjunctive assignment. Use plain assignment.
                  end
                end
            "#},
            indoc! {r#"
                class Banana
                  def initialize
                    @delicious = true
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_when_super_follows() {
        // `super` after the `||=` does not save it: the leading run starts
        // with the `or_asgn`, which is flagged before the loop breaks on
        // `super`.
        test::<DisjunctiveAssignmentInConstructor>().expect_offense(indoc! {r#"
            class Banana
              def initialize
                @delicious ||= true
                           ^^^ Unnecessary disjunctive assignment. Use plain assignment.
                super
              end
            end
        "#});
    }

    #[test]
    fn flags_each_leading_disjunctive_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_offense(indoc! {r#"
            class Banana
              def initialize
                @a ||= 1
                   ^^^ Unnecessary disjunctive assignment. Use plain assignment.
                @b ||= 2
                   ^^^ Unnecessary disjunctive assignment. Use plain assignment.
              end
            end
        "#});
    }

    #[test]
    fn stops_at_first_non_disjunctive_statement() {
        // The `absolutely_any_method` call breaks the leading run, so the
        // `@b ||= 2` after it is never examined.
        test::<DisjunctiveAssignmentInConstructor>().expect_offense(indoc! {r#"
            class Banana
              def initialize
                @a ||= 1
                   ^^^ Unnecessary disjunctive assignment. Use plain assignment.
                absolutely_any_method
                @b ||= 2
              end
            end
        "#});
    }

    // --- no offenses ---

    #[test]
    fn ignores_empty_constructor() {
        test::<DisjunctiveAssignmentInConstructor>()
            .expect_no_offenses("class Banana\n  def initialize\n  end\nend\n");
    }

    #[test]
    fn ignores_plain_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def initialize
                @delicious = true
              end
            end
        "#});
    }

    #[test]
    fn ignores_local_variable_disjunctive_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def initialize
                delicious ||= true
              end
            end
        "#});
    }

    #[test]
    fn ignores_when_method_call_precedes() {
        // The leading run is empty: the first statement is a method call,
        // which breaks the loop immediately.
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def initialize
                absolutely_any_method
                @delicious ||= true
              end
            end
        "#});
    }

    #[test]
    fn ignores_when_super_precedes() {
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def initialize
                super
                @delicious ||= true
              end
            end
        "#});
    }

    #[test]
    fn ignores_class_variable_disjunctive_assignment() {
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def initialize
                @@delicious ||= true
              end
            end
        "#});
    }

    #[test]
    fn ignores_non_initialize_method() {
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def setup
                @delicious ||= true
              end
            end
        "#});
    }

    #[test]
    fn ignores_singleton_initialize() {
        // `def self.initialize` is a `Defs` node — RuboCop's `on_def` does
        // not visit it.
        test::<DisjunctiveAssignmentInConstructor>().expect_no_offenses(indoc! {r#"
            class Banana
              def self.initialize
                @delicious ||= true
              end
            end
        "#});
    }
}
