// def_node_matcher! must reject the node-match form on an atom kind that has
// no literal pattern (`self`). For this the only valid form is the bare
// kind name. (`lvar`/`ivar`/`cvar`/`gvar` were promoted to one-slot
// kinds with a `Symbol` sub-pattern in murphy-o5k.)

murphy_plugin_macros::def_node_matcher!(m, "(self _)");

fn main() {}
