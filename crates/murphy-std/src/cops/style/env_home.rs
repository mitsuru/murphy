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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

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
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // Must have a receiver.
    let Some(recv_id) = receiver.get() else {
        return;
    };

    // Receiver must be `ENV` constant.
    if !is_env_const(recv_id, cx) {
        return;
    }

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

/// Returns `true` if `node` is the `ENV` constant.
fn is_env_const(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Const { name, .. } if cx.symbol_str(*name) == "ENV")
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
}

murphy_plugin_api::submit_cop!(EnvHome);
