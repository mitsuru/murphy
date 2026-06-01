//! `Style/ReturnNilInPredicateMethodDefinition` — flags `nil` returns in
//! predicate method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ReturnNilInPredicateMethodDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects explicit `return nil` / bare `return` inside predicate methods,
//!   and implicit `nil` as the last expression of the method body.
//!   Also recurses into if/unless branches to find nil returns inside
//!   conditionals (handles `if cond; nil; end` and `if cond; nil; else; ...`).
//!   AllowedMethods is supported (Vec<String>); AllowedPatterns (regex) is not.
//!   Autocorrect is unsafe (RuboCop marks it `SafeAutoCorrect: false`).
//!   `def self.foo?` singleton methods are also checked (via defs hook).
//!   Gap: Does not recurse into case/when branches (conservative, no false positives).
//! ```
//!
//! ## Matched shapes
//!
//! Method definitions (`def` / `def self.foo`) with a name ending in `?` that
//! contain any of:
//!
//! - `return nil` (explicit return of nil)
//! - bare `return` (implicit nil return)
//! - `nil` as the last expression of the body (implicit nil return)
//! - `nil` in then/else branches of `if`/`unless` in implicit-return position
//!
//! ## Autocorrect
//!
//! - `return nil` → `return false`
//! - bare `return` → `return false`
//! - implicit `nil` → `false`

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

const MSG: &str = "Return `false` instead of `nil` in predicate methods.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ReturnNilInPredicateMethodDefinition;

/// Options for `Style/ReturnNilInPredicateMethodDefinition`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods that are allowed to return nil in predicate definitions."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/ReturnNilInPredicateMethodDefinition",
    description = "Use `false` instead of `nil` in predicate method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl ReturnNilInPredicateMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }
}

fn check_def(node: NodeId, cx: &Cx<'_>) {
    // Must be a predicate method (name ends with `?`).
    if !cx.is_predicate_method(node) {
        return;
    }

    // Check AllowedMethods.
    let opts = cx.options_or_default::<Options>();
    if let Some(name) = cx.method_name(node)
        && opts.allowed_methods.iter().any(|m| m == name) {
            return;
        }

    // Check the method body.
    check_body(cx.def_body(node), cx);
}

/// Check method body for nil returns. `body_opt` is the top-level body node.
fn check_body(body_opt: OptNodeId, cx: &Cx<'_>) {
    let Some(body_id) = body_opt.get() else {
        return;
    };
    check_node_for_nil_returns(body_id, cx, true);
}

/// Recursively check a node for nil returns.
///
/// `is_implicit_return_position` means this node is in the implicit-return
/// position (last expression of the method body, or the final expression in a
/// branch that is in implicit-return position). Explicit `return nil` / bare
/// `return` offenses are flagged regardless of position.
fn check_node_for_nil_returns(node: NodeId, cx: &Cx<'_>, is_implicit_return_position: bool) {
    match cx.kind(node) {
        NodeKind::Return(value_opt) => {
            // `return nil` or bare `return` — always an offense regardless of position.
            match value_opt.get() {
                None => {
                    // bare `return` → implicit nil.
                    let range = cx.range(node);
                    cx.emit_offense(range, MSG, None);
                    cx.emit_edit(range, "return false");
                }
                Some(val_id) => {
                    if matches!(cx.kind(val_id), NodeKind::Nil) {
                        // `return nil` → `return false`.
                        let range = cx.range(node);
                        cx.emit_offense(range, MSG, None);
                        cx.emit_edit(range, "return false");
                    }
                }
            }
        }

        NodeKind::Nil
            // Implicit nil return — only flag when in implicit-return position.
            if is_implicit_return_position => {
                let range = cx.range(node);
                cx.emit_offense(range, MSG, None);
                cx.emit_edit(range, "false");
            }

        NodeKind::Begin(list) => {
            // Walk all children; only the last one can be in implicit-return position.
            let children = cx.list(*list);
            let len = children.len();
            for (i, &child_id) in children.iter().enumerate() {
                let last = i + 1 == len;
                check_node_for_nil_returns(child_id, cx, last && is_implicit_return_position);
            }
        }

        NodeKind::If { then_, else_, .. } => {
            // Recurse into then/else branches.
            // Both branches can be in implicit-return position if the if/unless
            // itself is in implicit-return position.
            if let Some(then_id) = then_.get() {
                check_node_for_nil_returns(then_id, cx, is_implicit_return_position);
            }
            if let Some(else_id) = else_.get() {
                check_node_for_nil_returns(else_id, cx, is_implicit_return_position);
            }
        }

        _ => {
            // Other nodes: don't recurse. We don't descend into case/when,
            // nested defs, or other compound nodes (conservative gap).
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, ReturnNilInPredicateMethodDefinition};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Explicit `return nil` -----

    #[test]
    fn flags_return_nil_early_in_predicate() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  return nil if x
                  ^^^^^^^^^^ Return `false` instead of `nil` in predicate methods.
                  true
                end
            "},
            indoc! {"
                def foo?
                  return false if x
                  true
                end
            "},
        );
    }

    #[test]
    fn flags_return_nil_as_only_stmt() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  return nil
                  ^^^^^^^^^^ Return `false` instead of `nil` in predicate methods.
                end
            "},
            indoc! {"
                def foo?
                  return false
                end
            "},
        );
    }

    // ----- Bare `return` -----

    #[test]
    fn flags_bare_return_in_predicate() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  return if x
                  ^^^^^^ Return `false` instead of `nil` in predicate methods.
                  true
                end
            "},
            indoc! {"
                def foo?
                  return false if x
                  true
                end
            "},
        );
    }

    // ----- Implicit `nil` as last expression -----

    #[test]
    fn flags_implicit_nil_as_last_expr() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  nil
                  ^^^ Return `false` instead of `nil` in predicate methods.
                end
            "},
            indoc! {"
                def foo?
                  false
                end
            "},
        );
    }

    #[test]
    fn flags_nil_as_last_in_multi_stmt_body() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  do_something
                  nil
                  ^^^ Return `false` instead of `nil` in predicate methods.
                end
            "},
            indoc! {"
                def foo?
                  do_something
                  false
                end
            "},
        );
    }

    // ----- `nil` inside if branches -----

    #[test]
    fn flags_nil_in_if_then_branch_at_end() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  if x
                    nil
                    ^^^ Return `false` instead of `nil` in predicate methods.
                  end
                end
            "},
            indoc! {"
                def foo?
                  if x
                    false
                  end
                end
            "},
        );
    }

    #[test]
    fn flags_nil_in_else_branch_at_end() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def foo?
                  if x
                    true
                  else
                    nil
                    ^^^ Return `false` instead of `nil` in predicate methods.
                  end
                end
            "},
            indoc! {"
                def foo?
                  if x
                    true
                  else
                    false
                  end
                end
            "},
        );
    }

    // ----- Non-predicate method — no offense -----

    #[test]
    fn no_offense_non_predicate_method() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_no_offenses(indoc! {"
            def foo
              nil
            end
        "});
    }

    #[test]
    fn no_offense_return_nil_non_predicate() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_no_offenses(indoc! {"
            def foo
              return nil
            end
        "});
    }

    // ----- Returns false — no offense -----

    #[test]
    fn no_offense_returns_false() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_no_offenses(indoc! {"
            def foo?
              false
            end
        "});
    }

    #[test]
    fn no_offense_return_false_early() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_no_offenses(indoc! {"
            def foo?
              return false if x
              true
            end
        "});
    }

    // ----- Singleton method (defs) -----

    #[test]
    fn flags_singleton_predicate_method() {
        test::<ReturnNilInPredicateMethodDefinition>().expect_correction(
            indoc! {"
                def self.foo?
                  return nil
                  ^^^^^^^^^^ Return `false` instead of `nil` in predicate methods.
                end
            "},
            indoc! {"
                def self.foo?
                  return false
                end
            "},
        );
    }

    // ----- AllowedMethods option -----

    #[test]
    fn no_offense_when_method_in_allowed_list() {
        let opts = Options {
            allowed_methods: vec!["foo?".to_string()],
        };
        test::<ReturnNilInPredicateMethodDefinition>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {"
                def foo?
                  nil
                end
            "});
    }
}
murphy_plugin_api::submit_cop!(ReturnNilInPredicateMethodDefinition);
