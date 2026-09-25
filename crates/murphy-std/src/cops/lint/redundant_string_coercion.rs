//! `Lint/RedundantStringCoercion` — detects redundant `to_s` in string contexts.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantStringCoercion
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Verbatim port of RuboCop's `to_s_without_args?` matcher `(call _ :to_s)`
//!   (murphy-s1yc). `call` = `{send csend}` covers safe-navigation
//!   (`foo&.to_s`); the `_` receiver binds an absent (receiverless `to_s`)
//!   or present receiver; strict arity (no trailing `...`) means zero args
//!   only (`foo.to_s(8)` does not match). Initial port covers `to_s` without
//!   arguments in interpolation and in bare `print`, `puts`, and `warn`
//!   arguments, including implicit receiver `to_s`. Known v1 limitation:
//!   interpolation handling depends on Murphy's `Dstr` / `Begin` lowering and
//!   may not cover every regexp/symbol/xstr interpolation variant that
//!   RuboCop's interpolation mixin visits.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, def_node_matcher};

// Verbatim port of RuboCop's `to_s_without_args?` (murphy-s1yc):
// `(call _ :to_s)` — `call` = `{send csend}` covers safe-navigation, the `_`
// receiver binds an absent (receiverless) or present receiver, and strict
// arity (no trailing `...`) requires exactly zero arguments.
def_node_matcher!(to_s_without_args, "(call _ :to_s)");

const MSG_DEFAULT: &str = "Redundant use of `Object#to_s` in %<context>s.";
const MSG_SELF: &str = "Use `self` instead of `Object#to_s` in %<context>s.";

#[derive(Default)]
pub struct RedundantStringCoercion;

#[cop(
    name = "Lint/RedundantStringCoercion",
    description = "Checks for redundant string conversion in string contexts.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantStringCoercion {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        check_interpolation_container(node, "interpolation", cx);
    }

    #[on_node(kind = "dsym")]
    fn check_dsym(&self, node: NodeId, cx: &Cx<'_>) {
        check_interpolation_container(node, "interpolation", cx);
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        check_interpolation_container(node, "interpolation", cx);
    }

    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check_interpolation_container(node, "interpolation", cx);
    }

    #[on_node(kind = "send", methods = ["print", "puts", "warn"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if receiver.get().is_some() {
            return;
        }
        let Some(method) = cx.method_name(node) else {
            return;
        };
        let context = format!("`{method}`");
        for &arg in cx.call_arguments(node) {
            if is_to_s_without_args(arg, cx) {
                register_offense(arg, &context, cx);
            }
        }
    }
}

fn check_interpolation_container(node: NodeId, context: &str, cx: &Cx<'_>) {
    for child in cx.descendants(node) {
        if !is_to_s_without_args(child, cx) {
            continue;
        }
        if interpolation_expression_parent(child, node, cx) {
            register_offense(child, context, cx);
        }
    }
}

fn interpolation_expression_parent(node: NodeId, container: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(cx.kind(parent), NodeKind::Begin(_) | NodeKind::Kwbegin(_))
        && cx.ancestors(parent).any(|ancestor| ancestor == container)
}

fn is_to_s_without_args(node: NodeId, cx: &Cx<'_>) -> bool {
    to_s_without_args(node, cx)
}

fn register_offense(node: NodeId, context: &str, cx: &Cx<'_>) {
    let receiver = cx.call_receiver(node).get();
    let template = if receiver.is_some() {
        MSG_DEFAULT
    } else {
        MSG_SELF
    };
    let message = template.replace("%<context>s", context);
    cx.emit_offense(cx.selector(node), &message, None);
    let replacement = receiver
        .map(|recv| cx.raw_source(cx.range(recv)).to_string())
        .unwrap_or_else(|| "self".to_string());
    cx.emit_edit(cx.range(node), &replacement);
}

murphy_plugin_api::submit_cop!(RedundantStringCoercion);

#[cfg(test)]
mod tests {
    use super::RedundantStringCoercion;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_to_s_in_interpolation() {
        test::<RedundantStringCoercion>().expect_correction(
            indoc! {r##"
                "result is #{value.to_s}"
                                   ^^^^ Redundant use of `Object#to_s` in interpolation.
            "##},
            "\"result is #{value}\"\n",
        );
    }

    #[test]
    fn flags_and_corrects_to_s_in_puts_args() {
        test::<RedundantStringCoercion>().expect_correction(
            indoc! {r#"
                puts first.to_s, to_s
                           ^^^^ Redundant use of `Object#to_s` in `puts`.
                                 ^^^^ Use `self` instead of `Object#to_s` in `puts`.
            "#},
            "puts first, self\n",
        );
    }

    #[test]
    fn accepts_other_contexts_and_to_s_with_arguments() {
        test::<RedundantStringCoercion>()
            .expect_no_offenses("p value.to_s\n")
            .expect_no_offenses("puts value.to_s(8)\n")
            .expect_no_offenses("obj.puts value.to_s\n");
    }

    #[test]
    fn flags_csend_to_s_in_interpolation() {
        // Verbatim `(call _ :to_s)` covers safe-navigation: `call` =
        // `{send csend}`, so `foo&.to_s` matches exactly like RuboCop.
        test::<RedundantStringCoercion>().expect_correction(
            indoc! {r##"
                "result is #{value&.to_s}"
                                    ^^^^ Redundant use of `Object#to_s` in interpolation.
            "##},
            "\"result is #{value}\"\n",
        );
    }

    #[test]
    fn flags_receiverless_to_s_in_interpolation() {
        // The `_` receiver binds the absent (nil-filled) slot, so a bare
        // `to_s` matches — same as RuboCop (murphy-if9y).
        test::<RedundantStringCoercion>().expect_correction(
            indoc! {r##"
                "result is #{to_s}"
                             ^^^^ Use `self` instead of `Object#to_s` in interpolation.
            "##},
            "\"result is #{self}\"\n",
        );
    }

    #[test]
    fn accepts_to_s_with_args_in_interpolation() {
        // Strict arity (no trailing `...`): `to_s(8)` does not match.
        test::<RedundantStringCoercion>().expect_no_offenses("\"result is #{value.to_s(8)}\"\n");
    }

    #[test]
    fn accepts_csend_to_s_with_args() {
        // `foo&.to_s(8)` has an argument, so it does not match.
        test::<RedundantStringCoercion>().expect_no_offenses("puts value&.to_s(8)\n");
    }
}
