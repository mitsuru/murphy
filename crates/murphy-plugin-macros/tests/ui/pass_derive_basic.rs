use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct BasicOptions {
    flag: bool,
    count: i64,
    label: String,
    names: Vec<String>,
}

fn main() {
    let _ = BasicOptions::default();
}
