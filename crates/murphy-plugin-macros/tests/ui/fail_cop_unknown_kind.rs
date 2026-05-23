use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    #[on_node(kind = "carrot")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
