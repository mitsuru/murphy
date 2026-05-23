use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    // Missing &self receiver.
    #[on_node(kind = "send")]
    fn check_send(node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
