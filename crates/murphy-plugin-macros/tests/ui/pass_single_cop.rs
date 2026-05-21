use murphy_plugin_api::{Cop, NoOptions};

struct NoTabs;

impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}

murphy_plugin_macros::register_cops!(NoTabs);

fn main() {}
