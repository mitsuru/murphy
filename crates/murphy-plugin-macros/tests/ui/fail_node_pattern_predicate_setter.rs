// node_pattern! mangles `?` / `!` suffixes (murphy-bj7), but a Ruby
// setter `=` suffix has no canonical Rust counterpart and stays an
// invalid identifier — must surface a compile_error pointing at the
// pattern.

murphy_plugin_macros::node_pattern!(m, "#foo=");

fn main() {}
