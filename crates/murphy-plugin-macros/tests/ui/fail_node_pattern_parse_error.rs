// node_pattern! must reject an unknown node kind name with a clear
// compile_error rather than silently producing a never-matching fn.

murphy_plugin_macros::node_pattern!(m, "(sned _)");

fn main() {}
