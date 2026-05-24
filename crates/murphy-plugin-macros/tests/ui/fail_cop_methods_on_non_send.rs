use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct MyCheck;

// `methods` only makes sense on send dispatch — it filters by the
// Send node's method symbol. Any other kind doesn't have a "method
// name" axis; the macro must reject this at parse time.
#[cop(name = "Plugin/MyCheck")]
impl MyCheck {
    #[on_node(kind = "if", methods = ["foo"])]
    fn check_if(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
