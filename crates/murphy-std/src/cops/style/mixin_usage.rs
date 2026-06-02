//! `Style/MixinUsage` — checks that `include`, `extend`, and `prepend` are used
//! inside classes or modules, not at the top level.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MixinUsage
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `include`, `extend`, and `prepend` calls at the top level.
//!   The call must have exactly one argument which must be a `Const` node
//!   (matching RuboCop's `(send nil? ${:include :extend :prepend} const)` pattern).
//!   Top-level detection walks up through any `Begin`, `Kwbegin`, `If`, or `Def`
//!   ancestors until reaching the root — mirroring RuboCop's `in_top_level_scope?`
//!   recursive matcher which covers `{kwbegin begin if def}` wrappers.
//!   No autocorrect (matches upstream).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "`%s` is used at the top level. Use inside `class` or `module`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MixinUsage;

#[cop(
    name = "Style/MixinUsage",
    description = "Checks that `include`, `extend` and `prepend` exists at the top level.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MixinUsage {
    #[on_node(kind = "send", methods = ["include", "extend", "prepend"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // Must have no receiver (bare `include M`, not `SomeClass.include M`).
    if receiver.get().is_some() {
        return;
    }

    // Must have exactly one argument, and it must be a Const node.
    let args_slice = cx.list(args);
    if args_slice.len() != 1 {
        return;
    }
    let arg = args_slice[0];
    if !matches!(cx.kind(arg), NodeKind::Const { .. }) {
        return;
    }

    // Must be at the top level (not inside a class or module).
    if !in_top_level_scope(node, cx) {
        return;
    }

    let method_name = cx.symbol_str(method);
    let msg = MSG.replacen("%s", method_name, 1);
    cx.emit_offense(cx.range(node), &msg, None);
}

/// Returns `true` if `node` is in the top-level scope.
///
/// Mirrors RuboCop's recursive `in_top_level_scope?` matcher:
/// - The node itself has no parent (it is the root), OR
/// - Its parent is one of `{begin, kwbegin, if, def}` that is itself in
///   the top-level scope.
///
/// `class` and `module` are intentionally excluded from the set — they break
/// the chain, which is the correct behavior (include inside a class is fine).
fn in_top_level_scope(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    loop {
        let parent_opt = cx.parent(current);
        match parent_opt.get() {
            None => {
                // `current` is the root — original node is top-level.
                return true;
            }
            Some(parent) => {
                if matches!(
                    cx.kind(parent),
                    NodeKind::Begin(..)
                        | NodeKind::Kwbegin(..)
                        | NodeKind::If { .. }
                        | NodeKind::Def { .. }
                ) {
                    // Continue walking up through transparent wrappers.
                    current = parent;
                } else {
                    // Parent is class, module, block, etc. — not top-level.
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MixinUsage;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_top_level_include() {
        test::<MixinUsage>().expect_offense(indoc! {"
            include M
            ^^^^^^^^^ `include` is used at the top level. Use inside `class` or `module`.
        "});
    }

    #[test]
    fn flags_top_level_extend() {
        test::<MixinUsage>().expect_offense(indoc! {"
            extend M
            ^^^^^^^^ `extend` is used at the top level. Use inside `class` or `module`.
        "});
    }

    #[test]
    fn flags_top_level_prepend() {
        test::<MixinUsage>().expect_offense(indoc! {"
            prepend M
            ^^^^^^^^^ `prepend` is used at the top level. Use inside `class` or `module`.
        "});
    }

    #[test]
    fn flags_include_inside_begin_at_top_level() {
        test::<MixinUsage>().expect_offense(indoc! {"
            begin
              include M
              ^^^^^^^^^ `include` is used at the top level. Use inside `class` or `module`.
            end
        "});
    }

    #[test]
    fn flags_include_inside_if_at_top_level() {
        test::<MixinUsage>().expect_offense(indoc! {"
            if cond
              include M
              ^^^^^^^^^ `include` is used at the top level. Use inside `class` or `module`.
            end
        "});
    }

    #[test]
    fn flags_include_inside_def_at_top_level() {
        test::<MixinUsage>().expect_offense(indoc! {"
            def foo
              include M
              ^^^^^^^^^ `include` is used at the top level. Use inside `class` or `module`.
            end
        "});
    }

    #[test]
    fn accepts_include_inside_class() {
        test::<MixinUsage>().expect_no_offenses(indoc! {"
            class C
              include M
            end
        "});
    }

    #[test]
    fn accepts_include_inside_module() {
        test::<MixinUsage>().expect_no_offenses(indoc! {"
            module M
              include Other
            end
        "});
    }

    #[test]
    fn accepts_include_with_receiver() {
        // `SomeClass.include M` — has a receiver, skip.
        test::<MixinUsage>().expect_no_offenses("SomeClass.include M\n");
    }

    #[test]
    fn accepts_include_with_multiple_args() {
        // `include A, B` — two args, not flagged by MixinUsage.
        test::<MixinUsage>().expect_no_offenses("include A, B\n");
    }

    #[test]
    fn accepts_include_with_non_const_arg() {
        // `include foo` — arg is a lvar/send, not flagged.
        test::<MixinUsage>().expect_no_offenses("include foo\n");
    }

    #[test]
    fn flags_include_with_namespaced_const_at_top_level() {
        test::<MixinUsage>().expect_offense(indoc! {"
            include Foo::Bar
            ^^^^^^^^^^^^^^^^ `include` is used at the top level. Use inside `class` or `module`.
        "});
    }

    #[test]
    fn accepts_include_with_namespaced_const_inside_class() {
        test::<MixinUsage>().expect_no_offenses(indoc! {"
            class C
              include Foo::Bar
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MixinUsage);
