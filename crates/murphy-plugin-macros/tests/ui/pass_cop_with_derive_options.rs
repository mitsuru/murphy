use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{CopOptions, cop, register_cops};

#[derive(CopOptions)]
struct MyOpts {
    #[option(default = 80, description = "max width")]
    max: i64,
}

#[derive(Default)]
struct Wide;

#[cop(name = "Plugin/Wide", options = MyOpts)]
impl Wide {
    #[on_node(kind = "send")]
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(Wide);

fn main() {}
