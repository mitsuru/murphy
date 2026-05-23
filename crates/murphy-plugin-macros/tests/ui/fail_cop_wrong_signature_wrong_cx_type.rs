use murphy_ast::NodeId;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    // Third parameter should be &Cx<'_>, not &str.
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &str) {}
}

fn main() {}
