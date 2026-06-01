// A type that does not implement `NodeCop` must be rejected by
// `submit_cop!` — `build_cop::<C>()` requires `C: NodeCop + Default`.

struct NotACop;

murphy_plugin_macros::register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(NotACop);

fn main() {}
