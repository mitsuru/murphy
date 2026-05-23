use murphy_plugin_macros::cop;

// #[cop] applied to a struct, not an impl block.
#[cop(name = "Plugin/NoTabs")]
struct NoTabs;

fn main() {}
