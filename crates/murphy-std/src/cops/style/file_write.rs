//! `Style/FileWrite` -- favor `File.write`/`File.binwrite` convenience methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileWrite
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (Enabled: pending in RuboCop).
//!   Two patterns are detected:
//!     1. Chained: `File.open(filename, 'w').write(content)` -> `File.write(filename, content)`
//!     2. Block:   `File.open(filename, 'w') { |f| f.write(content) }` -> `File.write(filename, content)`
//!   Mode -> method:
//!     - w, wt, w+, w+t  -> File.write
//!     - wb, w+b         -> File.binwrite
//!   Splat arguments `f.write(*objects)` are skipped (static analysis limitation).
//!   Heredoc content in block form is not reconstructed (gap vs RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! File.open(filename, 'w').write(content)
//! File.open(filename, 'w') { |f| f.write(content) }
//!
//! # good
//! File.write(filename, content)
//! File.binwrite(filename, content)  # for binary mode
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the full expression with `File.write(filename, content)` or
//! `File.binwrite(filename, content)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FileWrite;

const MSG: &str = "Use `File.%s`.";

/// Modes that indicate truncating write (text and binary).
const TRUNCATING_WRITE_MODES: &[&str] = &["w", "wt", "wb", "w+", "w+t", "w+b"];

#[cop(
    name = "Style/FileWrite",
    description = "Favor `File.(bin)write` convenience methods.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl FileWrite {
    /// Check `write` calls for the chained pattern.
    #[on_node(kind = "send", methods = ["write"])]
    fn check_send_write(&self, node: NodeId, cx: &Cx<'_>) {
        check_chained(node, cx);
    }

    /// Check block nodes for the block-based pattern.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_write(node, cx);
    }
}

/// Returns the write method name based on mode ('w'/'wt'/'w+' -> "write", 'wb'/'w+b' -> "binwrite").
fn write_method(mode: &str) -> &'static str {
    if mode.ends_with('b') {
        "binwrite"
    } else {
        "write"
    }
}

/// Check if `node` is a `File.open(filename, mode)` call with a truncating write mode.
/// Returns `Some((filename_node, mode_str))` if matched.
fn match_file_open<'a>(node: NodeId, cx: &Cx<'a>) -> Option<(NodeId, &'a str)> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };

    if cx.symbol_str(method) != "open" {
        return None;
    }

    // Receiver must be File or ::File.
    let recv = receiver.get()?;
    if !cx.is_global_const(recv, "File") {
        return None;
    }

    let arg_list = cx.list(args);
    if arg_list.len() != 2 {
        return None;
    }

    let filename_node = arg_list[0];
    let mode_node = arg_list[1];

    // Mode must be a string literal in the truncating write set.
    let NodeKind::Str(mode_sid) = *cx.kind(mode_node) else {
        return None;
    };
    let mode_str = cx.string_str(mode_sid);
    if !TRUNCATING_WRITE_MODES.contains(&mode_str) {
        return None;
    }

    Some((filename_node, mode_str))
}

/// Pattern 1: `File.open(filename, 'w').write(content)`
/// The `write` send's receiver is the `File.open(...)` call.
fn check_chained(write_node: NodeId, cx: &Cx<'_>) {
    // This write call must have exactly one argument (the content).
    let args = cx.call_arguments(write_node);
    if args.len() != 1 {
        return;
    }

    let content_node = args[0];

    // Skip splat arguments.
    if matches!(cx.kind(content_node), NodeKind::Splat(_)) {
        return;
    }

    // Receiver of write must be File.open.
    let recv = match cx.call_receiver(write_node).get() {
        Some(r) => r,
        None => return,
    };

    let (filename_node, mode_str) = match match_file_open(recv, cx) {
        Some(x) => x,
        None => return,
    };

    let method = write_method(mode_str);
    let msg = MSG.replacen("%s", method, 1);

    let filename_src = cx.raw_source(cx.range(filename_node));
    let content_src = cx.raw_source(cx.range(content_node));
    let replacement = format!("File.{method}({filename_src}, {content_src})");

    // Offense covers from the start of `File.open` to end of `.write(...)`.
    let offense_range = Range {
        start: cx.range(recv).start,
        end: cx.range(write_node).end,
    };

    cx.emit_offense(offense_range, &msg, None);
    cx.emit_edit(offense_range, &replacement);
}

/// Pattern 2: `File.open(filename, 'w') { |f| f.write(content) }`
fn check_block_write(block_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, body } = *cx.kind(block_node) else {
        return;
    };

    // The call must be File.open with a truncating write mode.
    let (filename_node, mode_str) = match match_file_open(call, cx) {
        Some(x) => x,
        None => return,
    };

    // Block args must be wrapped in Args node.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let block_arg_nodes = cx.list(args_list);

    // Must have exactly one argument (the file handle).
    if block_arg_nodes.len() != 1 {
        return;
    }

    let block_arg = block_arg_nodes[0];

    // Block arg must be a simple arg.
    let NodeKind::Arg(arg_sym) = *cx.kind(block_arg) else {
        return;
    };

    // Block body must be a single `write` call on the block arg.
    let body_node = match body.get() {
        Some(b) => b,
        None => return,
    };

    let NodeKind::Send { receiver: write_recv, method: write_method_sym, args: write_args } =
        *cx.kind(body_node)
    else {
        return;
    };

    if cx.symbol_str(write_method_sym) != "write" {
        return;
    }

    // The receiver of write must be the block's lvar.
    let write_recv_node = match write_recv.get() {
        Some(r) => r,
        None => return,
    };
    let NodeKind::Lvar(lvar_sym) = *cx.kind(write_recv_node) else {
        return;
    };
    if lvar_sym != arg_sym {
        return;
    }

    // write must have exactly one argument (the content).
    let write_arg_list = cx.list(write_args);
    if write_arg_list.len() != 1 {
        return;
    }
    let content_node = write_arg_list[0];

    // Skip splat arguments.
    if matches!(cx.kind(content_node), NodeKind::Splat(_)) {
        return;
    }

    let method = write_method(mode_str);
    let msg = MSG.replacen("%s", method, 1);

    let filename_src = cx.raw_source(cx.range(filename_node));
    let content_src = cx.raw_source(cx.range(content_node));
    let replacement = format!("File.{method}({filename_src}, {content_src})");

    cx.emit_offense(cx.range(block_node), &msg, None);
    cx.emit_edit(cx.range(block_node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::FileWrite;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_file_write_already_used() {
        test::<FileWrite>().expect_no_offenses("File.write(filename, content)\n");
    }

    #[test]
    fn no_offense_non_file_receiver() {
        test::<FileWrite>().expect_no_offenses("IO.open(filename, 'w').write(content)\n");
    }

    #[test]
    fn no_offense_non_write_mode() {
        test::<FileWrite>().expect_no_offenses("File.open(filename, 'r').write(content)\n");
        test::<FileWrite>().expect_no_offenses("File.open(filename, 'a').write(content)\n");
    }

    #[test]
    fn no_offense_write_with_multiple_args() {
        // write with multiple args is skipped.
        test::<FileWrite>().expect_no_offenses("File.open(filename, 'w').write(a, b)\n");
    }

    // --- Chained pattern ---

    #[test]
    fn flags_chained_write_text_mode() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            File.open(filename, 'w').write(content)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
        "#});
    }

    #[test]
    fn flags_chained_write_binary_mode() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            File.open(filename, 'wb').write(content)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binwrite`.
        "#});
    }

    #[test]
    fn flags_chained_write_wt_mode() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            File.open(filename, 'wt').write(content)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
        "#});
    }

    #[test]
    fn corrects_chained_write_to_file_write() {
        test::<FileWrite>().expect_correction(
            indoc! {r#"
                File.open(filename, 'w').write(content)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
            "#},
            "File.write(filename, content)\n",
        );
    }

    #[test]
    fn corrects_chained_write_binary_to_file_binwrite() {
        test::<FileWrite>().expect_correction(
            indoc! {r#"
                File.open(filename, 'wb').write(content)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binwrite`.
            "#},
            "File.binwrite(filename, content)\n",
        );
    }

    #[test]
    fn flags_qualified_file_open_chained() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            ::File.open(filename, 'w').write(content)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
        "#});
    }

    // --- Block pattern ---

    #[test]
    fn flags_block_write_text_mode() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            File.open(filename, 'w') { |f| f.write(content) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
        "#});
    }

    #[test]
    fn flags_block_write_binary_mode() {
        test::<FileWrite>().expect_offense(indoc! {r#"
            File.open(filename, 'wb') { |f| f.write(content) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binwrite`.
        "#});
    }

    #[test]
    fn corrects_block_write_to_file_write() {
        test::<FileWrite>().expect_correction(
            indoc! {r#"
                File.open(filename, 'w') { |f| f.write(content) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.write`.
            "#},
            "File.write(filename, content)\n",
        );
    }

    #[test]
    fn no_offense_block_write_with_splat() {
        test::<FileWrite>()
            .expect_no_offenses("File.open(filename, 'w') { |f| f.write(*objects) }\n");
    }
}

murphy_plugin_api::submit_cop!(FileWrite);
