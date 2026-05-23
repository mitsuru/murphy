use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct MultiKind;

#[cop(name = "Plugin/MultiKind")]
impl MultiKind {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}

    #[on_node(kind = "if")]
    fn check_if(&self, _node: NodeId, _cx: &Cx<'_>) {}

    #[on_node(kind = "def")]
    fn check_def(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic, MultiKind);

fn main() {}
