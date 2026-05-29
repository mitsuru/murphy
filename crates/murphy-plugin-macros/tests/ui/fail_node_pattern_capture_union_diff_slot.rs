// def_node_matcher! must reject a union where arms have different capture slots
// (`$a` vs `$b`) — the losing arm's slot would be unwritten at the
// matcher's `finish` step.

murphy_plugin_macros::def_node_matcher!(m, "{$a $b}");

fn main() {}
