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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

/// Stateless unit struct.
#[derive(Default)]
pub struct FileTouch;

/// Append modes that only create a file without updating timestamps.
const APPEND_MODES: &[&str] = &["a", "a+", "ab", "a+b", "at", "a+t"];

// RuboCop parity: `Style/FileTouch` `file_open?` inner is
// `(send (const {nil? cbase} :File) :open $(...) (str %APPEND_FILE_MODES))`
// (send-only, top-level only, exactly 2 args). In Murphy `::File` collapses
// to Const scope None, so `nil?` covers bare + `::` (pinned by existing
// `flags_qualified_file_open`); namespaced `Foo::File` still rejects (pinned
// by `boundary_ignores_namespaced_file_open`); send-only dispatch keeps `&.` inners silent (pinned by `boundary_ignores_csend_file_open`).
// Since murphy-m4dc, `$_` captures any single filename node and `$str` is a
// typed capture (captures the mode node AND requires Str kind), so
// `(send (const nil? :File) :open $_ $str)` captures filename + mode Str
// with exactly-2-args (no `...`, pinned by `boundary_ignores_single_arg` /
// `boundary_ignores_three_args` + `boundary_ignores_sym_mode`); the
// `%APPEND_FILE_MODES` const-set membership stays hand-rolled below (tPARAM_CONST
// is a const pattern in Murphy, not a param lookup).
// Capture-bearing, byte-identical offense emission.
def_node_matcher!(
    file_open_candidate,
    "(send (const nil? :File) :open $_ $str)"
);

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

    // `(send (const nil? :File) :open $_ $str)` — captures filename + mode
    // Str with exactly 2 args, send-only (csend silent), top-level File only.
    let Some((filename_node, mode_node)) = file_open_candidate(call, cx) else {
        return;
    };

    // Mode string must be in the append set (`%APPEND_FILE_MODES` stays
    // hand-rolled; `$str` already ensures Str kind).
    let NodeKind::Str(mode_sid) = *cx.kind(mode_node) else {
        return;
    };
    let mode_str = cx.string_str(mode_sid);
    if !APPEND_MODES.contains(&mode_str) {
        return;
    }

    let filename_src = cx.raw_source(cx.range(filename_node));
    let msg = format!(
        "Use `FileUtils.touch({filename_src})` instead of `File.open` in append mode with empty block."
    );
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

    // --- Boundary characterization (murphy-ft88.22): pin the exact node set
    // the hand-rolled `is_global_const(File)` + arity-2 + Str-mode + set
    // check matches, so the verbatim
    // `(send (const nil? :File) :open $_ $str)` refactor can be proven
    // equivalent. `::File` collapses to Const scope None, so `nil?` covers
    // bare + `::` (pinned by `flags_qualified_file_open` + new
    // bare + `::` (pinned by `flags_qualified_file_open`); namespaced `Foo::File` still rejects;
    // inner `&.` is csend and the cop only handles `send`, so it stays
    // silent; exactly 2 args (no `...`) so 1-arg/3-arg stay silent; mode
    // must be Str (sym stays silent) + APPEND_MODES set stays hand-rolled.

    #[test]
    fn boundary_ignores_namespaced_file_open() {
        test::<FileTouch>().expect_no_offenses("Foo::File.open(filename, 'a') {}\n");
    }

    #[test]
    fn boundary_ignores_csend_file_open() {
        test::<FileTouch>().expect_no_offenses("File&.open(filename, 'a') {}\n");
    }

    #[test]
    fn boundary_ignores_single_arg() {
        test::<FileTouch>().expect_no_offenses("File.open(filename) {}\n");
    }

    #[test]
    fn boundary_ignores_three_args() {
        test::<FileTouch>().expect_no_offenses("File.open('f', 'a', 'extra') {}\n");
    }

    #[test]
    fn boundary_ignores_sym_mode() {
        test::<FileTouch>().expect_no_offenses("File.open(filename, :a) {}\n");
    }

    #[test]
    fn boundary_flags_splat_filename() {
        test::<FileTouch>().expect_offense(indoc! {r#"
            File.open(*args, 'a') {}
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileUtils.touch(*args)` instead of `File.open` in append mode with empty block.
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
