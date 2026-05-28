// node_pattern! must reject `${int $float}` — the sugar body of the second
// arm is itself a `$` capture, which would create a nested unwritten slot.

murphy_plugin_macros::node_pattern!(m, "${int $float}");

fn main() {}
