// def_node_matcher! must reject the node-match form on an atom kind such as
// `int`, which has no structural children to destructure in v1.

murphy_plugin_macros::def_node_matcher!(m, "(int 5)");

fn main() {}
