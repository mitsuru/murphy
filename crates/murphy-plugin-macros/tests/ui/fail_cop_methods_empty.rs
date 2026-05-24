use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct MyCheck;

// `methods = []` filters every send out — equivalent to silently
// disabling the cop. That's almost certainly a typo, not intent;
// the macro must reject it at parse time so the author either lists
// a real method name or drops the argument.
#[cop(name = "Plugin/MyCheck")]
impl MyCheck {
    #[on_node(kind = "send", methods = [])]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
