// node_pattern! must reject `opts:` when the pattern contains no `%name`
// runtime parameter — the `opts:` clause would be unused.

#[derive(Default)]
struct MyOpts;
impl murphy_plugin_api::CopOptions for MyOpts {}

murphy_plugin_macros::node_pattern!(m, "(send _ :foo)", opts: MyOpts);

fn main() {}
