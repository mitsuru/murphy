use murphy_ast::NodeId;
use murphy_plugin_api::{Cx, NoOptions};
use murphy_plugin_macros::{cop, register_cops};

#[derive(Default)]
struct FullMeta;

#[cop(
    name = "Plugin/FullMeta",
    description = "a cop with all args set",
    default_severity = "warning",
    default_enabled = true,
    safe = true,
    safe_autocorrect = false,
    options = NoOptions,
)]
impl FullMeta {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(FullMeta);

fn main() {}
