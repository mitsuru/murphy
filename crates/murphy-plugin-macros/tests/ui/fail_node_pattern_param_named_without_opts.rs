// node_pattern! must reject a pattern using `%name` without an `opts:`
// clause — there is no CopOptions struct to resolve the field against.

murphy_plugin_macros::node_pattern!(m, "(send _ %method)");

fn main() {}
