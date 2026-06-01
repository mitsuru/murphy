use murphy_ast::NodeId;
use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeKindTag, Severity};

#[derive(Default)]
struct NoTabs;
#[derive(Default)]
struct NoSpaces;

impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}

impl NodeCop for NoTabs {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

impl Cop for NoSpaces {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoSpaces";
    const DESCRIPTION: &'static str = "Forbids trailing spaces.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
}

impl NodeCop for NoSpaces {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(2)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

murphy_plugin_macros::register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(NoTabs);
murphy_plugin_api::submit_cop!(NoSpaces);

fn main() {}
