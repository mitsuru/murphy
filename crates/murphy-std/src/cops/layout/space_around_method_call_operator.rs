//! `Layout/SpaceAroundMethodCallOperator` — flags spaces around a method-call
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundMethodCallOperator
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-o89j
//! notes: >
//!   Ports RuboCop's three checks: space before `.`/`&.`, space after
//!   `.`/`&.`, and space after `::` (constant path). Only gaps that are
//!   ENTIRELY spaces/tabs are flagged — a gap containing a newline
//!   (multiline chain `a\n  .b`) is accepted, matching RuboCop's
//!   `SPACES_REGEXP = /\A[ \t]+\z/`. Autocorrect removes the gap.
//!
//!   The `node.method?(:call) && !node.loc.selector` branch (`a.()`
//!   sugar where the selector is absent) is approximated: Murphy lowers
//!   `a.()` to a `Send` with method `call`. When `loc.name` is unset the
//!   after-dot gap is measured to the first token after the dot (the
//!   `(`), so `a. ()` is flagged. See the `call_sugar` tests.
//! ```
//!
//! operator (`.`, `&.`, `::`). Mirrors RuboCop's same-named cop:
//! `foo. bar` / `foo .bar` / `Foo:: Bar` are all flagged.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAroundMethodCallOperator;

#[cop(
    name = "Layout/SpaceAroundMethodCallOperator",
    description = "Checks method call operators to not have spaces around them.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpaceAroundMethodCallOperator {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check_space_after_double_colon(node, cx);
    }
}

/// RuboCop's `on_send`/`on_csend`: only dot (`.`) or safe-navigation (`&.`)
/// calls. A `::`-operator send is handled by the `on_const` path instead.
fn check_call(node: NodeId, cx: &Cx<'_>) {
    let dot = cx.loc(node).dot();
    if dot == Range::ZERO {
        return;
    }
    // RuboCop: `return unless node.dot? || node.safe_navigation?`. The dot
    // location can also be `::`; gate explicitly so constant-path sends
    // (`Foo::bar`) are excluded here (RuboCop's `on_send` only fires for
    // `.`/`&.`).
    let is_dot = cx.raw_source(dot) == ".";
    if !is_dot && !cx.is_safe_navigation(node) {
        return;
    }

    check_space_before_dot(node, cx, dot);
    check_space_after_dot(node, cx, dot);
}

/// RuboCop's `check_space_before_dot`: the gap from the receiver's end to the
/// dot's start.
fn check_space_before_dot(node: NodeId, cx: &Cx<'_>, dot: Range) {
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    let receiver_end = cx.range(receiver).end;
    check_space(cx, receiver_end, dot.start);
}

/// RuboCop's `check_space_after_dot`: the gap from the dot's end to the
/// selector's start.
fn check_space_after_dot(node: NodeId, cx: &Cx<'_>, dot: Range) {
    let name = cx.loc(node).name;
    let selector_begin = if name != Range::ZERO {
        name.start
    } else {
        // `a.()` call-sugar: no selector. Measure to the first non-space
        // token after the dot (the `(`), so `a. ()` is flagged.
        let toks = cx.sorted_tokens();
        let idx = toks.partition_point(|t| t.range.start < dot.end);
        match toks.get(idx) {
            Some(tok) => tok.range.start,
            None => return,
        }
    };
    check_space(cx, dot.end, selector_begin);
}

/// RuboCop's `on_const` + `check_space_after_double_colon`: the gap from the
/// `::` operator's end to the constant name's start. Only `Const` nodes with
/// a non-`cbase` scope carry a `::`.
fn check_space_after_double_colon(node: NodeId, cx: &Cx<'_>) {
    use murphy_plugin_api::NodeKind;
    let NodeKind::Const { scope, .. } = *cx.kind(node) else {
        return;
    };
    let Some(scope_id) = scope.get() else {
        return;
    };
    // `::Foo` (cbase) carries a leading `::` but RuboCop's `loc?(:double_colon)`
    // for a top-level constant has no receiver name to space against; Murphy's
    // `Cbase` scope has a zero-width range, so we skip it.
    if matches!(cx.kind(scope_id), NodeKind::Cbase) {
        return;
    }

    // Murphy does not set `loc.name` on `Const` path nodes, so derive the
    // `::` and the constant-name start from the token stream. The `::` lives
    // between the scope's end and this node's end; the constant name is the
    // first token after the `::`.
    let scope_end = cx.range(scope_id).end;
    let node_end = cx.range(node).end;
    let Some(double_colon_end) = double_colon_end(cx, scope_end, node_end) else {
        return;
    };
    // RuboCop's `name.begin_pos`: first token at/after the `::` end.
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < double_colon_end);
    let Some(name_tok) = toks.get(idx) else {
        return;
    };
    check_space(cx, double_colon_end, name_tok.range.start);
}

/// The end offset of the first `::` token in `[from, to)`, or `None` if
/// absent. `::` is `SourceTokenKind::Other`, so match the source bytes.
fn double_colon_end(cx: &Cx<'_>, from: u32, to: u32) -> Option<u32> {
    if from >= to {
        return None;
    }
    let gap = Range {
        start: from,
        end: to,
    };
    let text = cx.raw_source(gap);
    text.find("::")
        .map(|i| from.saturating_add(i as u32).saturating_add(2) /* len("::") */)
}

/// RuboCop's `check_space`: flag `[begin, end)` only when it is non-empty and
/// consists entirely of spaces/tabs (`SPACES_REGEXP = /\A[ \t]+\z/`). A gap
/// containing a newline (multiline chain) is accepted. Autocorrect removes it.
fn check_space(cx: &Cx<'_>, begin: u32, end: u32) {
    if end <= begin {
        return;
    }
    let range = Range { start: begin, end };
    let src = cx.raw_source(range);
    if src.is_empty() || !src.bytes().all(|b| matches!(b, b' ' | b'\t')) {
        return;
    }
    cx.emit_offense(range, "Avoid using spaces around a method call operator.", None);
    cx.emit_edit(range, "");
}

#[cfg(test)]
mod tests {
    use super::SpaceAroundMethodCallOperator;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── space before dot ──────────────────────────────────────────────────

    #[test]
    fn flags_and_corrects_space_before_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo .bar
                   ^ Avoid using spaces around a method call operator.
            "#},
            "foo.bar\n",
        );
    }

    #[test]
    fn flags_and_corrects_space_after_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo. bar
                    ^ Avoid using spaces around a method call operator.
            "#},
            "foo.bar\n",
        );
    }

    #[test]
    fn flags_space_on_both_sides_of_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo . bar
                   ^ Avoid using spaces around a method call operator.
                     ^ Avoid using spaces around a method call operator.
            "#},
            "foo.bar\n",
        );
    }

    #[test]
    fn flags_multiple_spaces_after_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo.  bar
                    ^^ Avoid using spaces around a method call operator.
            "#},
            "foo.bar\n",
        );
    }

    #[test]
    fn accepts_no_space_around_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("foo.bar\n");
    }

    // ── safe navigation `&.` ───────────────────────────────────────────────

    #[test]
    fn flags_space_before_safe_nav() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo &.bar
                   ^ Avoid using spaces around a method call operator.
            "#},
            "foo&.bar\n",
        );
    }

    #[test]
    fn flags_space_after_safe_nav() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo&. bar
                     ^ Avoid using spaces around a method call operator.
            "#},
            "foo&.bar\n",
        );
    }

    #[test]
    fn accepts_no_space_around_safe_nav() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("foo&.bar\n");
    }

    // ── multiline chains (newline gaps are accepted) ───────────────────────

    #[test]
    fn accepts_multiline_chain_with_leading_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses(indoc! {r#"
            foo
              .bar
              .baz
        "#});
    }

    #[test]
    fn accepts_multiline_chain_with_trailing_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses(indoc! {r#"
            foo.
              bar
        "#});
    }

    // ── call sugar `a.()` ──────────────────────────────────────────────────

    #[test]
    fn call_sugar_flags_space_after_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                a. ()
                  ^ Avoid using spaces around a method call operator.
            "#},
            "a.()\n",
        );
    }

    #[test]
    fn call_sugar_accepts_no_space() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("a.()\n");
    }

    #[test]
    fn call_sugar_flags_space_before_dot() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                a .()
                 ^ Avoid using spaces around a method call operator.
            "#},
            "a.()\n",
        );
    }

    // ── double colon `::` ──────────────────────────────────────────────────

    #[test]
    fn flags_and_corrects_space_after_double_colon() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                Foo:: Bar
                     ^ Avoid using spaces around a method call operator.
            "#},
            "Foo::Bar\n",
        );
    }

    #[test]
    fn accepts_no_space_around_double_colon() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("Foo::Bar\n");
    }

    #[test]
    fn accepts_top_level_constant() {
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("::Foo\n");
    }

    #[test]
    fn does_not_flag_double_colon_method_call() {
        // `Foo:: bar` is a `::`-operator method call, not a constant path.
        // RuboCop's `on_send` only fires for `.`/`&.`, so this is accepted.
        test::<SpaceAroundMethodCallOperator>().expect_no_offenses("Foo:: bar\n");
    }

    #[test]
    fn flags_nested_double_colon_space() {
        test::<SpaceAroundMethodCallOperator>().expect_offense(indoc! {r#"
            Foo::Bar:: Baz
                      ^ Avoid using spaces around a method call operator.
        "#});
    }

    // ── multiple offenses + idempotence ────────────────────────────────────

    #[test]
    fn corrects_chain_of_spaced_dots() {
        test::<SpaceAroundMethodCallOperator>().expect_correction(
            indoc! {r#"
                foo. bar. baz
                    ^ Avoid using spaces around a method call operator.
                         ^ Avoid using spaces around a method call operator.
            "#},
            "foo.bar.baz\n",
        );
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAroundMethodCallOperator>()
            .expect_no_corrections("foo.bar.baz\nFoo::Bar.new\n");
    }
}
murphy_plugin_api::submit_cop!(SpaceAroundMethodCallOperator);
