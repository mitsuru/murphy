use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

node_pattern!(
    is_debugger_entrypoint,
    "(send nil? {:debugger :byebug :pry})"
);
node_pattern!(is_binding_pry, "(send (send nil? :binding) :pry)");
node_pattern!(
    is_debugger_require,
    r#"(send nil? :require {"debug" "debug/open" "byebug" "pry" "pry-byebug"})"#
);

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
        if is_debugger_entrypoint(node, cx) || is_binding_pry(node, cx) {
            cx.emit_offense(cx.range(node), "Remove debugger entrypoint", None);
            return;
        }
        if is_debugger_require(node, cx) {
            cx.emit_offense(cx.range(node), "Remove debugger require", None);
        }
    }
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
            pry
            ^^^ Remove debugger entrypoint
            require 'pry'
            ^^^^^^^^^^^^^ Remove debugger require
            binding.pry
            ^^^^^^^^^^^ Remove debugger entrypoint
            debugger
            ^^^^^^^^ Remove debugger entrypoint
            byebug
            ^^^^^^ Remove debugger entrypoint
            require 'debug/open'
            ^^^^^^^^^^^^^^^^^^^^ Remove debugger require
        "#}
        );
    }

    #[test]
    fn ignores_non_debugger_usage_and_multibyte_source() {
        expect_no_offenses!(Debugger, "名前 = 'pry'\nlogger.pry\nrequire name\n");
    }
}
