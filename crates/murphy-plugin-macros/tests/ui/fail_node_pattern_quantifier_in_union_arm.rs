// A `{}` union arm is not a node child position, so a quantifier may not
// appear inside it (murphy-ycx). The parser rejects this before lowering.

murphy_plugin_macros::def_node_matcher!(m, "{int+ sym}");

fn main() {}
