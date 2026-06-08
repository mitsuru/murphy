//! `Lint/SendWithMixinArgument` — prefer direct mixin calls over `send`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SendWithMixinArgument
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's send/public_send/__send__ filter, constant receivers,
//!   symbol and string mixin method arguments, one or more constant module
//!   arguments including namespaces, offense message, and autocorrect.
//! ```
//!
//! ## Matched shapes
//!
//! - `Foo.send(:include, Bar)` / `Foo.public_send(:include, Bar)` / `Foo.__send__(...)`
//! - `include`, `prepend`, and `extend` mixin method names as symbols or strings
//!
//! ## Autocorrect
//!
//! Replaces the selector-through-argument tail with the direct mixin call,
//! preserving any receiver before the selector.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range};

const MSG: &str = "Use `%<method>s %<module_name>s` instead of `%<bad_method>s`.";

#[derive(Default)]
pub struct SendWithMixinArgument;

#[cop(
    name = "Lint/SendWithMixinArgument",
    description = "Checks for send/public_send/__send__ when using mixins.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SendWithMixinArgument {
    #[on_node(kind = "send", methods = ["send", "public_send", "__send__"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !receiver_is_allowed(receiver, cx) {
            return;
        }

        let args = cx.list(args);
        let Some((&first_arg, module_args)) = args.split_first() else {
            return;
        };
        let Some(mixin_method) = mixin_method(first_arg, cx) else {
            return;
        };
        if module_args.is_empty() || !module_args.iter().all(|&arg| is_const(arg, cx)) {
            return;
        }

        let module_names = module_args
            .iter()
            .map(|&arg| cx.raw_source(cx.range(arg)))
            .collect::<Vec<_>>()
            .join(", ");
        let bad_range = bad_location(node, cx);
        if bad_range == Range::ZERO {
            return;
        }
        let bad_method = cx.raw_source(bad_range);
        let message = MSG
            .replace("%<method>s", mixin_method)
            .replace("%<module_name>s", &module_names)
            .replace("%<bad_method>s", bad_method);
        let replacement = format!("{mixin_method} {module_names}");

        cx.emit_offense(bad_range, &message, None);
        cx.emit_edit(bad_range, &replacement);
    }
}

fn receiver_is_allowed(receiver: OptNodeId, cx: &Cx<'_>) -> bool {
    match receiver.get() {
        Some(id) => matches!(*cx.kind(id), NodeKind::Const { .. }),
        None => false,
    }
}

fn mixin_method(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    let name = match *cx.kind(node) {
        NodeKind::Sym(symbol) => cx.symbol_str(symbol),
        NodeKind::Str(string) => cx.string_str(string),
        _ => return None,
    };
    match name {
        "include" => Some("include"),
        "prepend" => Some("prepend"),
        "extend" => Some("extend"),
        _ => None,
    }
}

fn is_const(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Const { .. })
}

fn bad_location(node: NodeId, cx: &Cx<'_>) -> Range {
    let selector = cx.loc(node).name;
    if selector == Range::ZERO {
        return Range::ZERO;
    }
    Range {
        start: selector.start,
        end: cx.range(node).end,
    }
}

#[cfg(test)]
mod tests {
    use super::SendWithMixinArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_send_with_include_symbol_and_corrects() {
        test::<SendWithMixinArgument>().expect_correction(
            indoc! {r#"
                Foo.send(:include, Bar)
                    ^^^^^^^^^^^^^^^^^^^ Use `include Bar` instead of `send(:include, Bar)`.
            "#},
            "Foo.include Bar\n",
        );
    }

    #[test]
    fn ignores_receiverless_send_and_self_send() {
        test::<SendWithMixinArgument>()
            .expect_no_offenses(
                indoc! {r#"
                    class Foo
                      send(:include, Bar)
                    end
                "#},
            )
            .expect_no_offenses(
                indoc! {r#"
                    class Foo
                      self.send(:include, Bar)
                    end
                "#},
            );
    }

    #[test]
    fn flags_string_mixin_public_send_send_alias_namespace_and_multiple_modules() {
        test::<SendWithMixinArgument>()
            .expect_correction(
                indoc! {r#"
                    Foo.send('prepend', Bar)
                        ^^^^^^^^^^^^^^^^^^^^ Use `prepend Bar` instead of `send('prepend', Bar)`.
                "#},
                "Foo.prepend Bar\n",
            )
            .expect_correction(
                indoc! {r#"
                    Foo.public_send(:extend, Bar)
                        ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `extend Bar` instead of `public_send(:extend, Bar)`.
                "#},
                "Foo.extend Bar\n",
            )
            .expect_correction(
                indoc! {r#"
                    Foo.__send__(:include, Bar)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use `include Bar` instead of `__send__(:include, Bar)`.
                "#},
                "Foo.include Bar\n",
            )
            .expect_correction(
                indoc! {r#"
                    A::Foo.send(:include, B::Bar)
                           ^^^^^^^^^^^^^^^^^^^^^^ Use `include B::Bar` instead of `send(:include, B::Bar)`.
                "#},
                "A::Foo.include B::Bar\n",
            )
            .expect_correction(
                indoc! {r#"
                    Foo.send(:include, Bar, Baz)
                        ^^^^^^^^^^^^^^^^^^^^^^^^ Use `include Bar, Baz` instead of `send(:include, Bar, Baz)`.
                "#},
                "Foo.include Bar, Baz\n",
            );
    }

    #[test]
    fn ignores_non_mixin_or_direct_mixin_calls() {
        test::<SendWithMixinArgument>()
            .expect_no_offenses("Foo.send(:do_something, Bar)\n")
            .expect_no_offenses("Foo.include Bar\n")
            .expect_no_offenses("foo.send(:include, Bar)\n");
    }
}

murphy_plugin_api::submit_cop!(SendWithMixinArgument);
