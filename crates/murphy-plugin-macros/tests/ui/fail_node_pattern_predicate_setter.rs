// def_node_matcher! mangles `?` / `!` suffixes (murphy-bj7), but a Ruby
// setter `=` suffix has no canonical Rust counterpart and stays an
// invalid identifier — must surface a compile_error pointing at the
// pattern.

murphy_plugin_macros::def_node_matcher!(m, "#foo=");

fn main() {}
