//! `Style/FileTouch` -- favor `FileUtils.touch` for touching files.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileTouch
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (Enabled: pending in RuboCop).
//!   Detects `File.open(filename, <append_mode>) {}` and suggests
//!   `FileUtils.touch(filename)` instead.
//!   Append modes: a, a+, ab, a+b, at, a+t (matches RuboCop's APPEND_FILE_MODES).
//!   Autocorrect is unsafe (different timestamp semantics for existing files).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! File.open(filename, 'a') {}
//! File.open(filename, 'a+') {}
//!
//! # good
//! FileUtils.touch(filename)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `File.open(filename, 'a') {}` with `FileUtils.touch(filename)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FileTouch;

const MSG: &str = "Use `FileUtils.touch(%s)` instead of `File.open` in append mode with empty block.";

/// Append modes that only create a file without updating timestamps.
const APPEND_MODES: &[&str] = &["a", "a+", "ab", "a+b", "at", "a+t"];

#[cop(
    name = "Style/FileTouch",
    description = "Favor `FileUtils.touch` for touching files.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl FileTouch {
    /// We listen on `block` nodes so we can check both the call and the empty body.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(block_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, body, .. } = *cx.kind(block_node) else {
        return;
    };

    // Block must be empty (no body).
    if body.get().is_some() {
        return;
    }

    // Call must be `File.open`.
    let NodeKind::Send { receiver, method, args } = *cx.kind(call) else {
        return;
    };

    if cx.symbol_str(method) != "open" {
        return;
    }

    // Receiver must be `File` or `::File`.
    let recv = match receiver.get() {
        Some(r) => r,
        None => return,
    };
    if !cx.is_global_const(recv, "File") {
        return;
    }

    // Must have exactly 2 args: (filename, mode_string).
    let arg_list = cx.list(args);
    if arg_list.len() != 2 {
        return;
    }

    let filename_node = arg_list[0];
    let mode_node = arg_list[1];

    // Mode must be a string literal in the append set.
    let NodeKind::Str(mode_sid) = *cx.kind(mode_node) else {
        return;
    };
    let mode_str = cx.string_str(mode_sid);
    if !APPEND_MODES.contains(&mode_str) {
        return;
    }

    let filename_src = cx.raw_source(cx.range(filename_node));
    let msg = MSG.replacen("%s", filename_src, 1);
    let replacement = format!("FileUtils.touch({filename_src})");

    cx.emit_offense(cx.range(block_node), &msg, None);
    cx.emit_edit(cx.range(block_node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::FileTouch;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_non_empty_block() {
        test::<FileTouch>().expect_no_offenses("File.open(filename, 'a') { |f| f.write('x') }\n");
    }

    #[test]
    fn no_offense_non_append_mode() {
        test::<FileTouch>().expect_no_offenses("File.open(filename, 'w') {}\n");
        test::<FileTouch>().expect_no_offenses("File.open(filename, 'r') {}\n");
    }

    #[test]
    fn no_offense_non_file_receiver() {
        test::<FileTouch>().expect_no_offenses("IO.open(filename, 'a') {}\n");
    }

    #[test]
    fn no_offense_no_receiver() {
        test::<FileTouch>().expect_no_offenses("open(filename, 'a') {}\n");
    }

    // --- Offense cases ---

    #[test]
    fn flags_file_open_append_empty_block() {
        test::<FileTouch>().expect_offense(indoc! {r#"
            File.open(filename, 'a') {}
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(filename)` instead of `File.open` in append mode with empty block.
        "#});
    }

    #[test]
    fn flags_file_open_append_plus_empty_block() {
        test::<FileTouch>().expect_offense(indoc! {r#"
            File.open(filename, 'a+') {}
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(filename)` instead of `File.open` in append mode with empty block.
        "#});
    }

    #[test]
    fn flags_file_open_append_binary_mode() {
        test::<FileTouch>().expect_offense(indoc! {r#"
            File.open(filename, 'ab') {}
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(filename)` instead of `File.open` in append mode with empty block.
        "#});
    }

    #[test]
    fn flags_qualified_file_open() {
        test::<FileTouch>().expect_offense(indoc! {r#"
            ::File.open(filename, 'a') {}
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(filename)` instead of `File.open` in append mode with empty block.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_file_open_to_fileutils_touch() {
        test::<FileTouch>().expect_correction(
            indoc! {r#"
                File.open(filename, 'a') {}
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(filename)` instead of `File.open` in append mode with empty block.
            "#},
            "FileUtils.touch(filename)\n",
        );
    }

    #[test]
    fn corrects_with_string_literal_filename() {
        test::<FileTouch>().expect_correction(
            indoc! {r#"
                File.open('foo.txt', 'a') {}
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch('foo.txt')` instead of `File.open` in append mode with empty block.
            "#},
            "FileUtils.touch('foo.txt')\n",
        );
    }
}

murphy_plugin_api::submit_cop!(FileTouch);
