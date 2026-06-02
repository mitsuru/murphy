//! `Style/IfUnlessModifierOfIfUnless` — flags modifier `if`/`unless` applied
//! to another conditional (if, unless, or ternary).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IfUnlessModifierOfIfUnless
//! upstream_version_checked: 1.86.2
//! version_added: "0.39"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop 1.86.2. Detects modifier if/unless when the
//!   body is another if-type node (if, unless, or ternary). Autocorrect
//!   rewrites to block form without adding indentation to the body, matching
//!   RuboCop's wrap + remove approach.
//! ```
//!
//! ## Matched shapes
//!
//! A modifier-form `if` or `unless` node whose body is itself an `if`-type
//! node (including ternary `?:`, block-form `if`/`unless`, or another
//! modifier `if`/`unless`).
//!
//! ```ruby
//! # bad — ternary body
//! tired? ? 'stop' : 'go faster' if running?
//!
//! # bad — block-form conditional body
//! if tired?
//!   "please stop"
//! else
//!   "keep going"
//! end if running?
//!
//! # bad — modifier conditional body
//! foo if bar if baz
//!
//! # bad — unless modifier
//! x ? a : b unless running?
//!
//! # good
//! if running?
//!   tired? ? 'stop' : 'go faster'
//! end
//! ```
//!
//! ## Body extraction
//!
//! For modifier `if`: body is in `then_` (the truthy branch).
//! For modifier `unless`: body is in `else_` (the falsy branch in raw AST,
//! which is the "body" of `unless`).
//!
//! ## Autocorrect
//!
//! Rewrites the whole outer node to block form:
//! `<keyword> <condition>\n<body>\nend`
//!
//! No indentation is added to the body — this matches RuboCop's `wrap`
//! behavior which inserts the prefix before and suffix after the body range
//! without modifying the body source itself.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Avoid modifier `%s` after another conditional.";

#[derive(Default)]
pub struct IfUnlessModifierOfIfUnless;

#[cop(
    name = "Style/IfUnlessModifierOfIfUnless",
    description = "Avoid modifier if/unless usage on conditionals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IfUnlessModifierOfIfUnless {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Must be a modifier form (no `end` keyword), not a ternary.
        if !cx.is_modifier_form(node) {
            return;
        }

        let keyword = cx.if_keyword(node);
        if keyword.is_empty() {
            return;
        }

        // Extract the body: for `if` it's `then_`, for `unless` it's `else_`.
        let body_opt = if keyword == "unless" {
            cx.if_else_branch(node)
        } else {
            cx.if_then_branch(node)
        };

        let Some(body) = body_opt.get() else {
            return;
        };

        // Body must be an if-type node (if, unless, ternary — all NodeKind::If).
        // RuboCop's `if_type?` check; excludes while/until/case.
        if !matches!(cx.kind(body), NodeKind::If { .. }) {
            return;
        }

        // Offense: the keyword token of the outer modifier node.
        let keyword_loc = cx.if_keyword_loc(node);
        let offense_range = if keyword_loc != Range::ZERO {
            keyword_loc
        } else {
            cx.range(node)
        };

        let message = MSG.replacen("%s", keyword, 1);
        cx.emit_offense(offense_range, &message, None);

        // Autocorrect: rewrite to block form.
        // Extract condition and body source, then replace the entire outer
        // node with `keyword condition\nbody\nend`.
        let NodeKind::If { cond, .. } = *cx.kind(node) else {
            return;
        };
        let cond_src = cx.raw_source(cx.range(cond));
        let body_src = cx.raw_source(cx.range(body));
        let replacement = format!("{keyword} {cond_src}\n{body_src}\nend");
        cx.emit_edit(cx.range(node), &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::IfUnlessModifierOfIfUnless;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- modifier `if` with ternary body ---

    #[test]
    fn flags_if_modifier_with_ternary_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_offense(indoc! {r#"
            tired? ? 'stop' : 'go faster' if running?
                                          ^^ Avoid modifier `if` after another conditional.
        "#});
    }

    #[test]
    fn corrects_if_modifier_with_ternary_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_correction(
            indoc! {r#"
                tired? ? 'stop' : 'go faster' if running?
                                              ^^ Avoid modifier `if` after another conditional.
            "#},
            "if running?\ntired? ? 'stop' : 'go faster'\nend\n",
        );
    }

    // --- modifier `unless` with ternary body ---

    #[test]
    fn flags_unless_modifier_with_ternary_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_offense(indoc! {r#"
            x ? a : b unless running?
                      ^^^^^^ Avoid modifier `unless` after another conditional.
        "#});
    }

    #[test]
    fn corrects_unless_modifier_with_ternary_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_correction(
            indoc! {r#"
                x ? a : b unless running?
                          ^^^^^^ Avoid modifier `unless` after another conditional.
            "#},
            "unless running?\nx ? a : b\nend\n",
        );
    }

    // --- modifier `if` with block-form conditional body ---

    #[test]
    fn flags_if_modifier_with_block_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_offense(indoc! {r#"
            if tired?
              "please stop"
            else
              "keep going"
            end if running?
                ^^ Avoid modifier `if` after another conditional.
        "#});
    }

    #[test]
    fn corrects_if_modifier_with_block_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_correction(
            indoc! {r#"
                if tired?
                  "please stop"
                else
                  "keep going"
                end if running?
                    ^^ Avoid modifier `if` after another conditional.
            "#},
            "if running?\nif tired?\n  \"please stop\"\nelse\n  \"keep going\"\nend\nend\n",
        );
    }

    // --- modifier `unless` with block-form conditional body ---

    #[test]
    fn flags_unless_modifier_with_block_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_offense(indoc! {r#"
            if tired?
              a
            end unless running?
                ^^^^^^ Avoid modifier `unless` after another conditional.
        "#});
    }

    #[test]
    fn corrects_unless_modifier_with_block_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_correction(
            indoc! {r#"
                if tired?
                  a
                end unless running?
                    ^^^^^^ Avoid modifier `unless` after another conditional.
            "#},
            "unless running?\nif tired?\n  a\nend\nend\n",
        );
    }

    // --- modifier `if` applied to another modifier `if` ---

    #[test]
    fn flags_modifier_if_with_modifier_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_offense(indoc! {r#"
            foo if bar if baz
                       ^^ Avoid modifier `if` after another conditional.
        "#});
    }

    #[test]
    fn corrects_modifier_if_with_modifier_if_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_correction(
            indoc! {r#"
                foo if bar if baz
                           ^^ Avoid modifier `if` after another conditional.
            "#},
            "if baz\nfoo if bar\nend\n",
        );
    }

    // --- no offense cases ---

    #[test]
    fn accepts_non_conditional_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses("foo if running?\n");
    }

    #[test]
    fn accepts_non_conditional_body_unless() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses("foo unless running?\n");
    }

    #[test]
    fn accepts_block_form_if() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses(indoc! {r#"
            if running?
              tired? ? 'stop' : 'go faster'
            end
        "#});
    }

    #[test]
    fn accepts_ternary() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses("tired? ? 'stop' : 'go faster'\n");
    }

    #[test]
    fn accepts_modifier_if_with_non_conditional_body() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses("do_something if condition\n");
    }

    #[test]
    fn accepts_block_form_unless() {
        test::<IfUnlessModifierOfIfUnless>().expect_no_offenses(indoc! {r#"
            unless running?
              do_something
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(IfUnlessModifierOfIfUnless);
