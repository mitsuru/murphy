use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, on_node};

#[derive(Default)]
struct Gated;

#[cop(name = "Plugin/Gated")]
impl Gated {
    #[cfg(feature = "nope")]
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
