//! `Lint/IncompatibleIoSelectWithFiberScheduler` — checks `IO.select` calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/IncompatibleIoSelectWithFiberScheduler
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covers `IO.select` / `::IO.select` with exactly one non-splat read or
//!   write IO, no exception IOs, and nil/empty opposite side. Emits unsafe
//!   autocorrection unless the return value is assigned. Fully matches the
//!   upstream shapes currently expressible through the v1 AST surface.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG_PREFIX: &str = "Use";

#[derive(Default)]
pub struct IncompatibleIoSelectWithFiberScheduler;

#[cop(
    name = "Lint/IncompatibleIoSelectWithFiberScheduler",
    description = "Checks for IO.select usage that is incompatible with Fiber Scheduler.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IncompatibleIoSelectWithFiberScheduler {
    #[on_node(kind = "send", methods = ["select"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_io_select(node, cx) {
            return;
        }
        let args = cx.call_arguments(node);
        let read = args.first().copied();
        let write = args.get(1).copied();
        let excepts = args.get(2).copied();
        let timeout = args.get(3).copied();

        if excepts.is_some_and(|arg| !is_empty_array(arg, cx) && !matches!(cx.kind(arg), NodeKind::Nil)) {
            return;
        }

        let preferred = if let Some(io) = single_io_array(read, cx).filter(|_| empty_or_nil(write, cx)) {
            preferred_call(io, "wait_readable", timeout, cx)
        } else if let Some(io) = single_io_array(write, cx).filter(|_| empty_or_nil(read, cx)) {
            preferred_call(io, "wait_writable", timeout, cx)
        } else {
            return;
        };

        let current = cx.raw_source(cx.range(node));
        let message = format!("{MSG_PREFIX} `{preferred}` instead of `{current}`.");
        cx.emit_offense(cx.range(node), &message, None);
        if !return_value_assigned(node, cx) {
            cx.emit_edit(cx.range(node), &preferred);
        }
    }
}

fn is_io_select(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    matches!(*cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "IO")
}

fn single_io_array(node: Option<NodeId>, cx: &Cx<'_>) -> Option<NodeId> {
    let node = node?;
    let NodeKind::Array(list) = *cx.kind(node) else {
        return None;
    };
    let elems = cx.list(list);
    if elems.len() == 1 && !matches!(cx.kind(elems[0]), NodeKind::Splat(_)) {
        Some(elems[0])
    } else {
        None
    }
}

fn is_empty_array(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Array(list) if cx.list(list).is_empty())
}

fn empty_or_nil(node: Option<NodeId>, cx: &Cx<'_>) -> bool {
    node.is_none_or(|node| matches!(cx.kind(node), NodeKind::Nil) || is_empty_array(node, cx))
}

fn preferred_call(io: NodeId, method: &str, timeout: Option<NodeId>, cx: &Cx<'_>) -> String {
    let recv = cx.raw_source(cx.range(io));
    match timeout {
        Some(timeout) => format!("{recv}.{method}({})", cx.raw_source(cx.range(timeout))),
        None => format!("{recv}.{method}"),
    }
}

fn return_value_assigned(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|ancestor| cx.is_assignment(ancestor))
}

murphy_plugin_api::submit_cop!(IncompatibleIoSelectWithFiberScheduler);

#[cfg(test)]
mod tests {
    use super::IncompatibleIoSelectWithFiberScheduler;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_single_read_io_select() {
        test::<IncompatibleIoSelectWithFiberScheduler>().expect_correction(
            indoc! {r#"
                IO.select([io], [], [])
                ^^^^^^^^^^^^^^^^^^^^^^^ Use `io.wait_readable` instead of `IO.select([io], [], [])`.
            "#},
            "io.wait_readable\n",
        );
    }

    #[test]
    fn flags_but_does_not_correct_when_return_value_is_assigned() {
        test::<IncompatibleIoSelectWithFiberScheduler>()
            .expect_offense(indoc! {r#"
                rs, _ = IO.select([rp], [])
                        ^^^^^^^^^^^^^^^^^^^ Use `rp.wait_readable` instead of `IO.select([rp], [])`.
            "#})
            .expect_no_corrections("rs, _ = IO.select([rp], [])\n");
    }

    #[test]
    fn accepts_unsupported_io_select_shapes() {
        test::<IncompatibleIoSelectWithFiberScheduler>()
            .expect_no_offenses("IO.select([foo, bar], [], [])\n")
            .expect_no_offenses("IO.select([rp], [wp], [])\n")
            .expect_no_offenses("IO.select([rp], [], [excepts])\n")
            .expect_no_offenses("collection.select { |item| item.ok? }\n");
    }
}
