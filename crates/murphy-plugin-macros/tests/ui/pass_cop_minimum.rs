use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(NoTabs);

fn main() {}
