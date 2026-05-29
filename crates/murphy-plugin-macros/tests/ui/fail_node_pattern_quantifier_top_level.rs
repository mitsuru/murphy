// A postfix `*`/`+`/`?` quantifier is only valid as a direct child of a node
// match (murphy-ycx). At the top level there is no list to quantify over, so
// the parser rejects it before the macro can lower anything.

murphy_plugin_macros::def_node_matcher!(m, "int+");

fn main() {}
