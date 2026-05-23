use murphy_plugin_macros::cop;

trait SomeTrait {}

#[derive(Default)]
struct Foo;

// #[cop] on a trait impl — should error.
#[cop(name = "Plugin/Foo")]
impl SomeTrait for Foo {}

fn main() {}
