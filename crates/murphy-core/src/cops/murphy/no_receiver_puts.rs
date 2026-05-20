//! `Murphy/NoReceiverPuts`: flag receiver-less `puts` / `print` / `p`.
//!
//! Ruby debugging output (`puts "x"`, `print 1`, `p obj`) almost always
//! belongs in a logger instead. This cop flags a call **only** when it has
//! no explicit receiver *and* its method name is one of `puts`, `print`,
//! `p`.
//!
//! ADR 0001 Ruby semantics — the negatives are load-bearing:
//! - `obj.puts` has a receiver → no offense.
//! - `logger.info "x"` is named `info` → no offense.
//! - `x = 1` is a local-variable assignment, not a `CallNode`, so
//!   [`on_call_node`](Cop::on_call_node) is never invoked for it.
//!
//! The offense `range` is the **message/selector** location (e.g. just
//! `puts`), in byte offsets (ADR 0001), taken from prism's
//! `CallNode::message_loc()`.
//!
//! **This file is the native-cop template.** To add a cop: copy this file,
//! change `name()`, the name gate, and the message; keep the gate order and
//! the `let-else` on `message_loc`.

use crate::cop::{Cop, CopContext};
use crate::{Offense, Range, Severity};

/// The cop. Stateless unit struct (Phase-1 cops are stateless; design §3).
pub struct NoReceiverPuts;

/// Method names that, called without a receiver, are flagged.
const FLAGGED_NAMES: [&[u8]; 3] = [b"puts", b"print", b"p"];

impl Cop for NoReceiverPuts {
    fn name(&self) -> &str {
        "Murphy/NoReceiverPuts"
    }

    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        // Gate 1 (ADR 0001): an explicit receiver (`obj.puts`) is fine.
        if node.receiver().is_some() {
            return;
        }
        // Gate 2: only the bare-output methods. Compare raw name bytes —
        // `logger.info` is already excluded by gate 1, but `info` / other
        // names must be excluded here too.
        if !FLAGGED_NAMES.contains(&node.name().as_slice()) {
            return;
        }
        // Range = the selector token (e.g. `puts`), not the whole call.
        // Panic-safety only: a selector location always exists for
        // puts/print/p in practice, so this `else` is not a semantic skip —
        // it just refuses to unwrap-panic if prism ever returns `None`.
        let Some(loc) = node.message_loc() else {
            return;
        };

        sink.push(Offense::new(
            ctx.file,
            self.name(),
            Range::from_prism_location(&loc),
            Severity::Warning,
            "Use a logger instead of puts",
        ));
    }
}
