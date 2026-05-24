//! Trailing-comma tolerance in `#[on_node(kind = "send", methods = [...])]`
//! (murphy-34d). Both shapes parse cleanly:
//!
//! - `#[on_node(kind = "send",)]` — trailing comma after the kind value
//!   alone (no methods).
//! - `#[on_node(kind = "send", methods = ["foo",])]` — trailing comma
//!   inside the methods array.
//!
//! Locks the parser's `if input.is_empty() return` short-circuit and
//! the `while ... break` array loop against future regressions that
//! could quietly reject one of the two shapes.

use murphy_ast::NodeId;
use murphy_plugin_api::Cx;
use murphy_plugin_macros::cop;

#[derive(Default)]
struct TrailingAfterKind;

#[cop(name = "Plugin/TrailingAfterKind")]
impl TrailingAfterKind {
    #[on_node(kind = "send",)]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

#[derive(Default)]
struct TrailingInArray;

#[cop(name = "Plugin/TrailingInArray")]
impl TrailingInArray {
    #[on_node(kind = "send", methods = ["foo", "bar",])]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
