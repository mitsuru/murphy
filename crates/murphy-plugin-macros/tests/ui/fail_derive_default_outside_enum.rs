use murphy_plugin_macros::CopOptions;

#[derive(CopOptions)]
struct BadEnum {
    // default "nope" is not one of the enum_values.
    #[option(default = "nope", enum_values = ["yes", "maybe"])]
    choice: String,
}

fn main() {}
