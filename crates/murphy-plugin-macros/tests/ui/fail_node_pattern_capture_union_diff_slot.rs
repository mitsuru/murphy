// node_pattern! must reject a union where arms have different capture slots
// (`$a` vs `$b`) — the losing arm's slot would be unwritten at the
// matcher's `finish` step.

murphy_plugin_macros::node_pattern!(m, "{$a $b}");

fn main() {}
