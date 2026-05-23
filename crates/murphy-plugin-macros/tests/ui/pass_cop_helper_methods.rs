use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct WithHelpers;

#[cop(name = "Plugin/WithHelpers")]
impl WithHelpers {
    const MAX_DEPTH: u32 = 10;

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let _ = self.helper_depth(node, cx, 0);
    }

    fn helper_depth(&self, _node: NodeId, _cx: &Cx<'_>, depth: u32) -> bool {
        depth < Self::MAX_DEPTH
    }

    fn another_helper(&self, _x: u32) -> bool {
        true
    }
}

register_cops!(mode = dynamic, WithHelpers);

fn main() {}
