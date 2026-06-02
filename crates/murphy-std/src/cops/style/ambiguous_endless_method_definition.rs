//! `Style/AmbiguousEndlessMethodDefinition` — flags endless methods inside
//! lower-precedence operators (`and`, `or`) or modifier forms of `if`,
//! `unless`, `while`, `until` where the keyword's scope is ambiguous.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AmbiguousEndlessMethodDefinition
//! upstream_version_checked: 1.86.2
//! version_added: "1.68"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop marks this cop `Enabled: pending`; Murphy maps that to
//!   `default_enabled = false`. Only `on_def` is dispatched — `defs`
//!   (singleton methods) is intentionally excluded, matching RuboCop's
//!   `on_def`-only dispatch. Autocorrect converts the endless def to a
//!   multiline def, leaving the surrounding operator/modifier intact.
//! ```
//!
//! ## Matched shapes
//!
//! An endless `def` node (no `end` keyword) whose immediate parent is one of:
//!
//! - `if`/`unless` in modifier form and the `def` is the then/else branch
//!   (not the condition): `def foo = true if bar`
//! - `and` with `def` as the left-hand side: `def foo = true and bar`
//! - `or` with `def` as the left-hand side: `def foo = true or bar`
//! - `while`/`until` in modifier form with `def` as the body:
//!   `def foo = true while bar`
//!
//! ## Not flagged
//!
//! - `def foo = (true if bar)` — `if` is inside the body; `def`'s parent is
//!   not the `if` node.
//! - `(def foo = true) if bar` — parenthesised def; the parent of `def` is
//!   not the outer `if`.
//! - Block-form: `if bar\n  def foo = true\nend` — the `if` has `end`.
//! - `&&`/`||`: `def foo = true && bar` — binds tighter; ends up in the body.
//!
//! ## Autocorrect
//!
//! Converts the endless `def` to a multiline method definition, leaving the
//! surrounding operator/modifier intact:
//!
//! ```text
//! # before
//! def foo = true if bar
//!
//! # after
//! def foo
//!   true
//! end if bar
//! ```
//!
//! The correction range covers only the `def` node; the ` if bar` suffix
//! remains untouched.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// RuboCop message: "Avoid using `%<keyword>s` statements with endless methods."
fn message(keyword: &str) -> String {
    format!("Avoid using `{keyword}` statements with endless methods.")
}

#[derive(Default)]
pub struct AmbiguousEndlessMethodDefinition;

#[cop(
    name = "Style/AmbiguousEndlessMethodDefinition",
    description = "Avoid endless methods inside ambiguous lower-precedence operators.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl AmbiguousEndlessMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` if the `def` node is an endless method (no `end` keyword).
fn is_endless(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.loc(node).end_keyword() == Range::ZERO
}

/// Checks whether the `def` is an ambiguous child of a lower-precedence
/// operator/modifier and emits an offense + autocorrect if so.
fn check(def_node: NodeId, cx: &Cx<'_>) {
    if !is_endless(def_node, cx) {
        return;
    }

    let Some(parent) = cx.parent(def_node).get() else {
        return;
    };

    let keyword: &str = match cx.kind(parent) {
        NodeKind::If { cond, then_, else_ } => {
            // def must be in the then_ or else_ branch, not the condition.
            let in_branch = then_.get() == Some(def_node) || else_.get() == Some(def_node);
            if !in_branch || *cond == def_node {
                return;
            }
            // Must be modifier form (no `end` keyword on the if/unless).
            if !cx.is_modifier_form(parent) {
                return;
            }
            cx.if_keyword(parent)
        }
        NodeKind::And { lhs, .. } => {
            if *lhs != def_node {
                return;
            }
            "and"
        }
        NodeKind::Or { lhs, .. } => {
            if *lhs != def_node {
                return;
            }
            "or"
        }
        NodeKind::While { body, .. } => {
            if body.get() != Some(def_node) {
                return;
            }
            if !cx.is_modifier_form(parent) {
                return;
            }
            "while"
        }
        NodeKind::Until { body, .. } => {
            if body.get() != Some(def_node) {
                return;
            }
            if !cx.is_modifier_form(parent) {
                return;
            }
            "until"
        }
        _ => return,
    };

    cx.emit_offense(cx.range(parent), &message(keyword), None);

    // Autocorrect: convert the endless def to a multiline form.
    // Replace only the def node's range; the surrounding operator/modifier
    // (` if bar`, ` and bar`, …) is left intact.
    if let Some(replacement) = build_multiline_def(def_node, cx) {
        cx.emit_edit(cx.range(def_node), &replacement);
    }
}

/// Builds a multiline def replacement for an endless `def` node.
///
/// For `def foo(x) = expr`, produces:
/// ```text
/// def foo(x)
///   expr
/// end
/// ```
///
/// The `=` token is found between the method name/selector end and the
/// body start. The header is everything from the def start to just before
/// the `=`, trimmed of trailing whitespace.
fn build_multiline_def(def_node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let def_range = cx.range(def_node);
    let body_node = cx.def_body(def_node).get()?;
    let body_range = cx.range(body_node);
    let body_src = cx.raw_source(body_range);

    // Use the method name (selector) end as the search start for `=`.
    // For `def foo(x) = x`, name ends after `foo`, then `(x) = ` follows.
    // For `def foo = x`, name ends after `foo`, then ` = ` follows.
    let name_range = cx.selector(def_node);
    let search_from = if name_range != Range::ZERO {
        name_range.end
    } else {
        def_range.start
    };

    // Search for `=` in [search_from, body_range.start).
    let eq_range = find_equals_token(cx, search_from, body_range.start)?;

    // Header = source from def start up to (not including) `=`, trimmed.
    let source = cx.source().as_bytes();
    let header_bytes = &source[def_range.start as usize..eq_range.start as usize];
    let header = std::str::from_utf8(header_bytes).ok()?.trim_end();

    Some(format!("{header}\n  {body_src}\nend"))
}

/// Finds the first `=` token (bare assignment operator, not `==`, `=>`, etc.)
/// in the token stream in the range `[from, to)`.
fn find_equals_token(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &source[tok.range.start as usize..tok.range.end as usize] == b"="
        {
            return Some(tok.range);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::AmbiguousEndlessMethodDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Positive cases ---

    #[test]
    fn flags_modifier_if() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true if bar
                ^^^^^^^^^^^^^^^^^^^^^ Avoid using `if` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end if bar
            "},
        );
    }

    #[test]
    fn flags_modifier_unless() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true unless bar
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using `unless` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end unless bar
            "},
        );
    }

    #[test]
    fn flags_and_keyword() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true and bar
                ^^^^^^^^^^^^^^^^^^^^^^ Avoid using `and` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end and bar
            "},
        );
    }

    #[test]
    fn flags_or_keyword() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true or bar
                ^^^^^^^^^^^^^^^^^^^^^ Avoid using `or` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end or bar
            "},
        );
    }

    #[test]
    fn flags_modifier_while() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true while bar
                ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using `while` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end while bar
            "},
        );
    }

    #[test]
    fn flags_modifier_until() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo = true until bar
                ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using `until` statements with endless methods.
            "},
            indoc! {"
                def foo
                  true
                end until bar
            "},
        );
    }

    #[test]
    fn flags_modifier_if_with_args() {
        test::<AmbiguousEndlessMethodDefinition>().expect_correction(
            indoc! {"
                def foo(x) = x if bar
                ^^^^^^^^^^^^^^^^^^^^^ Avoid using `if` statements with endless methods.
            "},
            indoc! {"
                def foo(x)
                  x
                end if bar
            "},
        );
    }

    // --- Negative cases ---

    #[test]
    fn accepts_body_parenthesized_if() {
        // def foo = (true if bar) — if is inside the body, not a parent.
        test::<AmbiguousEndlessMethodDefinition>()
            .expect_no_offenses("def foo = (true if bar)\n");
    }

    #[test]
    fn accepts_parenthesized_def_modifier_if() {
        // (def foo = true) if bar — prism wraps def in Unknown; def's parent
        // is not the outer if.
        test::<AmbiguousEndlessMethodDefinition>()
            .expect_no_offenses("(def foo = true) if bar\n");
    }

    #[test]
    fn accepts_block_form_if() {
        test::<AmbiguousEndlessMethodDefinition>().expect_no_offenses(indoc! {"
            if bar
              def foo = true
            end
        "});
    }

    #[test]
    fn accepts_double_ampersand() {
        // && binds tighter; the && node ends up inside the def body.
        test::<AmbiguousEndlessMethodDefinition>()
            .expect_no_offenses("def foo = true && bar\n");
    }

    #[test]
    fn accepts_double_pipe() {
        test::<AmbiguousEndlessMethodDefinition>()
            .expect_no_offenses("def foo = true || bar\n");
    }

    #[test]
    fn accepts_normal_multiline_def() {
        test::<AmbiguousEndlessMethodDefinition>().expect_no_offenses(indoc! {"
            def foo
              true
            end if bar
        "});
    }
}

murphy_plugin_api::submit_cop!(AmbiguousEndlessMethodDefinition);
