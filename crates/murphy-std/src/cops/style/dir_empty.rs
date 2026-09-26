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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

const MSG: &str = "Use `%s` instead.";

// RuboCop parity: `Style/DirEmpty` `offensive?` is
// `{(send (send (send $(const {nil? cbase} :Dir) :entries $_) :size) {:== :!= :>} (int 2))
//   (send (send (send $(const {nil? cbase} :Dir) :children $_) :size) {:== :!= :>} (int 0))
//   (send (send $(const {nil? cbase} :Dir) :children $_) :empty?)
//   (send (send $(const {nil? cbase} :Dir) :each_child $_) :none?)}`
// (send-only, top-level only; `(int N)` is the bare literal `N` in Murphy;
// the `:size` middles take no args).
// A top-level `{...}` union is not expressible in v1 (`$` captures and
// variable-length child lists are both rejected inside `{}`), so each
// RuboCop union arm becomes its own matcher below — each arm verbatim,
// capturing the Dir const node (slot 0, for the `::Dir`-preserving
// replacement) and the path arg (slot 1).
// In Murphy `::Dir` collapses to Const scope None, so `nil?` covers bare +
// `::` (pinned by `flags_qualified_dir_entries` +
// `boundary_flags_cbase_each_child_none`); namespaced `Foo::Dir` still
// rejects (pinned by `boundary_ignores_namespaced_dir`); send-only dispatch
// keeps `&.` inners/outer silent (pinned by `boundary_ignores_csend_inner` +
// `boundary_ignores_csend_outer`); `.size` must be argument-free (pinned by
// `boundary_ignores_size_with_args`); `each_child` has no size-comparison arm
// (pinned by `boundary_ignores_each_child_size_cmp`), `entries` has no
// `empty?` arm (pinned by `boundary_ignores_entries_empty`), and children
// size must be 0 (pinned by `boundary_ignores_children_size_eq_2`).
// The `!` bang for `!=`/`>` and message construction stay hand-rolled below.
// Capture-bearing, byte-identical offense emission.
def_node_matcher!(
    dir_entries_size_cmp,
    "(send (send (send $(const nil? :Dir) :entries $_) :size) {:== :!= :>} 2)"
);
def_node_matcher!(
    dir_children_size_cmp,
    "(send (send (send $(const nil? :Dir) :children $_) :size) {:== :!= :>} 0)"
);
def_node_matcher!(
    dir_children_empty,
    "(send (send $(const nil? :Dir) :children $_) :empty?)"
);
def_node_matcher!(
    dir_each_child_none,
    "(send (send $(const nil? :Dir) :each_child $_) :none?)"
);

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

/// Emits the `Dir.empty?(arg)` offense, preserving the `::Dir` spelling
/// of the matched const node.
fn emit(dir_const_node: NodeId, arg_node: NodeId, bang: &str, node: NodeId, cx: &Cx<'_>) {
    let dir_const_src = cx.raw_source(cx.range(dir_const_node));
    let arg_src = cx.raw_source(cx.range(arg_node));
    let replacement = format!("{bang}{dir_const_src}.empty?({arg_src})");
    let msg = MSG.replace("%s", &replacement);
    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(cx.range(node), &replacement);
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { method: sym, .. } = *cx.kind(node) else {
        return;
    };

    let outer_method = cx.symbol_str(sym).to_owned();

    match outer_method.as_str() {
        "empty?" => {
            // `(send (send $(const nil? :Dir) :children $_) :empty?)`
            let Some((dir_const_node, arg_node)) = dir_children_empty(node, cx) else {
                return;
            };
            emit(dir_const_node, arg_node, "", node, cx);
        }
        "none?" => {
            // `(send (send $(const nil? :Dir) :each_child $_) :none?)`
            let Some((dir_const_node, arg_node)) = dir_each_child_none(node, cx) else {
                return;
            };
            emit(dir_const_node, arg_node, "", node, cx);
        }
        "==" | "!=" | ">" => {
            // `(send (send (send $(const nil? :Dir) :entries $_) :size) {:== :!= :>} 2)` /
            // `(send (send (send $(const nil? :Dir) :children $_) :size) {:== :!= :>} 0)`
            // — the int value is pinned by the pattern, so only the `!`
            // bang for `!=`/`>` stays hand-rolled.
            let bang = if outer_method == "!=" || outer_method == ">" {
                "!"
            } else {
                ""
            };
            if let Some((dir_const_node, arg_node)) = dir_entries_size_cmp(node, cx) {
                emit(dir_const_node, arg_node, bang, node, cx);
            } else if let Some((dir_const_node, arg_node)) = dir_children_size_cmp(node, cx) {
                emit(dir_const_node, arg_node, bang, node, cx);
            }
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

    // --- Boundary characterization (murphy-ft88.23): pin the exact node set
    // the hand-rolled `match_dir_call` + `.size`-arity + int-value checks
    // match, so the verbatim per-arm
    // `(send (send (send $(const nil? :Dir) :entries $_) :size) {:== :!= :>} 2)` /
    // `(send (send (send $(const nil? :Dir) :children $_) :size) {:== :!= :>} 0)` /
    // `(send (send $(const nil? :Dir) :children $_) :empty?)` /
    // `(send (send $(const nil? :Dir) :each_child $_) :none?)`
    // refactor can be proven equivalent. `::Dir` collapses to Const scope
    // None, so `nil?` covers bare + `::` (pinned by
    // `flags_qualified_dir_entries` + new `boundary_flags_cbase_each_child_none`);
    // namespaced `Foo::Dir` still rejects; inner/outer `&.` are csend and the
    // cop only handles `send`, so they stay silent; `.size` must be
    // argument-free; `each_child` has no size-comparison arm and `entries`
    // has no `empty?` arm; children size must be 0 (2 stays silent).

    #[test]
    fn boundary_flags_cbase_each_child_none() {
        test::<DirEmpty>().expect_correction(
            indoc! {r#"
                ::Dir.each_child('path').none?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `::Dir.empty?('path')` instead.
            "#},
            "::Dir.empty?('path')\n",
        );
    }

    #[test]
    fn boundary_ignores_namespaced_dir() {
        test::<DirEmpty>().expect_no_offenses("Foo::Dir.entries('path').size == 2\n");
    }

    #[test]
    fn boundary_ignores_csend_inner() {
        test::<DirEmpty>().expect_no_offenses("Dir&.entries('path').size == 2\n");
    }

    #[test]
    fn boundary_ignores_csend_outer() {
        test::<DirEmpty>().expect_no_offenses("Dir.children('path')&.empty?\n");
    }

    #[test]
    fn boundary_ignores_size_with_args() {
        test::<DirEmpty>().expect_no_offenses("Dir.entries('path').size(1) == 2\n");
    }

    #[test]
    fn boundary_ignores_each_child_size_cmp() {
        // each_child has no size-comparison arm in RuboCop's pattern.
        test::<DirEmpty>().expect_no_offenses("Dir.each_child('path').size == 0\n");
    }

    #[test]
    fn boundary_ignores_entries_empty() {
        // entries has no `empty?` arm in RuboCop's pattern.
        test::<DirEmpty>().expect_no_offenses("Dir.entries('path').empty?\n");
    }

    #[test]
    fn boundary_ignores_children_size_eq_2() {
        // children size must be 0; 2 stays silent.
        test::<DirEmpty>().expect_no_offenses("Dir.children('path').size == 2\n");
    }
}

murphy_plugin_api::submit_cop!(DirEmpty);
