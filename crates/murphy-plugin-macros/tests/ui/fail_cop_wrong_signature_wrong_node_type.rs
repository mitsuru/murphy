use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    // Second parameter should be NodeId, not u32.
    #[on_node(kind = "send")]
    fn check_send(&self, _node: u32, _cx: &Cx<'_>) {}
}

fn main() {}
