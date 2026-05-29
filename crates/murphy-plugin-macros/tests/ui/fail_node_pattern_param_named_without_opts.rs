// def_node_matcher! must reject a pattern using `%name` without an `opts:`
// clause — there is no CopOptions struct to resolve the field against.

murphy_plugin_macros::def_node_matcher!(m, "(send _ %method)");

fn main() {}
