// node_pattern! must reject the node-match form on an atom kind that has
// no literal pattern (e.g. `self`, `lvar`, `ivar`, `cvar`, `gvar`). For
// these the only valid form is the bare kind name.

murphy_plugin_macros::node_pattern!(m, "(self _)");

fn main() {}
