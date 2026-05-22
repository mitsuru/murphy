use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct AttrOptions {
    #[option(default = 80, description = "Maximum line width")]
    max: i64,

    #[option(default = "indented", enum_values = ["indented", "aligned"])]
    style: String,

    #[option(default = ["id"], description = "Names always allowed")]
    allowed: Vec<String>,

    #[option(deprecated = "use max", reason = "renamed for clarity")]
    max_chars: Option<i64>,

    #[option(deprecated)]
    legacy: Option<bool>,
}

fn main() {
    let _ = AttrOptions::default();
}
