//! `Style/EnvHome` — prefer `Dir.home` over `ENV['HOME']`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EnvHome
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `ENV['HOME']` and `ENV.fetch('HOME', nil)` and suggests `Dir.home`.
//!   Marked unsafe upstream because assigning `nil` to `ENV['HOME']` differs
//!   from `Dir.home`. The cop is `pending` by default in RuboCop (Enabled:
//!   pending); Murphy follows the same default_enabled = false.
//!
//!   Covered:
//!     - `ENV['HOME']` → `Dir.home`
//!     - `ENV.fetch('HOME', nil)` → `Dir.home`
//!     - `ENV.fetch('HOME')` → not flagged (raises KeyError; different semantics)
//!     - `ENV.fetch('HOME', default)` where default is non-nil → not flagged
//!   Autocorrect: replace the entire send node with `Dir.home`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! ENV['HOME']
//! ENV.fetch('HOME', nil)
//!
//! # good
//! Dir.home
//! ENV.fetch('HOME')      # raises KeyError if unset — different semantics
//! ENV.fetch('HOME', '/') # non-nil default
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Style/EnvHome` receiver guard is `ENV` top-level
// (`(const {nil? cbase} :ENV)`), methods `{:[] :fetch}`.
// In Murphy `::ENV` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (pinned by `boundary_flags_cbase_env_bracket_home` /
// `boundary_flags_cbase_env_fetch_home_nil`). Namespaced `Foo::ENV` still
// accepts (pinned by `boundary_accepts_namespaced_*`). `send` covers `Send`
// only (not `Csend`), matching the `#[on_node(kind = "send")]` dispatch
// (pinned by `boundary_accepts_csend_*`). `HOME` string + `nil` second-arg
// guards stay hand-rolled below.
def_node_matcher!(
    is_env_home_send,
    "(send (const nil? :ENV) {:[] :fetch} ...)"
);

const MSG: &str = "Use `Dir.home` instead of `ENV['HOME']`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EnvHome;

#[cop(
    name = "Style/EnvHome",
    description = "Checks for consistent usage of `ENV['HOME']`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl EnvHome {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `(send (const nil? :ENV) {:[] :fetch} ...)` (`ENV` / `::ENV`,
    // top-level only). `send` covers `Send` only (not `Csend`).
    if !is_env_home_send(node, cx) {
        return;
    }

    let NodeKind::Send {
        method, args, ..
    } = *cx.kind(node)
    else {
        return;
    };

    let method_str = cx.symbol_str(method);
    let arg_list = cx.list(args);

    match method_str {
        "[]" if arg_list.len() == 1 && is_str_home(arg_list[0], cx) => {
            // `ENV['HOME']` — single argument must be the string "HOME".
            cx.emit_offense(cx.range(node), MSG, None);
            cx.emit_edit(cx.range(node), "Dir.home");
        }
        "fetch" => {
            // `ENV.fetch('HOME')` — no second arg: different semantics, skip.
            // `ENV.fetch('HOME', nil)` — second arg is nil: flag it.
            // `ENV.fetch('HOME', default)` — non-nil second arg: skip.
            if arg_list.len() < 2 {
                return;
            }
            if !is_str_home(arg_list[0], cx) {
                return;
            }
            // Second argument must be nil.
            if !matches!(cx.kind(arg_list[1]), NodeKind::Nil) {
                return;
            }
            cx.emit_offense(cx.range(node), MSG, None);
            cx.emit_edit(cx.range(node), "Dir.home");
        }
        _ => {}
    }
}

/// Returns `true` if `node` is the string literal `"HOME"` or `'HOME'`.
fn is_str_home(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(s) if cx.string_str(*s) == "HOME")
}

#[cfg(test)]
mod tests {
    use super::EnvHome;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_env_bracket_home() {
        test::<EnvHome>().expect_correction(
            indoc! {r#"
                ENV['HOME']
                ^^^^^^^^^^^ Use `Dir.home` instead of `ENV['HOME']`.
            "#},
            "Dir.home\n",
        );
    }

    #[test]
    fn flags_env_fetch_home_nil() {
        test::<EnvHome>().expect_correction(
            indoc! {r#"
                ENV.fetch('HOME', nil)
                ^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.home` instead of `ENV['HOME']`.
            "#},
            "Dir.home\n",
        );
    }

    #[test]
    fn accepts_dir_home() {
        test::<EnvHome>().expect_no_offenses("Dir.home\n");
    }

    #[test]
    fn accepts_env_fetch_home_no_default() {
        // No second arg: raises KeyError — different semantics.
        test::<EnvHome>().expect_no_offenses("ENV.fetch('HOME')\n");
    }

    #[test]
    fn accepts_env_fetch_home_non_nil_default() {
        test::<EnvHome>().expect_no_offenses("ENV.fetch('HOME', '/')\n");
    }

    #[test]
    fn accepts_env_bracket_other_key() {
        test::<EnvHome>().expect_no_offenses("ENV['PATH']\n");
    }

    // --- Boundary characterization (murphy-ft88.8): pin the exact node set
    // the hand-rolled `is_global_const` guard matches, so the verbatim
    // `(send (const nil? :ENV) {:[] :fetch} ...)` refactor can be proven
    // equivalent. `::ENV` collapses to `Const{scope:None}` in Murphy: `nil?`
    // covers bare + `::`. Namespaced `Foo::ENV` still accepts; `&.` is a
    // `csend` node and `send` covers `Send` only.

    #[test]
    fn boundary_flags_cbase_env_bracket_home() {
        test::<EnvHome>().expect_correction(
            indoc! {r#"
                ::ENV['HOME']
                ^^^^^^^^^^^^^ Use `Dir.home` instead of `ENV['HOME']`.
            "#},
            "Dir.home\n",
        );
    }

    #[test]
    fn boundary_flags_cbase_env_fetch_home_nil() {
        test::<EnvHome>().expect_correction(
            indoc! {r#"
                ::ENV.fetch('HOME', nil)
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `Dir.home` instead of `ENV['HOME']`.
            "#},
            "Dir.home\n",
        );
    }

    #[test]
    fn boundary_accepts_namespaced_env_bracket_home() {
        test::<EnvHome>().expect_no_offenses("Foo::ENV['HOME']\n");
    }

    #[test]
    fn boundary_accepts_namespaced_env_fetch_home_nil() {
        test::<EnvHome>().expect_no_offenses("Foo::ENV.fetch('HOME', nil)\n");
    }

    #[test]
    fn boundary_accepts_csend_env_bracket_home() {
        // `&.` is a `csend` node; `send` covers `Send` only.
        test::<EnvHome>().expect_no_offenses("ENV&.[]('HOME')\n");
    }

    #[test]
    fn boundary_accepts_csend_env_fetch_home_nil() {
        test::<EnvHome>().expect_no_offenses("ENV&.fetch('HOME', nil)\n");
    }
}

murphy_plugin_api::submit_cop!(EnvHome);
