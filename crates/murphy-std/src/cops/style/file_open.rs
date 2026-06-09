//! `Style/FileOpen` — flags `File.open` without a block that may leak file descriptors.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileOpen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

const MSG: &str =
    "`File.open` without a block may leak a file descriptor; use the block form.";

#[derive(Default)]
pub struct FileOpen;

#[cop(
    name = "Style/FileOpen",
    description = "Flags `File.open` without a block.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl FileOpen {
    #[on_node(kind = "send", methods = ["open"])]
    fn check_open(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let NodeKind::Const { name, .. } = *cx.kind(recv_id) else {
            return;
        };
        if cx.symbol_str(name) != "File" {
            return;
        }
        if has_block(node, cx) {
            return;
        }
        if node_used_as_receiver(node, cx) || is_assigned(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

fn has_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let parent = cx.parent(node);
    let Some(parent_id) = parent.get() else {
        return false;
    };
    if let NodeKind::Block { call, .. } = cx.kind(parent_id) {
        return *call == node;
    }
    cx.children(node).iter().any(|&child| {
        matches!(cx.kind(child), NodeKind::BlockPass(_))
    })
}

fn node_used_as_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node).get().is_some_and(|parent| match cx.kind(parent) {
        NodeKind::Send { receiver, .. } => receiver.get() == Some(node),
        _ => false,
    })
}

fn is_assigned(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node).get().is_some_and(|parent| {
        matches!(
            cx.kind(parent),
            NodeKind::Lvasgn { .. }
                | NodeKind::Ivasgn { .. }
                | NodeKind::Gvasgn { .. }
                | NodeKind::Cvasgn { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::FileOpen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_file_open_assigned() {
        test::<FileOpen>().expect_offense(indoc! {"
            f = File.open('file')
                ^^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
        "});
    }

    #[test]
    fn flags_file_open_chained() {
        test::<FileOpen>().expect_offense(indoc! {"
            File.open('file').read
            ^^^^^^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
        "});
    }

    #[test]
    fn accepts_file_open_with_block() {
        test::<FileOpen>().expect_no_offenses(
            "File.open('file') { |f| f.read }\n",
        );
    }

    #[test]
    fn accepts_file_read() {
        test::<FileOpen>().expect_no_offenses("File.read('file')\n");
    }
}
murphy_plugin_api::submit_cop!(FileOpen);
