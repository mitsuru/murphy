use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct DuplicateOptions {
    #[option(name = "Shared")]
    first: String,
    #[option(name = "Shared")]
    second: String,
}

fn main() {}
