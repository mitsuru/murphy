//! `Style/FileOpen` — flags `File.open` without a block that may leak file descriptors.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileOpen
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `offensive_usage?`: only `File.open` whose value is
//!   discarded, assigned to a local variable, or used as the receiver of a
//!   chained call is flagged. A returned or argument-passed open file is left
//!   to the caller.
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
        let recv_id = unwrap_begin(recv_id, cx);
        let NodeKind::Const { name, .. } = *cx.kind(recv_id) else {
            return;
        };
        if cx.symbol_str(name) != "File" {
            return;
        }
        if has_block(node, cx) {
            return;
        }
        // RuboCop's `offensive_usage?`: only flag when the descriptor is at
        // risk — discarded, assigned to a local, or the receiver of a chained
        // call. A returned or argument-passed `File.open` is the caller's to
        // manage, so it is left alone.
        if !offensive_usage(node, cx) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

/// RuboCop's `offensive_usage?`: flag when `File.open`'s value is discarded, or
/// when its immediate parent assigns it to a local variable, or when it is the
/// receiver of a chained call.
fn offensive_usage(node: NodeId, cx: &Cx<'_>) -> bool {
    // `is_value_used` is the canonical port of rubocop-ast's `Node#value_used?`:
    // it propagates discard through `if`/`case` branches, loop bodies, and
    // pass-through containers, not just `begin` sequences. A discarded value
    // means the descriptor leaks, so the usage is offensive.
    if !cx.is_value_used(node) {
        return true;
    }
    let Some(parent_id) = cx.parent(node).get() else {
        return false;
    };
    matches!(*cx.kind(parent_id), NodeKind::Lvasgn { .. }) || is_chained_receiver(node, cx)
}

/// True when `node` is the receiver of a chained call, looking through any
/// wrapping parentheses: `File.open('f').read` and `(File.open('f')).read` both
/// qualify (a parenthesised group lowers to a single-child `Begin`).
fn is_chained_receiver(mut node: NodeId, cx: &Cx<'_>) -> bool {
    while let Some(parent) = cx.parent(node).get() {
        if cx.call_receiver(parent).get() == Some(node) {
            return true;
        }
        match cx.kind(parent) {
            NodeKind::Begin(list) if matches!(cx.list(*list), [only] if *only == node) => {
                node = parent;
            }
            _ => return false,
        }
    }
    false
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            break;
        }
        node = child_list[0];
    }
    node
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
    fn flags_parenthesized_file_receiver() {
        test::<FileOpen>().expect_offense(indoc! {"
            (File).open('file')
            ^^^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
        "});
    }

    #[test]
    fn flags_file_open_chained() {
        test::<FileOpen>().expect_offense(indoc! {"
            File.open('file').read
            ^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
        "});
    }

    #[test]
    fn flags_parenthesized_file_open_chained() {
        // A chained call on a parenthesised `File.open` still leaks the
        // descriptor; the wrapping `Begin` must be looked through.
        test::<FileOpen>().expect_offense(indoc! {"
            (File.open('file')).read
             ^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
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

    #[test]
    fn flags_standalone_file_open_whose_value_is_discarded() {
        // First statement in a sequence: value discarded -> flagged.
        test::<FileOpen>().expect_offense(indoc! {"
            File.open('file')
            ^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
            do_more
        "});
    }

    #[test]
    fn accepts_file_open_as_method_return_value() {
        // Implicit return of an open file: the caller manages the descriptor.
        test::<FileOpen>().expect_no_offenses("def io\n  File.open('file')\nend\n");
    }

    #[test]
    fn accepts_file_open_explicitly_returned() {
        test::<FileOpen>().expect_no_offenses("def io\n  return File.open('file')\nend\n");
    }

    #[test]
    fn accepts_file_open_passed_as_argument() {
        test::<FileOpen>().expect_no_offenses("process(File.open('file'))\n");
    }

    #[test]
    fn accepts_file_open_as_keyword_argument_value() {
        // Mastodon's `attach(file: File.open('…'))` shape.
        test::<FileOpen>().expect_no_offenses("attach(file: File.open('file'))\n");
    }

    #[test]
    fn accepts_file_open_as_block_return_value() {
        // `let(:f) { File.open('…') }` — the block's value is the open file.
        test::<FileOpen>().expect_no_offenses("let(:f) { File.open('file') }\n");
    }

    #[test]
    fn flags_file_open_in_discarded_if_branch() {
        // The `if`'s value is discarded (a non-final statement), so the branch
        // value — the open file — is discarded too and the descriptor leaks.
        test::<FileOpen>().expect_offense(indoc! {"
            if cond
              File.open('file')
              ^^^^^^^^^^^^^^^^^ `File.open` without a block may leak a file descriptor; use the block form.
            end
            do_more
        "});
    }

    #[test]
    fn accepts_file_open_as_used_if_branch_value() {
        // The `if` is the assigned value, so its branch — the open file — is
        // used; the caller manages the descriptor.
        test::<FileOpen>().expect_no_offenses(indoc! {"
            f = if cond
              File.open('file')
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(FileOpen);
