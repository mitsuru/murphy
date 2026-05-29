// def_node_matcher! must reject `${int $float}` — the sugar body of the second
// arm is itself a `$` capture, which would create a nested unwritten slot.

murphy_plugin_macros::def_node_matcher!(m, "${int $float}");

fn main() {}
