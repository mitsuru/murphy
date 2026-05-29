// def_node_matcher! must reject an unknown node kind name with a clear
// compile_error rather than silently producing a never-matching fn.

murphy_plugin_macros::def_node_matcher!(m, "(sned _)");

fn main() {}
