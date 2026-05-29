// def_node_matcher! must reject a `$` capture inside a `{}` union, since a
// union arm cannot bind a capture deterministically in v1.

murphy_plugin_macros::def_node_matcher!(m, "{$_ int}");

fn main() {}
