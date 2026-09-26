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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

const MSG: &str = "Use `__dir__` to get an absolute path to the current file's directory.";

// RuboCop parity: `Style/Dir` `dir_replacement?` is
// `{(send (const {nil? cbase} :File) :expand_path (send (const {nil? cbase} :File) :dirname #file_keyword?))
//   (send (const {nil? cbase} :File) :dirname (send (const {nil? cbase} :File) :realpath #file_keyword?))}`
// (send-only, top-level only, exactly 1 arg per call — no `...`).
// A top-level `{arm arm}` union is not expressible in v1 (variable-length
// child lists are rejected inside `{}`), so each RuboCop union arm becomes
// its own matcher below — each arm verbatim.
// In Murphy `::File` collapses to Const scope None, so `nil?` covers bare +
// `::` (pinned by `flags_qualified_expand_path_dirname` +
// `boundary_flags_cbase_dirname_realpath`); namespaced `Foo::File` still
// rejects (pinned by `boundary_ignores_namespaced_file` +
// `boundary_ignores_mixed_namespaced_inner`); send-only dispatch keeps `&.` inners/outer silent (pinned by
// `boundary_ignores_csend_outer` + `boundary_ignores_csend_inner`);
// `#file_keyword?` calls the free `file_keyword_p` below (body carried over
// verbatim from the hand-rolled `is_file_keyword`: `__FILE__` is Unknown-kind
// with that source; a plain string stays silent, pinned by
// `boundary_ignores_string_leaf`).
// Predicate-only, no captures, byte-identical offense emission.
def_node_matcher!(
    dir_expand_path_replacement,
    "(send (const nil? :File) :expand_path (send (const nil? :File) :dirname #file_keyword?))"
);
def_node_matcher!(
    dir_dirname_replacement,
    "(send (const nil? :File) :dirname (send (const nil? :File) :realpath #file_keyword?))"
);

/// Returns `true` if `node` represents `__FILE__`.
fn file_keyword_p(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Unknown)
        && cx.raw_source(cx.range(node)) == "__FILE__"
}

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

fn check(node: NodeId, cx: &Cx<'_>) {
    if !dir_expand_path_replacement(node, cx) && !dir_dirname_replacement(node, cx) {
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

    // --- Boundary characterization (murphy-ft88.23): pin the exact node set
    // the hand-rolled `is_global_const(File)` + arity-1 + `is_file_keyword`
    // checks match, so the verbatim
    // `{(send (const nil? :File) :expand_path (send (const nil? :File) :dirname #file_keyword?))
    //   (send (const nil? :File) :dirname (send (const nil? :File) :realpath #file_keyword?))}`
    // refactor can be proven equivalent. `::File` collapses to Const scope
    // None, so `nil?` covers bare + `::` (pinned by
    // `flags_qualified_expand_path_dirname`); namespaced `Foo::File` still
    // rejects; inner/outer `&.` are csend and the cop only handles `send`,
    // so they stay silent; exactly 1 arg (no `...`) so 2-arg outers stay
    // silent; the leaf must be `__FILE__` (a string stays silent).

    #[test]
    fn boundary_ignores_namespaced_file() {
        test::<Dir>().expect_no_offenses("path = Foo::File.expand_path(Foo::File.dirname(__FILE__))\n");
    }

    #[test]
    fn boundary_ignores_mixed_namespaced_inner() {
        test::<Dir>().expect_no_offenses("path = File.expand_path(Foo::File.dirname(__FILE__))\n");
    }

    #[test]
    fn boundary_ignores_csend_outer() {
        test::<Dir>().expect_no_offenses("path = File&.expand_path(File.dirname(__FILE__))\n");
    }

    #[test]
    fn boundary_ignores_csend_inner() {
        test::<Dir>().expect_no_offenses("path = File.expand_path(File&.dirname(__FILE__))\n");
    }

    #[test]
    fn boundary_ignores_extra_outer_arg() {
        test::<Dir>().expect_no_offenses("path = File.expand_path(File.dirname(__FILE__), __dir__)\n");
    }

    #[test]
    fn boundary_ignores_string_leaf() {
        test::<Dir>().expect_no_offenses("path = File.expand_path(File.dirname('__FILE__'))\n");
    }

    #[test]
    fn boundary_flags_cbase_dirname_realpath() {
        test::<Dir>().expect_correction(
            indoc! {r#"
                path = ::File.dirname(::File.realpath(__FILE__))
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `__dir__` to get an absolute path to the current file's directory.
            "#},
            "path = __dir__\n",
        );
    }
}

murphy_plugin_api::submit_cop!(Dir);
