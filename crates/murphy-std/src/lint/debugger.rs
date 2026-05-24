use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct Debugger;

#[cop(
    name = "Lint/Debugger",
    description = "Flag debugger calls and debugger requires.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Debugger {
    #[on_node(kind = "send", methods = ["debugger", "byebug", "pry", "require"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        let method = cx.symbol_str(method);
        if receiver == OptNodeId::NONE && matches!(method, "debugger" | "byebug") {
            cx.emit_offense(cx.range(node), "Remove debugger entrypoint", None);
            return;
        }
        if method == "pry"
            && receiver
                .get()
                .is_some_and(|r| is_bare_call(cx, r, "binding"))
        {
            cx.emit_offense(cx.range(node), "Remove debugger entrypoint", None);
            return;
        }
        if receiver == OptNodeId::NONE && method == "require" {
            let Some(arg) = cx.list(args).first().copied() else {
                return;
            };
            let NodeKind::Str(s) = *cx.kind(arg) else {
                return;
            };
            if matches!(
                cx.string_str(s),
                "debug" | "debug/open" | "byebug" | "pry" | "pry-byebug"
            ) {
                cx.emit_offense(cx.range(node), "Remove debugger require", None);
            }
        }
    }
}

fn is_bare_call(cx: &Cx<'_>, node: NodeId, name: &str) -> bool {
    matches!(*cx.kind(node), NodeKind::Send { receiver, method, .. } if receiver == OptNodeId::NONE && cx.symbol_str(method) == name)
}

#[cfg(test)]
mod tests {
    use super::Debugger;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    #[test]
    fn flags_debugger_calls_and_requires() {
        expect_offense!(
            Debugger,
            indoc! {r#"
            require 'pry'
            ^^^^^^^^^^^^^ Remove debugger require
            binding.pry
            ^^^^^^^^^^^ Remove debugger entrypoint
            debugger
            ^^^^^^^^ Remove debugger entrypoint
        "#}
        );
    }

    #[test]
    fn ignores_non_debugger_usage_and_multibyte_source() {
        expect_no_offenses!(Debugger, "名前 = 'pry'\nlogger.pry\nrequire name\n");
    }
}
