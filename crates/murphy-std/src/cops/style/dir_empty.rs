//! `Style/DirEmpty` — prefer `Dir.empty?(path)` over verbose patterns.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DirEmpty
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags patterns that can be replaced by `Dir.empty?`:
//!     - `Dir.entries(path).size == 2` / `!= 2` / `> 2`
//!     - `Dir.children(path).size == 0` / `!= 0` / `> 0`
//!     - `Dir.children(path).empty?`
//!     - `Dir.each_child(path).none?`
//!   Both `Dir` and `::Dir` receivers are accepted.
//!   Minimum Ruby version 2.4 (Murphy v1 does not gate on target_ruby_version).
//! ```
//!
//! ## Matched shapes
//!
//! See notes above. Autocorrect replaces the whole outer call.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Use `%s` instead.";

#[derive(Default)]
pub struct DirEmpty;

#[cop(
    name = "Style/DirEmpty",
    description = "Prefer `Dir.empty?` when checking if a directory is empty.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DirEmpty {
    #[on_node(kind = "send", methods = ["==", "!=", ">", "empty?", "none?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `(dir_const_node, arg_node, method_name)` if `node` is
/// `Dir.<method>(arg)` with exactly one arg (accepting `::Dir`).
fn match_dir_call(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId, String)> {
    let NodeKind::Send {
        receiver,
        method: sym,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };

    let recv = receiver.get()?;
    if !cx.is_global_const(recv, "Dir") {
        return None;
    }

    let method_name = cx.symbol_str(sym).to_owned();
    if !matches!(method_name.as_str(), "entries" | "children" | "each_child") {
        return None;
    }

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return None;
    }

    Some((recv, arg_list[0], method_name))
}

/// Returns `true` if `node` is an integer literal with the given value.
fn is_int(node: NodeId, value: i64, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Int(v) if *v == value)
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

    let outer_method = cx.symbol_str(sym).to_owned();

    match outer_method.as_str() {
        "empty?" => {
            // Dir.children(path).empty?
            let Some(recv) = receiver.get() else { return };
            let Some((dir_recv, arg_node, inner_method)) = match_dir_call(recv, cx) else {
                return;
            };
            if inner_method != "children" {
                return;
            }
            let dir_const_src = cx.raw_source(cx.range(dir_recv));
            let arg_src = cx.raw_source(cx.range(arg_node));
            let replacement = format!("{dir_const_src}.empty?({arg_src})");
            let msg = MSG.replace("%s", &replacement);
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), &replacement);
        }
        "none?" => {
            // Dir.each_child(path).none?
            let Some(recv) = receiver.get() else { return };
            let Some((dir_recv, arg_node, inner_method)) = match_dir_call(recv, cx) else {
                return;
            };
            if inner_method != "each_child" {
                return;
            }
            let dir_const_src = cx.raw_source(cx.range(dir_recv));
            let arg_src = cx.raw_source(cx.range(arg_node));
            let replacement = format!("{dir_const_src}.empty?({arg_src})");
            let msg = MSG.replace("%s", &replacement);
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), &replacement);
        }
        "==" | "!=" | ">" => {
            // Dir.entries(path).size == 2 / Dir.children(path).size == 0 / etc.
            let arg_list = cx.list(args);
            if arg_list.len() != 1 {
                return;
            }
            let int_node = arg_list[0];

            let Some(recv) = receiver.get() else { return };

            // receiver must be `<dir_call>.size`
            let NodeKind::Send {
                receiver: size_recv,
                method: size_sym,
                args: size_args,
            } = *cx.kind(recv)
            else {
                return;
            };
            if cx.symbol_str(size_sym) != "size" || !cx.list(size_args).is_empty() {
                return;
            }

            let Some(dir_recv_id) = size_recv.get() else {
                return;
            };
            let Some((dir_const_node, arg_node, inner_method)) =
                match_dir_call(dir_recv_id, cx)
            else {
                return;
            };

            // Determine bang and required int value based on method+inner.
            let (bang, required_int) = match (outer_method.as_str(), inner_method.as_str()) {
                ("==", "entries") => ("", 2i64),
                ("!=", "entries") => ("!", 2i64),
                (">", "entries") => ("!", 2i64),
                ("==", "children") => ("", 0i64),
                ("!=", "children") => ("!", 0i64),
                (">", "children") => ("!", 0i64),
                _ => return,
            };

            if !is_int(int_node, required_int, cx) {
                return;
            }

            let dir_const_src = cx.raw_source(cx.range(dir_const_node));
            let arg_src = cx.raw_source(cx.range(arg_node));
            let replacement = format!("{bang}{dir_const_src}.empty?({arg_src})");
            let msg = MSG.replace("%s", &replacement);
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), &replacement);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::DirEmpty;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_entries_size_eq_2() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.entries('path').size == 2
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.empty?('path')` instead.
            "#},
            "Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_entries_size_ne_2() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.entries('path').size != 2
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Dir.empty?('path')` instead.
            "#},
            "!Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_entries_size_gt_2() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.entries('path').size > 2
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Dir.empty?('path')` instead.
            "#},
            "!Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_children_size_eq_0() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.children('path').size == 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.empty?('path')` instead.
            "#},
            "Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_children_empty() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.children('path').empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.empty?('path')` instead.
            "#},
            "Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_each_child_none() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                Dir.each_child('path').none?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.empty?('path')` instead.
            "#},
            "Dir.empty?('path')\n",
        );
    }

    #[test]
    fn flags_qualified_dir_entries() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                ::Dir.entries('path').size == 2
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `::Dir.empty?('path')` instead.
            "#},
            "::Dir.empty?('path')\n",
        );
    }

    #[test]
    fn accepts_dir_empty_already() {
        test::<DirEmpty>().expect_no_offenses("Dir.empty?('path')\n");
    }

    #[test]
    fn accepts_entries_size_eq_3() {
        test::<DirEmpty>().expect_no_offenses("Dir.entries('path').size == 3\n");
    }

    #[test]
    fn accepts_entries_size_eq_0() {
        // entries with 0 is not matched (only children with 0)
        test::<DirEmpty>().expect_no_offenses("Dir.entries('path').size == 0\n");
    }
}

murphy_plugin_api::submit_cop!(DirEmpty);
