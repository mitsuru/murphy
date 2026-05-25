use murphy_plugin_macros::{CopOptionEnum, CopOptions};

#[derive(CopOptionEnum, Clone, Copy)]
enum Style {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[derive(CopOptions)]
struct Options {
    #[option(default = "no_space")]
    style: Style,
}

fn main() {
    let _ = Options::default();
}
