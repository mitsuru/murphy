// Representative def_node_matcher! invocations that must compile cleanly.
#![allow(dead_code)]

use murphy_plugin_macros::def_node_matcher;

def_node_matcher!(p_wildcard, "_");
def_node_matcher!(p_literal, "42");
def_node_matcher!(p_send, "(send nil? :puts $...)");
def_node_matcher!(p_nested_caps, "(if $_ $(send nil? :foo) _)");
def_node_matcher!(p_union, "{send csend}");
def_node_matcher!(p_traversal, "^(def :foo _ `nil)");

fn main() {}
