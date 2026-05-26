//! Shared internal helpers for the murphy-rspec pack.
//!
//! Anything reusable across cops belongs here so each cop file stays
//! focused on its single rule. Visibility is `pub(crate)`: helpers are
//! an implementation detail of the pack, not part of its public surface.

use murphy_plugin_api::{Cx, NodeId, NodeKind, OptNodeId};

/// `true` when `call` is a bare RSpec example call. Matches the receiver
/// shape `RSpec/ExampleLength` and `RSpec/MultipleExpectations` both
/// police: explicit-receiver forms like `Other.it "x"` are some other
/// DSL's `it` and are skipped.
pub(crate) fn is_example_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(
        cx.symbol_str(method),
        "it" | "specify"
            | "example"
            | "fit"
            | "fspecify"
            | "fexample"
            | "xit"
            | "xspecify"
            | "xexample"
            | "skip"
            | "pending"
    )
}
