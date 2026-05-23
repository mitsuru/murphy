// node_pattern! must reject a `#predicate` name carrying a trailing `?`
// (or `!`), which is not a valid Rust identifier for the emitted call.

murphy_plugin_macros::node_pattern!(m, "#odd?");

fn main() {}
