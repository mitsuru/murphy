use murphy_plugin_macros::cop;

#[derive(Default)]
struct NoTabs;

// No #[on_node] methods — should error.
#[cop(name = "Plugin/NoTabs")]
impl NoTabs {
    fn helper(&self) {}
}

fn main() {}
