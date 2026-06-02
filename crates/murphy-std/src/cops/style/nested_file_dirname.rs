//! `Style/NestedFileDirname` — flags nested `File.dirname` calls in favor
//! of the level argument introduced in Ruby 3.1.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NestedFileDirname
//! upstream_version_checked: 1.86.2
//! status: complete
//! gap_issues: []
//! notes: >
//!   Detection is fully implemented. Fires on the outermost File.dirname
//!   when its first argument is also File.dirname (level >= 2). Parent guard
//!   prevents double-reporting on inner nodes in triple+ chains.
//!
//!   Autocorrect is implemented for the common case (no inline comments).
//!   Skipped when the call contains inline comments to avoid dropping them.
//! ```
//!
//! ## Matched shapes
//!
//! `File.dirname(File.dirname(...))` — the outermost call whose first
//! argument is also `File.dirname(...)`. `::File` is also accepted.
//!
//! The offense range covers from the `dirname` selector to the end of the
//! call (matching RuboCop's `offense_range`).
//!
//! ## Example
//!
//! ```ruby
//! # bad
//! File.dirname(File.dirname(path))       # → File.dirname(path, 2)
//! File.dirname(File.dirname(File.dirname(path)))  # → File.dirname(path, 3)
//!
//! # good
//! File.dirname(path, 2)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use `dirname(%<path>s, %<level>s)` instead.";

#[derive(Default)]
pub struct NestedFileDirname;

#[cop(
    name = "Style/NestedFileDirname",
    description = "Use `File.dirname(path, n)` instead of nested `File.dirname` calls.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "3.1",
    options = NoOptions,
)]
impl NestedFileDirname {
    #[on_node(kind = "send", methods = ["dirname"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` if `node` is `File.dirname(path)` with exactly one argument
/// (accepting `::File`). Excludes `File.dirname(path, n)` which already uses
/// the level form.
fn is_file_dirname(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };

    // Receiver must be File or ::File.
    let Some(recv) = receiver.get() else {
        return false;
    };
    if !cx.is_global_const(recv, "File") {
        return false;
    }

    if cx.symbol_str(method) != "dirname" {
        return false;
    }

    // Must have exactly one argument. This excludes `File.dirname(path, 2)`
    // which already uses the level form — treating it as a chain segment would
    // produce semantically wrong autocorrect (under-counted depth).
    cx.list(args).len() == 1
}

/// Walk the nested chain `File.dirname(File.dirname(...path...))` and return
/// `(innermost_path_node, depth)`.
fn path_with_dir_level(node: NodeId, level: u32, cx: &Cx<'_>) -> (NodeId, u32) {
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return (node, level);
    };
    let arg_list = cx.list(args);
    if arg_list.is_empty() {
        return (node, level);
    }
    let first_arg = arg_list[0];
    if is_file_dirname(first_arg, cx) {
        path_with_dir_level(first_arg, level + 1, cx)
    } else {
        (first_arg, level)
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be File.dirname.
    if !is_file_dirname(node, cx) {
        return;
    }

    // Parent guard: if the parent is also File.dirname, we are the inner node
    // — skip to avoid double-reporting. Only the outermost fires.
    if let Some(parent) = cx.parent(node).get() {
        if is_file_dirname(parent, cx) {
            return;
        }
    }

    // First argument must itself be File.dirname (level >= 2).
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return;
    };
    let arg_list = cx.list(args);
    if arg_list.is_empty() {
        return;
    }
    let first_arg = arg_list[0];
    if !is_file_dirname(first_arg, cx) {
        return;
    }

    let (path_node, level) = path_with_dir_level(node, 1, cx);
    if level < 2 {
        return;
    }

    let path_src = cx.raw_source(cx.range(path_node));
    let message = MSG
        .replace("%<path>s", path_src)
        .replace("%<level>s", &level.to_string());

    // Offense range: from the selector (`dirname`) to the end of the node.
    let selector = cx.selector(node);
    let node_range = cx.range(node);
    let offense_range = if selector != Range::ZERO {
        Range {
            start: selector.start,
            end: node_range.end,
        }
    } else {
        node_range
    };

    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: skip when there are inline comments to avoid dropping them.
    if cx.comments_for_node(node).is_empty() {
        let replacement = format!("dirname({path_src}, {level})");
        cx.emit_edit(offense_range, &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::NestedFileDirname;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- basic detection and autocorrect ---

    #[test]
    fn flags_double_nested_dirname() {
        test::<NestedFileDirname>().expect_correction(
            indoc! {r#"
                File.dirname(File.dirname(path))
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dirname(path, 2)` instead.
            "#},
            "File.dirname(path, 2)\n",
        );
    }

    #[test]
    fn flags_triple_nested_dirname() {
        test::<NestedFileDirname>().expect_correction(
            indoc! {r#"
                File.dirname(File.dirname(File.dirname(path)))
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dirname(path, 3)` instead.
            "#},
            "File.dirname(path, 3)\n",
        );
    }

    #[test]
    fn flags_qualified_file_dirname() {
        test::<NestedFileDirname>().expect_correction(
            indoc! {r#"
                ::File.dirname(::File.dirname(path))
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dirname(path, 2)` instead.
            "#},
            "::File.dirname(path, 2)\n",
        );
    }

    // --- no offense cases ---

    #[test]
    fn accepts_single_dirname() {
        test::<NestedFileDirname>().expect_no_offenses("File.dirname(path)\n");
    }

    #[test]
    fn accepts_dirname_with_level_already() {
        test::<NestedFileDirname>().expect_no_offenses("File.dirname(path, 2)\n");
    }

    #[test]
    fn accepts_non_file_dirname() {
        test::<NestedFileDirname>().expect_no_offenses("Foo.dirname(Bar.dirname(path))\n");
    }

    #[test]
    fn accepts_file_dirname_of_non_file_dirname() {
        test::<NestedFileDirname>().expect_no_offenses("File.dirname(some_other_call(path))\n");
    }

    // --- comment guard ---

    #[test]
    fn flags_but_no_autocorrect_with_comment() {
        test::<NestedFileDirname>().expect_offense(indoc! {r#"
            File.dirname(File.dirname(path)) # important
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dirname(path, 2)` instead.
        "#});
    }

    // --- parent guard (no double-reporting) ---

    #[test]
    fn triple_nested_only_one_offense() {
        // Only the outermost dirname fires.
        test::<NestedFileDirname>().expect_offense(indoc! {r#"
            File.dirname(File.dirname(File.dirname(path)))
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dirname(path, 3)` instead.
        "#});
    }

    // --- level-argument guard (regression for the review finding) ---

    #[test]
    fn accepts_inner_dirname_with_level_arg() {
        // File.dirname(File.dirname(path, 2)) — inner already uses level form.
        // The inner node has 2 args so is_file_dirname returns false → not flagged.
        test::<NestedFileDirname>().expect_no_offenses("File.dirname(File.dirname(path, 2))
");
    }

    #[test]
    fn accepts_outer_dirname_with_level_arg() {
        // File.dirname(File.dirname(path), 2) — outer has 2 args.
        // The outer node has 2 args so is_file_dirname returns false → not flagged.
        test::<NestedFileDirname>().expect_no_offenses("File.dirname(File.dirname(path), 2)
");
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <NestedFileDirname as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(3, 1)),
        );
    }
}

murphy_plugin_api::submit_cop!(NestedFileDirname);
