use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct OptionalOptions {
    maybe_flag: Option<bool>,
    maybe_count: Option<i64>,
    maybe_label: Option<String>,
}

fn main() {
    let _ = OptionalOptions::default();
}
