//! `Style/Dir` — replace verbose `File.expand_path(File.dirname(__FILE__))`
//! and `File.dirname(File.realpath(__FILE__))` with `__dir__`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Dir
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags two patterns:
//!     1. `File.expand_path(File.dirname(__FILE__))` → `__dir__`
//!     2. `File.dirname(File.realpath(__FILE__))` → `__dir__`
//!   Both `File` and `::File` receivers are accepted.
//!   Autocorrect replaces the whole outer call with `__dir__`.
//!   Minimum Ruby version 2.0 (Murphy v1 does not gate on target_ruby_version).
//! ```
//!
//! ## Matched shapes
//!
//! 1. `File.expand_path(File.dirname(__FILE__))`  — outer `expand_path`, inner `dirname`
//! 2. `File.dirname(File.realpath(__FILE__))` — outer `dirname`, inner `realpath`
//!
//! ## Autocorrect
//!
//! Replaces the entire outer call node with `__dir__`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Use `__dir__` to get an absolute path to the current file's directory.";

#[derive(Default)]
pub struct Dir;

#[cop(
    name = "Style/Dir",
    description = "Use `__dir__` to get an absolute path to the current file's directory.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Dir {
    #[on_node(kind = "send", methods = ["expand_path", "dirname"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` if `node` is `File.<method>(__FILE__)` (accepting `::File`),
/// with exactly one argument which is `__FILE__`.
fn is_file_call(node: NodeId, method: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method: sym,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };

    let Some(recv) = receiver.get() else {
        return false;
    };
    if !cx.is_global_const(recv, "File") {
        return false;
    }
    if cx.symbol_str(sym) != method {
        return false;
    }
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return false;
    }
    is_file_keyword(arg_list[0], cx)
}

/// Returns `true` if `node` represents `__FILE__`.
fn is_file_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Unknown)
        && cx.raw_source(cx.range(node)) == "__FILE__"
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method: sym,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    let method_name = cx.symbol_str(sym);

    let matches = match method_name {
        "expand_path" => {
            // File.expand_path(File.dirname(__FILE__))
            if let Some(recv) = receiver.get() {
                if cx.is_global_const(recv, "File") {
                    let arg_list = cx.list(args);
                    arg_list.len() == 1 && is_file_call(arg_list[0], "dirname", cx)
                } else {
                    false
                }
            } else {
                false
            }
        }
        "dirname" => {
            // File.dirname(File.realpath(__FILE__))
            if let Some(recv) = receiver.get() {
                if cx.is_global_const(recv, "File") {
                    let arg_list = cx.list(args);
                    arg_list.len() == 1 && is_file_call(arg_list[0], "realpath", cx)
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };

    if !matches {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);
    cx.emit_edit(cx.range(node), "__dir__");
}

#[cfg(test)]
mod tests {
    use super::Dir;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expand_path_dirname_file() {
        test::<Dir>().expect_correction(
            indoc! {r#"
                path = File.expand_path(File.dirname(__FILE__))
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `__dir__` to get an absolute path to the current file's directory.
            "#},
            "path = __dir__\n",
        );
    }

    #[test]
    fn flags_qualified_expand_path_dirname() {
        test::<Dir>().expect_correction(
            indoc! {r#"
                path = ::File.expand_path(::File.dirname(__FILE__))
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `__dir__` to get an absolute path to the current file's directory.
            "#},
            "path = __dir__\n",
        );
    }

    #[test]
    fn flags_dirname_realpath_file() {
        test::<Dir>().expect_correction(
            indoc! {r#"
                path = File.dirname(File.realpath(__FILE__))
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `__dir__` to get an absolute path to the current file's directory.
            "#},
            "path = __dir__\n",
        );
    }

    #[test]
    fn accepts_dir_already() {
        test::<Dir>().expect_no_offenses("path = __dir__\n");
    }

    #[test]
    fn accepts_expand_path_without_dirname() {
        test::<Dir>().expect_no_offenses("path = File.expand_path('relative_path')\n");
    }

    #[test]
    fn accepts_dirname_with_regular_string() {
        test::<Dir>().expect_no_offenses("path = File.dirname('/some/path')\n");
    }

    #[test]
    fn accepts_dirname_realpath_non_file_receiver() {
        test::<Dir>().expect_no_offenses("path = File.dirname(Foo.realpath(__FILE__))\n");
    }

    #[test]
    fn accepts_expand_path_dirname_non_file_const() {
        test::<Dir>().expect_no_offenses("path = File.expand_path(Foo.dirname(__FILE__))\n");
    }
}

murphy_plugin_api::submit_cop!(Dir);
