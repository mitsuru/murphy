// A `$` capture inside a quantifier body is ambiguous (which iteration writes
// the slot?), so the parser rejects it (murphy-ycx). The capturing form is
// `$pat+` / `$pat*` / `$pat?`, which binds the iterations as a whole.

murphy_plugin_macros::node_pattern!(m, "(array ($int)+)");

fn main() {}
