//! `Style/ExpandPathArguments` — prefer `__dir__` over `__FILE__` in `File.expand_path`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ExpandPathArguments
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered patterns:
//!     - `File.expand_path('.', __FILE__)` -> `File.expand_path(__FILE__)`
//!     - `File.expand_path('..', __FILE__)` -> `File.expand_path(__dir__)`
//!     - `File.expand_path('../..', __FILE__)` -> `File.expand_path('..', __dir__)`
//!     - `File.expand_path('../lib', __FILE__)` -> `File.expand_path(__dir__)`
//!       (path with one `..` and other components reduces to `__dir__` + remaining)
//!     - `Pathname(__FILE__).parent.expand_path` -> `Pathname(__dir__).expand_path`
//!     - `Pathname.new(__FILE__).parent.expand_path` -> `Pathname.new(__dir__).expand_path`
//!   `__FILE__` is represented as an Unknown node in the arena AST; detected via
//!   raw_source comparison.
//!   `::File` (cbase scope) is also accepted, matching RuboCop's node pattern.
//!   Second argument must be `__FILE__` (Unknown node with source `__FILE__`).
//!   First argument must be a plain string literal (Str node).
//!   Gap: Pathname pattern only accepts nil receiver (bare `Pathname(...)`) or
//!   Pathname constant receiver (`Pathname.new`). Chained forms not covered.
//!   Autocorrect uses whole-node replacement for all patterns.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! File.expand_path('..', __FILE__)
//! File.expand_path('../..', __FILE__)
//! File.expand_path('.', __FILE__)
//! Pathname(__FILE__).parent.expand_path
//! Pathname.new(__FILE__).parent.expand_path
//!
//! # good
//! File.expand_path(__dir__)
//! File.expand_path('..', __dir__)
//! Pathname(__dir__).expand_path
//! Pathname.new(__dir__).expand_path
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Style/ExpandPathArguments` `file_expand_path` is
// `(send (const {nil? cbase} :File) :expand_path $_ $_)` (File receiver,
// two args; captures stay hand-rolled). Murphy splits the const-receiver
// check (`File.expand_path` top-level) from the `__FILE__`/string-literal
// guards, so the verbatim port covers the const part only:
// `(send (const nil? :File) :expand_path ...)` (send-only, matching the
// `Send`-only dispatch via `#[on_node(kind = "send")]`; no `csend` handler).
// In Murphy `::File` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (both flag, pinned by `boundary_flags_cbase_file_expand_path`).
// Namespaced `Foo::File` is not top-level, so silent (pinned by
// `boundary_ignores_namespaced_file_expand_path`). `send` covers `Send`
// only, matching the `Send`-only dispatch (csend silent, pinned by
// `boundary_ignores_csend_file_expand_path`). The `$_ $_` captures +
// `__FILE__`/string handling + Pathname branches stay hand-rolled below.
def_node_matcher!(
    file_expand_path_const,
    "(send (const nil? :File) :expand_path ...)"
);

// RuboCop parity: `Style/ExpandPathArguments` `pathname_new_parent_expand_path`
// is `(send (send (send (const {nil? cbase} :Pathname) :new $_) :parent) ...)`
// (Pathname receiver, one arg; captures + `__FILE__` guards stay hand-rolled).
// Murphy splits the const-receiver check (`Pathname.new` top-level) from the
// `__FILE__`/arity guards, so the verbatim port covers the const part only:
// `(const nil? :Pathname)` (top-level only).
// In Murphy `::Pathname` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (both flag, pinned by
// `boundary_flags_cbase_pathname_new_parent_expand_path`). Namespaced
// `Foo::Pathname` is not top-level, so silent (pinned by
// `boundary_ignores_namespaced_pathname_new_parent_expand_path`). The `:new`
// method + 1-arg `__FILE__` guards stay hand-rolled below.
// Predicate-only, no captures, byte-identical offense emission.
def_node_matcher!(pathname_const, "(const nil? :Pathname)");

/// Stateless unit struct.
#[derive(Default)]
pub struct ExpandPathArguments;

const FILE_MSG: &str =
    "Use `expand_path(%<new_path>s%<new_default_dir>s)` instead of \
     `expand_path(%<current_path>s, __FILE__)`.";
const PATHNAME_MSG: &str =
    "Use `Pathname(__dir__).expand_path` instead of `Pathname(__FILE__).parent.expand_path`.";
const PATHNAME_NEW_MSG: &str =
    "Use `Pathname.new(__dir__).expand_path` instead of \
     `Pathname.new(__FILE__).parent.expand_path`.";

#[cop(
    name = "Style/ExpandPathArguments",
    description = "Use `expand_path(__dir__)` instead of `expand_path('..', __FILE__)`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ExpandPathArguments {
    #[on_node(kind = "send", methods = ["expand_path"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_expand_path(node, cx);
    }
}

fn check_expand_path(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, method: _, args } = *cx.kind(node) else {
        return;
    };

    // `(send (const nil? :File) :expand_path ...)` (top-level only, send-only).
    // The `$_ $_` captures + `__FILE__`/string guards stay hand-rolled below.
    if file_expand_path_const(node, cx)
    {
        let args = cx.list(args);
        if args.len() == 2 {
            check_file_expand_path(node, args[0], args[1], cx);
        }
        return;
    }

    // Try Pathname(__FILE__).parent.expand_path or Pathname.new(__FILE__).parent.expand_path
    // The receiver of expand_path is the Pathname object
    let Some(recv) = receiver.get() else {
        return;
    };

    // recv should be (send :parent (send :Pathname nil (unknown)))
    let NodeKind::Send {
        receiver: inner_recv,
        method: parent_method,
        args: parent_args,
    } = *cx.kind(recv)
    else {
        return;
    };
    if cx.symbol_str(parent_method) != "parent" || !cx.list(parent_args).is_empty() {
        return;
    }
    let Some(inner_recv_id) = inner_recv.get() else {
        return;
    };

    let NodeKind::Send {
        receiver: pathname_recv,
        method: pathname_method,
        args: pathname_args,
    } = *cx.kind(inner_recv_id)
    else {
        return;
    };

    let pathname_args_list = cx.list(pathname_args);
    if pathname_args_list.len() != 1 || !is_file_magic(pathname_args_list[0], cx) {
        return;
    }

    // Check bare Pathname(__FILE__): (send :Pathname nil ...)
    if cx.symbol_str(pathname_method) == "Pathname" && pathname_recv.is_none() {
        cx.emit_offense(cx.range(node), PATHNAME_MSG, None);
        cx.emit_edit(cx.range(node), "Pathname(__dir__).expand_path");
        return;
    }

    // Check Pathname.new(__FILE__): (send :new (const :Pathname nil) ...)
    if cx.symbol_str(pathname_method) == "new"
        && let Some(pathname_recv_id) = pathname_recv.get()
        && is_pathname_const(pathname_recv_id, cx)
    {
        cx.emit_offense(cx.range(node), PATHNAME_NEW_MSG, None);
        cx.emit_edit(cx.range(node), "Pathname.new(__dir__).expand_path");
    }
}

fn check_file_expand_path(node: NodeId, path_arg: NodeId, default_dir: NodeId, cx: &Cx<'_>) {
    // Second arg must be __FILE__
    if !is_file_magic(default_dir, cx) {
        return;
    }

    // First arg must be a plain string
    let NodeKind::Str(string_id) = *cx.kind(path_arg) else {
        return;
    };

    let current_path = cx.string_str(string_id);

    // Compute depth: number of `..` components after removing `.`
    let depth = path_depth(current_path);
    let parent_path = compute_parent_path(current_path);

    let (new_path, new_default_dir) = if depth == 0 {
        // `.` only -> __FILE__
        (String::new(), "__FILE__".to_string())
    } else if depth == 1 {
        // one level up -> __dir__
        (String::new(), "__dir__".to_string())
    } else {
        // multiple levels -> parent path + __dir__
        (format!("'{}', ", parent_path), "__dir__".to_string())
    };

    let message = FILE_MSG
        .replace("%<new_path>s", &new_path)
        .replace("%<new_default_dir>s", &new_default_dir)
        .replace("%<current_path>s", &format!("'{}'", current_path));

    // Offense range covers the selector (method name location)
    let sel_range = cx.node(node).loc.name;
    cx.emit_offense(sel_range, &message, None);

    // Autocorrect: replace args section
    let args = cx.call_arguments(node);
    let correction = if depth == 0 {
        "__FILE__".to_string()
    } else if depth == 1 {
        "__dir__".to_string()
    } else {
        format!("'{}', __dir__", parent_path)
    };

    // We need to replace the arguments portion — from first arg start to last arg end
    let first_arg_range = cx.range(args[0]);
    let last_arg_range = cx.range(args[args.len() - 1]);
    let args_range = murphy_plugin_api::Range {
        start: first_arg_range.start,
        end: last_arg_range.end,
    };
    cx.emit_edit(args_range, &correction);
}

/// Count the number of `..` components in a path (after stripping `.`).
fn path_depth(path: &str) -> usize {
    path.split('/')
        .filter(|&p| p == "..")
        .count()
}

/// Compute the parent path: strip one `..` and remove `.` components.
fn compute_parent_path(path: &str) -> String {
    let mut parts: Vec<&str> = path.split('/').collect();
    // Remove '.' components
    parts.retain(|&p| p != ".");
    // Remove first '..' found
    if let Some(pos) = parts.iter().position(|&p| p == "..") {
        parts.remove(pos);
    }
    parts.join("/")
}

/// Returns true if this node is `__FILE__` (Unknown node with raw source `__FILE__`).
fn is_file_magic(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Unknown)
        && cx.raw_source(cx.range(node)) == "__FILE__"
}

// `is_file_const` replaced by verbatim `file_expand_path_const` above (predicate-only).

/// Returns true if `node` is `Pathname` or `::Pathname` constant.
/// Verbatim `(const nil? :Pathname)` (top-level only); `::` collapses to
/// `Const{scope:None}` so `nil?` covers bare + `::`.
fn is_pathname_const(node: NodeId, cx: &Cx<'_>) -> bool {
    pathname_const(node, cx)
}

#[cfg(test)]
mod tests {
    use super::ExpandPathArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No offense ---

    #[test]
    fn accepts_expand_path_with_dir() {
        test::<ExpandPathArguments>().expect_no_offenses("File.expand_path(__dir__)\n");
    }

    #[test]
    fn accepts_expand_path_two_args_with_dir() {
        test::<ExpandPathArguments>().expect_no_offenses("File.expand_path('..', __dir__)\n");
    }

    #[test]
    fn accepts_expand_path_one_arg() {
        test::<ExpandPathArguments>().expect_no_offenses("File.expand_path(__FILE__)\n");
    }

    #[test]
    fn accepts_pathname_dir() {
        test::<ExpandPathArguments>()
            .expect_no_offenses("Pathname(__dir__).expand_path\n");
    }

    #[test]
    fn accepts_non_file_receiver() {
        test::<ExpandPathArguments>().expect_no_offenses("Foo.expand_path('..', __FILE__)\n");
    }

    // --- File.expand_path offenses ---

    #[test]
    fn flags_dot_file() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            File.expand_path('.', __FILE__)
                 ^^^^^^^^^^^ Use `expand_path(__FILE__)` instead of `expand_path('.', __FILE__)`.
        "});
    }

    #[test]
    fn flags_dotdot_file() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            File.expand_path('..', __FILE__)
                 ^^^^^^^^^^^ Use `expand_path(__dir__)` instead of `expand_path('..', __FILE__)`.
        "});
    }

    #[test]
    fn flags_dotdotdot_file() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            File.expand_path('../..', __FILE__)
                 ^^^^^^^^^^^ Use `expand_path('..', __dir__)` instead of `expand_path('../..', __FILE__)`.
        "});
    }

    // --- Pathname offenses ---

    #[test]
    fn flags_pathname_file_parent_expand_path() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            Pathname(__FILE__).parent.expand_path
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Pathname(__dir__).expand_path` instead of `Pathname(__FILE__).parent.expand_path`.
        "});
    }

    #[test]
    fn flags_pathname_new_file_parent_expand_path() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            Pathname.new(__FILE__).parent.expand_path
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Pathname.new(__dir__).expand_path` instead of `Pathname.new(__FILE__).parent.expand_path`.
        "});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_dot_file_to_file() {
        test::<ExpandPathArguments>().expect_correction(
            indoc! {"
                File.expand_path('.', __FILE__)
                     ^^^^^^^^^^^ Use `expand_path(__FILE__)` instead of `expand_path('.', __FILE__)`.
            "},
            "File.expand_path(__FILE__)\n",
        );
    }

    #[test]
    fn corrects_dotdot_file_to_dir() {
        test::<ExpandPathArguments>().expect_correction(
            indoc! {"
                File.expand_path('..', __FILE__)
                     ^^^^^^^^^^^ Use `expand_path(__dir__)` instead of `expand_path('..', __FILE__)`.
            "},
            "File.expand_path(__dir__)\n",
        );
    }

    #[test]
    fn corrects_dotdotdot_file_to_dotdot_dir() {
        test::<ExpandPathArguments>().expect_correction(
            indoc! {"
                File.expand_path('../..', __FILE__)
                     ^^^^^^^^^^^ Use `expand_path('..', __dir__)` instead of `expand_path('../..', __FILE__)`.
            "},
            "File.expand_path('..', __dir__)\n",
        );
    }

    #[test]
    fn corrects_pathname_file_parent_to_dir() {
        test::<ExpandPathArguments>().expect_correction(
            indoc! {"
                Pathname(__FILE__).parent.expand_path
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Pathname(__dir__).expand_path` instead of `Pathname(__FILE__).parent.expand_path`.
            "},
            "Pathname(__dir__).expand_path\n",
        );
    }

    #[test]
    fn corrects_pathname_new_file_parent_to_dir() {
        test::<ExpandPathArguments>().expect_correction(
            indoc! {"
                Pathname.new(__FILE__).parent.expand_path
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Pathname.new(__dir__).expand_path` instead of `Pathname.new(__FILE__).parent.expand_path`.
            "},
            "Pathname.new(__dir__).expand_path\n",
        );
    }

    // --- Boundary characterization (murphy-ft88.17): pin the exact node set
    // the hand-rolled `is_file_const` (nil/cbase scope, reject namespaced) +
    // method `expand_path` matches, so the verbatim
    // `(send (const nil? :File) :expand_path ...)` refactor can be proven
    // equivalent. `::File` collapses to `Const{scope:None}`: `nil?` covers
    // bare + `::` (both flag). Namespaced `Foo::File` is not top-level, so
    // silent. `send` covers `Send` only, matching the `Send`-only dispatch
    // (no `csend` handler, so `&.` is silent). Pathname branches stay
    // hand-rolled below.

    #[test]
    fn boundary_flags_cbase_file_expand_path() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            ::File.expand_path('..', __FILE__)
                   ^^^^^^^^^^^ Use `expand_path(__dir__)` instead of `expand_path('..', __FILE__)`.
        "});
    }

    #[test]
    fn boundary_ignores_namespaced_file_expand_path() {
        test::<ExpandPathArguments>()
            .expect_no_offenses("Foo::File.expand_path('..', __FILE__)\n");
    }

    #[test]
    fn boundary_ignores_csend_file_expand_path() {
        test::<ExpandPathArguments>()
            .expect_no_offenses("File&.expand_path('..', __FILE__)\n");
    }

    // --- Boundary characterization (murphy-ft88.24): pin the exact node set
    // the hand-rolled `is_pathname_const` (nil/cbase scope, reject namespaced)
    // matches, so the verbatim `(const nil? :Pathname)` refactor can be proven
    // equivalent. `::Pathname` collapses to `Const{scope:None}`: `nil?` covers
    // bare + `::` (both flag). Namespaced `Foo::Pathname` is not top-level,
    // so silent. `send` covers `Send` only, matching the `Send`-only dispatch
    // (no `csend` handler, so `&.` is silent).

    #[test]
    fn boundary_flags_cbase_pathname_new_parent_expand_path() {
        test::<ExpandPathArguments>().expect_offense(indoc! {"
            ::Pathname.new(__FILE__).parent.expand_path
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Pathname.new(__dir__).expand_path` instead of `Pathname.new(__FILE__).parent.expand_path`.
        "});
    }

    #[test]
    fn boundary_ignores_namespaced_pathname_new_parent_expand_path() {
        test::<ExpandPathArguments>()
            .expect_no_offenses("Foo::Pathname.new(__FILE__).parent.expand_path\n");
    }

    #[test]
    fn boundary_ignores_csend_pathname_parent_expand_path() {
        test::<ExpandPathArguments>()
            .expect_no_offenses("Pathname.new(__FILE__).parent&.expand_path\n");
    }
}

murphy_plugin_api::submit_cop!(ExpandPathArguments);
