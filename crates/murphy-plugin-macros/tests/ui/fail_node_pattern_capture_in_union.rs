// node_pattern! must reject a `$` capture inside a `{}` union, since a
// union arm cannot bind a capture deterministically in v1.

murphy_plugin_macros::node_pattern!(m, "{$_ int}");

fn main() {}
