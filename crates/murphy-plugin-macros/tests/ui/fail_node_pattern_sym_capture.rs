// def_node_matcher! must reject a `$` capture at a symbol slot (the `send`
// method name), which only accepts a `:sym` literal or `_` in v1.

murphy_plugin_macros::def_node_matcher!(m, "(send nil? $_)");

fn main() {}
