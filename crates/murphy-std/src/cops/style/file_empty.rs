//! `Style/FileEmpty` — prefer `File.empty?` when checking if a file is empty.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileEmpty
//! upstream_version_checked: 1.86.2
//! version_added: "1.48"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Unsafe: File.size/File.read raise ENOENT on missing files; File.empty? does not.
//!   Detects seven patterns (see Matched shapes below).
//!   Both File and FileTest receiver classes are handled.
//!   Scope: nil-scoped and cbase-scoped consts are accepted.
//!   Bang rule: replacement is negated (`!`) when:
//!     - operator is `>=` or `!=` AND the first child (receiver) is NOT a `!` call, OR
//!     - operator is `==` AND the first child (receiver) IS a `!` call.
//!   Autocorrect replaces the whole expression node.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! File.zero?('path')                       → File.empty?('path')
//! File.size('path') == 0                   → File.empty?('path')
//! File.size('path') >= 0                   → !File.empty?('path')
//! !File.size('path') == 0                  → !File.empty?('path')
//! File.size('path').zero?                  → File.empty?('path')
//! File.read('path').empty?                 → File.empty?('path')
//! File.binread('path') == ''               → File.empty?('path')
//! File.binread('path') != ''               → !File.empty?('path')
//! FileTest.zero?('path')                   → FileTest.empty?('path')
//!
//! # good
//! File.empty?('path')
//! FileTest.empty?('path')
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FileEmpty;

#[cop(
    name = "Style/FileEmpty",
    description = "Prefer `File.empty?` when checking if a file is empty.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl FileEmpty {
    #[on_node(kind = "send", methods = ["zero?", "==", "!=", ">=", "empty?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some((file_class_src, arg_src, needs_bang)) = detect_offense(node, cx) {
            let replacement = if needs_bang {
                format!("!{file_class_src}.empty?({arg_src})")
            } else {
                format!("{file_class_src}.empty?({arg_src})")
            };
            let msg = format!("Use `{file_class_src}.empty?({arg_src})` instead.");
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), &replacement);
        }
    }
}

/// Returns `Some((file_class_src, arg_src, needs_bang))` if the node is an
/// offensive pattern, or `None` if it should be accepted.
fn detect_offense<'a>(node: NodeId, cx: &'a Cx<'a>) -> Option<(&'a str, &'a str, bool)> {
    let method = cx.method_name(node)?;

    match method {
        // Pattern: File.zero?('path') or FileTest.zero?('path')
        "zero?" => {
            let recv = cx.call_receiver(node).get()?;

            // Direct receiver: `File.zero?('path')`
            if let Some(fc) = file_class_src(recv, cx) {
                let args = cx.call_arguments(node);
                if args.len() == 1 {
                    let arg_src = cx.raw_source(cx.range(args[0]));
                    return Some((fc, arg_src, false));
                }
            }

            // Chained: `File.size('path').zero?` — receiver is `File.size(path)`
            if let Some((fc, a)) = extract_file_size_or_read(recv, cx) {
                return Some((fc, a, false));
            }

            None
        }

        // Pattern: File.read('path').empty? or File.size('path').empty?
        "empty?" if cx.call_arguments(node).is_empty() => {
            let recv = cx.call_receiver(node).get()?;
            let (fc, a) = extract_file_size_or_read(recv, cx)?;
            Some((fc, a, false))
        }

        // Patterns: File.size('path') == 0, >= 0, != 0; File.read/binread('path') == '', != ''
        // Also: !File.size('path') == 0 (where receiver is a ! call)
        "==" | ">=" | "!=" => {
            let recv = cx.call_receiver(node).get()?;
            let args = cx.call_arguments(node);
            if args.len() != 1 {
                return None;
            }
            let rhs = args[0];

            // Determine if recv is `!` applied to File.size/read (or not).
            let (file_class_src, arg_src, first_child_is_bang) =
                if let Some(inner) = is_bang_send(recv, cx) {
                    // recv is `!File.size(path)` — the actual receiver is inside the bang.
                    let (fc, a) = extract_file_size_or_read(inner, cx)?;
                    (fc, a, true)
                } else {
                    let (fc, a) = extract_file_size_or_read(recv, cx)?;
                    (fc, a, false)
                };

            // Check RHS: must be `0` for size patterns or `''` for read/binread patterns.
            if !rhs_is_zero_or_empty_string(rhs, cx) {
                return None;
            }

            // Bang rule:
            //   - `>=` or `!=` AND first_child is NOT `!` → needs bang
            //   - `==` AND first_child IS `!` → needs bang
            let needs_bang = match method {
                ">=" | "!=" => !first_child_is_bang,
                "==" => first_child_is_bang,
                _ => false,
            };

            Some((file_class_src, arg_src, needs_bang))
        }

        _ => None,
    }
}

/// If `node` is a `!` send (negation call), returns the receiver NodeId.
fn is_bang_send(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send { method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "!" {
        return None;
    }
    cx.call_receiver(node).get()
}

/// Extracts (file_class_src, arg_src) from a `File.size(path)`,
/// `File.read(path)`, or `File.binread(path)` send node.
fn extract_file_size_or_read<'a>(
    node: NodeId,
    cx: &'a Cx<'a>,
) -> Option<(&'a str, &'a str)> {
    let method = cx.method_name(node)?;
    if !matches!(method, "size" | "read" | "binread") {
        return None;
    }
    let recv = cx.call_receiver(node).get()?;
    let fc = file_class_src(recv, cx)?;
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    let arg_src = cx.raw_source(cx.range(args[0]));
    Some((fc, arg_src))
}

/// Returns the source string for `File` or `FileTest` receiver const,
/// if the node is a nil-scoped or cbase-scoped `File` or `FileTest` constant.
fn file_class_src<'a>(node: NodeId, cx: &'a Cx<'a>) -> Option<&'a str> {
    let NodeKind::Const { name, scope } = *cx.kind(node) else {
        return None;
    };
    let const_name = cx.symbol_str(name);
    if !matches!(const_name, "File" | "FileTest") {
        return None;
    }
    // Accept nil scope or cbase scope (::File / ::FileTest).
    let scope_ok = match scope.get() {
        None => true,
        Some(scope_id) => matches!(*cx.kind(scope_id), NodeKind::Cbase),
    };
    if !scope_ok {
        return None;
    }
    Some(cx.raw_source(cx.range(node)))
}

/// Returns true if the node is an integer `0` or an empty string `""`.
fn rhs_is_zero_or_empty_string(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Int(v) => v == 0,
        NodeKind::Str(string_id) => cx.string_str(string_id).is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::FileEmpty;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- File.zero? ---

    #[test]
    fn flags_file_zero() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.zero?('path/to/file')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "File.empty?('path/to/file')\n",
        );
    }

    // --- File.size == 0 ---

    #[test]
    fn flags_file_size_eq_zero() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.size('path/to/file') == 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "File.empty?('path/to/file')\n",
        );
    }

    // --- File.size >= 0 → !File.empty? ---

    #[test]
    fn flags_file_size_gte_zero() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.size('path/to/file') >= 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "!File.empty?('path/to/file')\n",
        );
    }

    // --- File.size.zero? ---

    #[test]
    fn flags_file_size_dot_zero() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.size('path/to/file').zero?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "File.empty?('path/to/file')\n",
        );
    }

    // --- File.read.empty? ---

    #[test]
    fn flags_file_read_dot_empty() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.read('path/to/file').empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "File.empty?('path/to/file')\n",
        );
    }

    // --- File.binread == '' ---

    #[test]
    fn flags_file_binread_eq_empty_str() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.binread('path/to/file') == ''
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "File.empty?('path/to/file')\n",
        );
    }

    // --- File.binread != '' → !File.empty? ---

    #[test]
    fn flags_file_binread_neq_empty_str() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                File.binread('path/to/file') != ''
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.empty?('path/to/file')` instead.
            "#},
            "!File.empty?('path/to/file')\n",
        );
    }

    // --- FileTest.zero? ---

    #[test]
    fn flags_filetest_zero() {
        test::<FileEmpty>().expect_correction(
            indoc! {r#"
                FileTest.zero?('path/to/file')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `FileTest.empty?('path/to/file')` instead.
            "#},
            "FileTest.empty?('path/to/file')\n",
        );
    }

    // --- allowed cases ---

    #[test]
    fn accepts_file_empty() {
        test::<FileEmpty>().expect_no_offenses("File.empty?('path/to/file')\n");
    }

    #[test]
    fn accepts_filetest_empty() {
        test::<FileEmpty>().expect_no_offenses("FileTest.empty?('path/to/file')\n");
    }

    #[test]
    fn accepts_non_file_zero() {
        test::<FileEmpty>().expect_no_offenses("x.zero?\n");
    }

    #[test]
    fn accepts_non_zero_size_comparison() {
        test::<FileEmpty>().expect_no_offenses("File.size('path') == 1\n");
    }
}
murphy_plugin_api::submit_cop!(FileEmpty);
