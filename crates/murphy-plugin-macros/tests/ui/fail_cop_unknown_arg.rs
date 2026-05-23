use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct MyCheck;

#[cop(name = "Plugin/MyCheck", bogus = 1)]
impl MyCheck {
    #[on_node(kind = "send")]
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(MyCheck);

fn main() {}
