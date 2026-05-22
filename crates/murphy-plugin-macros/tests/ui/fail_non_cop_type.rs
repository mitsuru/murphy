// A type that does not implement `NodeCop` must be rejected by
// `register_cops!` — `build_cop::<C>()` requires `C: NodeCop + Default`,
// and the uniqueness check reads `<C as Cop>::NAME`.

struct NotACop;

murphy_plugin_macros::register_cops!(NotACop);

fn main() {}
