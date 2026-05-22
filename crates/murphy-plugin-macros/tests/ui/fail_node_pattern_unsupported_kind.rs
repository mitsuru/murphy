// node_pattern! must reject a node match on a kind that has no v1 schema
// (`rescue` is a valid pattern name but absent from the 24-kind table).

murphy_plugin_macros::node_pattern!(m, "(rescue _ _ _)");

fn main() {}
