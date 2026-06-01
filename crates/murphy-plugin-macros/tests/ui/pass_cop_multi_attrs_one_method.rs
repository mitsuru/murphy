use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct BranchChecker;

#[cop(name = "Plugin/BranchChecker")]
impl BranchChecker {
    #[on_node(kind = "if")]
    #[on_node(kind = "case")]
    #[on_node(kind = "when")]
    fn check_branch(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(BranchChecker);

fn main() {}
