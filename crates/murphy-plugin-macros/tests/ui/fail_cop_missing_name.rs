use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

// #[cop] without any arguments — should error with missing 'name'.
#[cop]
impl NoTabs {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
