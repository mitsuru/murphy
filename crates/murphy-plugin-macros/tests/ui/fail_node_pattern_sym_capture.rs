// node_pattern! must reject a `$` capture at a symbol slot (the `send`
// method name), which only accepts a `:sym` literal or `_` in v1.

murphy_plugin_macros::node_pattern!(m, "(send nil? $_)");

fn main() {}
