//! `Lint/UselessRuby2Keywords` — Checks for unnecessary `ruby2_keywords` calls.
//!
//! `ruby2_keywords` is only useful for methods that accept a rest argument
//! (`*args`) without also accepting keyword arguments (`k:`, `k: 1`, `**kwargs`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessRuby2Keywords
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: direct def and symbol reference cases.
//! ```
//!
//! ## Matched shapes
//!
//! - `ruby2_keywords def foo; end` — def without any arguments.
//! - `ruby2_keywords def foo(arg); end` — def with positional-only args.
//! - `ruby2_keywords def foo(**kwargs); end` — def with keyword splat.
//! - `ruby2_keywords def foo(*args, **kwargs); end` — def with splat + keyword splat.
//! - `ruby2_keywords :foo` where `def foo` has no rest arg or has keyword args.
//!
//! ## No autocorrect
//!
//! There is no safe mechanical rewrite: removal may not be desirable
//! (the method may need `ruby2_keywords` for delegation semantics).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

fn msg(method_name: &str) -> String {
    format!("`ruby2_keywords` is unnecessary for method `{method_name}`.")
}

/// Check if `ruby2_keywords` is useless for the given method args.
fn is_useless(args_id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return true;
    };
    let params = cx.list(list);
    if params.is_empty() {
        return true;
    }
    let has_rest = params.iter().any(|p| matches!(*cx.kind(*p), NodeKind::Restarg(_)));
    let has_kw = params.iter().any(|p| {
        matches!(
            *cx.kind(*p),
            NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. } | NodeKind::Kwrestarg(_)
        )
    });
    // ruby2_keywords is only useful when there is *args AND no keyword args.
    // Useless unless: has rest arg AND no keyword args.
    !has_rest || has_kw
}

/// Find a `Def` with the given name among the direct children of `parent`.
/// Uses `cx.children()` which returns only immediate children, so it will not
/// traverse into nested `class`/`module`/`sclass` bodies.
fn find_def_in_scope(parent: NodeId, target: &str, cx: &Cx<'_>) -> Option<NodeId> {
    for child in cx.children(parent) {
        if let NodeKind::Def { name, .. } = *cx.kind(child) {
            if cx.symbol_str(name) == target {
                return Some(child);
            }
        }
    }
    None
}

#[derive(Default)]
pub struct UselessRuby2Keywords;

#[cop(
    name = "Lint/UselessRuby2Keywords",
    description = "Checks for unnecessary `ruby2_keywords` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessRuby2Keywords {
    #[on_node(kind = "send", methods = ["ruby2_keywords"])]
    fn check_ruby2_keywords(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method: _, args, .. } = *cx.kind(node) else {
            return;
        };
        // Only flag bare `ruby2_keywords` calls (no explicit receiver).
        if receiver != OptNodeId::NONE {
            return;
        };
        let args_list = cx.list(args);
        if args_list.is_empty() {
            return;
        }

        for &arg in args_list {
            match *cx.kind(arg) {
                NodeKind::Def {
                    name,
                    args: def_args,
                    ..
                } => {
                    if is_useless(def_args, cx) {
                        cx.emit_offense(
                            cx.range(node),
                            &msg(cx.symbol_str(name)),
                            None,
                        );
                    }
                }
                NodeKind::Sym(method_name) => {
                    let target = cx.symbol_str(method_name);
                    // Look for a `def` matching the symbol among the direct
                    // children of the immediate parent (same lexical scope).
                    if let Some(parent) = cx.parent(node).get() {
                        if let Some(def_id) = find_def_in_scope(parent, target, cx) {
                            let NodeKind::Def { args: def_args, .. } = *cx.kind(def_id) else {
                                return;
                            };
                            if is_useless(def_args, cx) {
                                cx.emit_offense(cx.range(node), &msg(target), None);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UselessRuby2Keywords;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_no_args() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_positional_args() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(arg); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_kwrestarg() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(**kwargs); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_keyword_args() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(i:, j:); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_restarg_with_keywords() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(*args, i:, j:); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_restarg_with_kwoptarg() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(*args, i: 1); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn flags_restarg_with_kwrestarg() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            ruby2_keywords def foo(*args, **kwargs); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn accepts_restarg_only() {
        test::<UselessRuby2Keywords>().expect_no_offenses(indoc! {"
            ruby2_keywords def foo(*args); end
        "});
    }

    #[test]
    fn accepts_restarg_with_positional() {
        test::<UselessRuby2Keywords>().expect_no_offenses(indoc! {"
            ruby2_keywords def foo(arg1, arg2, *rest); end
        "});
    }

    #[test]
    fn flags_symbol_useless() {
        test::<UselessRuby2Keywords>().expect_offense(indoc! {r#"
            def foo(**kwargs); end
            ruby2_keywords :foo
            ^^^^^^^^^^^^^^^^^^^ `ruby2_keywords` is unnecessary for method `foo`.
        "#});
    }

    #[test]
    fn accepts_symbol_allowed() {
        test::<UselessRuby2Keywords>().expect_no_offenses(indoc! {"
            def foo(*args); end
            ruby2_keywords :foo
        "});
    }

    #[test]
    fn accepts_symbol_no_def() {
        test::<UselessRuby2Keywords>().expect_no_offenses(indoc! {"
            ruby2_keywords :foo
        "});
    }
}

murphy_plugin_api::submit_cop!(UselessRuby2Keywords);
