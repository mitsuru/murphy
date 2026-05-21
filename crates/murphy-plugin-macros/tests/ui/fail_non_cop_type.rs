// A type that does not implement Cop must be rejected by the trait
// bound that `register_cops!` plants on the static table.

struct NotACop;

murphy_plugin_macros::register_cops!(NotACop);

fn main() {}
