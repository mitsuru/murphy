use murphy_plugin_macros::on_node;

struct MyChecker;

#[on_node(kind = "send")]
fn check_send() {}

fn main() {}
