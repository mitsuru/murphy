//! `Lint/ToEnumArguments` — ensures `to_enum`/`enum_for` receives current method arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ToEnumArguments
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-g65d]
//! notes: >
//!   Core RuboCop shapes implemented: missing/swapped positional arguments,
//!   missing keyword/keyword-rest forwarding, __method__/__callee__/literal
//!   method names, explicit self receiver, enum_for alias, and non-current
//!   method guards. Known v1 limitation: wrapped method-name expressions such
//!   as T.must(__callee__) are not resolved (murphy-g65d).
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, OptNodeId};

const MSG: &str = "Ensure you correctly provided all the arguments.";

#[derive(Default)]
pub struct ToEnumArguments;

#[cop(
    name = "Lint/ToEnumArguments",
    description = "Ensures to_enum/enum_for receives current method arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ToEnumArguments {
    #[on_node(kind = "send", methods = ["to_enum", "enum_for"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !matches!(receiver, OptNodeId::NONE) && !receiver.get().is_some_and(|r| matches!(cx.kind(r), NodeKind::SelfExpr)) {
            return;
        }
        let Some(def_node) = cx.ancestors(node).find(|&a| matches!(cx.kind(a), NodeKind::Def { .. } | NodeKind::Defs { .. })) else {
            return;
        };
        let Some((method_arg, passed_args)) = cx.list(args).split_first() else {
            return;
        };
        if !method_name_matches(*method_arg, def_node, cx) {
            return;
        }
        if !arguments_match(passed_args, def_node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

fn method_name_matches(method_arg: NodeId, def_node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(def_name) = cx.method_name(def_node) else {
        return false;
    };
    match *cx.kind(method_arg) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == def_name,
        NodeKind::Send { receiver, method, args } => {
            receiver == OptNodeId::NONE
                && cx.list(args).is_empty()
                && matches!(cx.symbol_str(method), "__method__" | "__callee__")
        }
        _ => false,
    }
}

fn arguments_match(passed_args: &[NodeId], def_node: NodeId, cx: &Cx<'_>) -> bool {
    let args_node = match *cx.kind(def_node) {
        NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => args,
        _ => return false,
    };
    let NodeKind::Args(def_args_list) = *cx.kind(args_node) else {
        return false;
    };
    let def_args = cx.list(def_args_list);
    let mut passed_idx = 0;
    for &def_arg in def_args {
        if matches!(cx.kind(def_arg), NodeKind::Blockarg(_)) {
            continue;
        }
        let Some(&passed_arg) = passed_args.get(passed_idx) else {
            return false;
        };
        match *cx.kind(def_arg) {
            NodeKind::Arg(name) | NodeKind::Restarg(name) => {
                passed_idx += 1;
                if !lvar_name_is(passed_arg, name, cx) {
                    return false;
                }
            }
            NodeKind::Optarg { name, .. } => {
                passed_idx += 1;
                if !lvar_name_is(passed_arg, name, cx) {
                    return false;
                }
            }
            NodeKind::Kwarg(name) | NodeKind::Kwoptarg { name, .. } => {
                if !passed_args[passed_idx..]
                    .iter()
                    .any(|&arg| hash_has_keyword_pair(arg, name, cx))
                {
                    return false;
                }
            }
            NodeKind::Kwrestarg(name) => {
                if !passed_args[passed_idx..]
                    .iter()
                    .any(|&arg| kwsplat_name_is(arg, name, cx) || hash_has_kwsplat(arg, name, cx))
                {
                    return false;
                }
            }
            NodeKind::ForwardArgs => {
                passed_idx += 1;
                if !matches!(cx.kind(passed_arg), NodeKind::ForwardedArgs) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn lvar_name_is(node: NodeId, name: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == cx.symbol_str(name),
        NodeKind::Splat(inner) => inner.get().is_some_and(|id| lvar_name_is(id, name, cx)),
        _ => false,
    }
}

fn kwsplat_name_is(node: NodeId, name: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Kwsplat(inner) => inner.get().is_some_and(|id| lvar_name_is(id, name, cx)),
        _ => false,
    }
}

fn hash_has_keyword_pair(node: NodeId, name: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    let NodeKind::Hash(pairs) = *cx.kind(node) else {
        return false;
    };
    cx.list(pairs).iter().any(|&pair| {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            return false;
        };
        matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == cx.symbol_str(name))
            && lvar_name_is(value, name, cx)
    })
}

fn hash_has_kwsplat(node: NodeId, name: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    let NodeKind::Hash(pairs) = *cx.kind(node) else {
        return false;
    };
    cx.list(pairs).iter().any(|&pair| kwsplat_name_is(pair, name, cx))
}

murphy_plugin_api::submit_cop!(ToEnumArguments);

#[cfg(test)]
mod tests {
    use super::ToEnumArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_required_argument() {
        test::<ToEnumArguments>().expect_offense(indoc! {r#"
            def m(x)
              return to_enum(:m) unless block_given?
                     ^^^^^^^^^^^ Ensure you correctly provided all the arguments.
            end
        "#});
    }

    #[test]
    fn accepts_correct_arguments() {
        test::<ToEnumArguments>().expect_no_offenses(indoc! {r#"
            def m(x, y = 1, *args, required:, optional: true, **kwargs, &block)
              return to_enum(:m, x, y, *args, required: required, optional: optional, **kwargs) unless block_given?
            end
        "#});
    }

    #[test]
    fn flags_swapped_arguments_and_accepts_other_method() {
        test::<ToEnumArguments>()
            .expect_offense(indoc! {r#"
                def m(x, y = 1)
                  return enum_for(__method__, y, x) unless block_given?
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^ Ensure you correctly provided all the arguments.
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                def m(x)
                  return to_enum(:not_m) unless block_given?
                end
            "#});
    }
}
