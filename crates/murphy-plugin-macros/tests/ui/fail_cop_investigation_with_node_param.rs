//! `#[on_new_investigation]` methods must NOT take a `NodeId` parameter
//! (the file is the subject; there is no specific node). Catch the
//! signature mismatch at macro expansion time so a regression in
//! `validate_signature`'s Dispatch::Investigation branch fails this
//! fixture instead of silently producing a method that won't link.

use murphy_plugin_api::{Cx, NodeId};
use murphy_plugin_macros::cop;

#[derive(Default)]
struct Bad;

#[cop(name = "Plugin/Bad")]
impl Bad {
    // Three params after &self — must be two (only cx).
    #[on_new_investigation]
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

fn main() {}
