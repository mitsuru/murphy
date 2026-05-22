use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct BadOptions {
    // f64 is not a supported option field type.
    ratio: f64,
}

fn main() {}
