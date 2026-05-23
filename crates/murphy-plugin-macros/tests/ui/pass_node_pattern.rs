// Representative node_pattern! invocations that must compile cleanly.
#![allow(dead_code)]

use murphy_plugin_macros::node_pattern;

node_pattern!(p_wildcard, "_");
node_pattern!(p_literal, "42");
node_pattern!(p_send, "(send nil? :puts $...)");
node_pattern!(p_nested_caps, "(if $_ $(send nil? :foo) _)");
node_pattern!(p_union, "{send csend}");
node_pattern!(p_traversal, "^(def :foo _ `nil)");

fn main() {}
