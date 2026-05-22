// node_pattern! must reject the node-match form on an atom kind such as
// `int`, which has no structural children to destructure in v1.

murphy_plugin_macros::node_pattern!(m, "(int 5)");

fn main() {}
