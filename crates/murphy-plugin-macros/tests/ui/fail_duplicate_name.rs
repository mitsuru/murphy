use murphy_plugin_api::{Cop, NoOptions};

struct First;
struct Second;

impl Cop for First {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/Same";
}

impl Cop for Second {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/Same";
}

murphy_plugin_macros::register_cops!(First, Second);

fn main() {}
