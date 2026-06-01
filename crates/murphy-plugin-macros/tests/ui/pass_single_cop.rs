use murphy_ast::NodeId;
use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeKindTag};

#[derive(Default)]
struct NoTabs;

impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}

impl NodeCop for NoTabs {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

murphy_plugin_macros::register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(NoTabs);

fn main() {}
