use murphy_ast::NodeId;
use murphy_plugin_api::{Cop, Cx, NodeCop, NodeKindTag};
use murphy_plugin_macros::{CopOptions, register_cops};

// `#[derive(CopOptions)]` generates the `impl Default`, so the struct is
// not separately `#[derive(Default)]`.
#[derive(CopOptions)]
struct MaxDepthOptions {
    #[option(default = 3, description = "Maximum nesting depth")]
    max_depth: i64,
}

#[derive(Default)]
struct DeepNest;

impl Cop for DeepNest {
    type Options = MaxDepthOptions;
    const NAME: &'static str = "Plugin/DeepNest";
}

impl NodeCop for DeepNest {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(DeepNest);

fn main() {}
