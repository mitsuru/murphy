// def_node_matcher! must reject a node match on a kind that has no v1 schema
// (`rescue` is a valid pattern name but absent from the 24-kind table).

murphy_plugin_macros::def_node_matcher!(m, "(rescue _ _ _)");

fn main() {}
