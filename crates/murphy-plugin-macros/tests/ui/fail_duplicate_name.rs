use murphy_ast::NodeId;
use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeKindTag};

#[derive(Default)]
struct First;
#[derive(Default)]
struct Second;

impl Cop for First {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/Same";
}

impl NodeCop for First {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

impl Cop for Second {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/Same";
}

impl NodeCop for Second {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

murphy_plugin_macros::register_cops!(mode = dynamic, First, Second);

fn main() {}
