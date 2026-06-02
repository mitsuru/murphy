//! `Style/Proc` — use `proc` instead of `Proc.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Proc
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `Proc.new { ... }` and `::Proc.new { ... }` when used with a block,
//!   suggesting `proc` instead. `Proc.new` without a block is not flagged.
//!   Handles `block`, `numblock`, and `itblock` forms.
//!
//!   The offense range covers `Proc.new` (from the receiver start to the
//!   selector end). The autocorrect replaces that range with `proc`.
//! ```
//!
//! ## Matched shapes
//!
//! `Block`, `Numblock`, and `Itblock` nodes whose call child is
//! `(send (const {nil? cbase} :Proc) :new)` with no arguments to `new`.
//!
//! ## Enforcement logic
//!
//! Flag unconditionally when the shape matches. Offense range is just
//! `Proc.new` (receiver start to selector end). Autocorrect replaces it
//! with `proc`.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct Proc;

const MSG: &str = "Use `proc` instead of `Proc.new`.";

#[cop(
    name = "Style/Proc",
    description = "Use `proc` instead of `Proc.new`.",
    default_severity = "warning",
    default_enabled = true,
)]
impl Proc {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }
}

fn check_block_call(node: NodeId, cx: &Cx<'_>) {
    // Extract the call child of the block node.
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        NodeKind::Itblock { send, .. } => send,
        _ => return,
    };

    // The call must be a Send with method `new`.
    let NodeKind::Send { receiver, method, args } = *cx.kind(call) else {
        return;
    };
    if cx.symbol_str(method) != "new" {
        return;
    }
    // Must have no arguments to `new`.
    if !cx.list(args).is_empty() {
        return;
    }

    // Receiver must be `(const {nil? cbase} :Proc)`.
    let Some(recv_id) = receiver.get() else {
        return;
    };
    if !cx.is_global_const(recv_id, "Proc") {
        return;
    }

    // Offense range: from the start of the receiver to the end of the
    // `new` selector. This covers exactly `Proc.new` (or `::Proc.new`).
    let offense_range = Range {
        start: cx.range(recv_id).start,
        end: cx.selector(call).end,
    };
    cx.emit_offense(offense_range, MSG, None);
    cx.emit_edit(offense_range, "proc");
}

#[cfg(test)]
mod tests {
    use super::Proc;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_proc_new_block() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = Proc.new { |x| puts x }
                    ^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { |x| puts x }\n",
        );
    }

    #[test]
    fn flags_cbase_proc_new() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = ::Proc.new { |x| puts x }
                    ^^^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { |x| puts x }\n",
        );
    }

    #[test]
    fn accepts_proc_new_without_block() {
        test::<Proc>().expect_no_offenses("p = Proc.new\n");
    }

    #[test]
    fn accepts_cbase_proc_new_without_block() {
        test::<Proc>().expect_no_offenses("p = ::Proc.new\n");
    }

    #[test]
    fn flags_numblock_proc_new() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = Proc.new { puts _1 }
                    ^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { puts _1 }\n",
        );
    }
}
murphy_plugin_api::submit_cop!(Proc);
