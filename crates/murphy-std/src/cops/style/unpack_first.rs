//! `Style/UnpackFirst` — flags `.unpack('fmt').first`, `.unpack('fmt')[0]`,
//! `.unpack('fmt').slice(0)`, and `.unpack('fmt').at(0)` in favor of
//! `.unpack1('fmt')`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/UnpackFirst
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-zgcp
//! notes: >
//!   Murphy v1 does not track target_ruby_version; this cop fires regardless
//!   of the Ruby version in use (RuboCop enforces minimum_target_ruby_version
//!   2.4 for `unpack1`).  Both `send` and `csend` (safe-navigation) outer
//!   calls are handled, mirroring RuboCop's `alias on_csend on_send`.
//!   The mixed case `recv&.unpack(fmt).first` (inner csend, outer plain send)
//!   is excluded: autocorrecting it to `recv&.unpack1(fmt)` would change
//!   behaviour when recv is nil (NoMethodError vs. silent nil).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! 'foo'.unpack('h*').first
//! 'foo'.unpack('h*')[0]
//! 'foo'.unpack('h*').slice(0)
//! 'foo'.unpack('h*').at(0)
//!
//! # good
//! 'foo'.unpack1('h*')
//! 'foo'.unpack('h*').first(1)   # .first with an arg is not equivalent
//! 'foo'.unpack('h*')[1]         # non-zero index is not equivalent
//! ```
//!
//! ## Autocorrect
//!
//! Two surgical edits (per `.claude/rules/autocorrect-pattern.md`):
//! 1. Rename the inner `unpack` selector to `unpack1`.
//! 2. Delete the outer accessor (`.first`, `[0]`, `.slice(0)`, `.at(0)`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct UnpackFirst;

#[cop(
    name = "Style/UnpackFirst",
    description = "Use `unpack1` instead of `unpack` followed by `.first` or `[0]`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UnpackFirst {
    #[on_node(kind = "send", methods = ["first", "[]", "slice", "at"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        // Filter to only the relevant methods -- `methods = [...]` is only
        // valid for `kind = "send"` so we do the check manually here.
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if matches!(cx.symbol_str(method), "first" | "[]" | "slice" | "at") {
            check(node, cx);
        }
    }
}

/// Returns `Some((unpack_call, unpack_format_node))` when `outer` matches:
/// - `<recv>.unpack(<fmt>).first`          (no args on outer)
/// - `<recv>.unpack(<fmt>)[0]`             (int 0 arg on outer)
/// - `<recv>.unpack(<fmt>).slice(0)`       (int 0 arg on outer)
/// - `<recv>.unpack(<fmt>).at(0)`          (int 0 arg on outer)
///
/// The mixed case `<recv>&.unpack(<fmt>).first` (inner Csend, outer Send) is
/// rejected: if `recv` is nil the inner `&.unpack` returns nil, and the outer
/// `.first` would then raise `NoMethodError`. Replacing with `&.unpack1` would
/// silently return nil instead — a behaviour change.
fn match_unpack_first(outer: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    // Outer node must be send/csend.
    let (outer_is_csend, outer_method, outer_args) = match cx.kind(outer) {
        NodeKind::Send { method, args, .. } => (false, cx.symbol_str(*method), cx.list(*args)),
        NodeKind::Csend { method, args, .. } => (true, cx.symbol_str(*method), cx.list(*args)),
        _ => return None,
    };

    // Validate outer method and its arguments.
    match outer_method {
        "first" => {
            // `.first` with no arguments only.
            if !outer_args.is_empty() {
                return None;
            }
        }
        "[]" | "slice" | "at" => {
            // Must have exactly one argument: integer literal 0.
            if outer_args.len() != 1 {
                return None;
            }
            match cx.kind(outer_args[0]) {
                NodeKind::Int(0) => {}
                _ => return None,
            }
        }
        _ => return None,
    }

    // Inner node (receiver of the outer call) must be a send/csend to `unpack`
    // with exactly one argument.
    let inner = cx.call_receiver(outer).get()?;
    let (inner_is_csend, inner_method, inner_args) = match cx.kind(inner) {
        NodeKind::Send { method, args, .. } => (false, cx.symbol_str(*method), cx.list(*args)),
        NodeKind::Csend { method, args, .. } => (true, cx.symbol_str(*method), cx.list(*args)),
        _ => return None,
    };

    if inner_method != "unpack" {
        return None;
    }
    if inner_args.len() != 1 {
        return None;
    }

    // Inner must itself have a receiver (not a bare `unpack(...)`).
    cx.call_receiver(inner).get()?;

    // Reject the unsafe mixed case: `recv&.unpack(fmt).first` — the inner
    // safe-navigation can return nil, and the outer non-nil-safe accessor
    // would then raise NoMethodError.  After autocorrect it would silently
    // return nil instead, which is a behaviour change.
    if inner_is_csend && !outer_is_csend {
        return None;
    }

    Some((inner, inner_args[0]))
}

fn check(outer: NodeId, cx: &Cx<'_>) {
    let Some((inner, fmt_node)) = match_unpack_first(outer, cx) else {
        return;
    };

    // Offense range: from the inner `unpack` selector to the end of the outer node.
    let inner_selector = cx.selector(inner);
    let outer_end = cx.range(outer).end;
    let offense_range = Range {
        start: inner_selector.start,
        end: outer_end,
    };

    let fmt_src = cx.raw_source(cx.range(fmt_node));
    let current_src = cx.raw_source(offense_range);
    let message = format!("Use `unpack1({fmt_src})` instead of `{current_src}`.");
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect (surgical two-edit form):
    // Edit 1: rename `unpack` -> `unpack1` on the inner selector.
    cx.emit_edit(inner_selector, "unpack1");
    // Edit 2: delete from end of inner call to end of outer call.
    let delete_range = Range {
        start: cx.range(inner).end,
        end: outer_end,
    };
    cx.emit_edit(delete_range, "");
}

#[cfg(test)]
mod tests {
    use super::UnpackFirst;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- .first -----

    #[test]
    fn flags_unpack_first() {
        test::<UnpackFirst>().expect_correction(
            indoc! {r#"
                'foo'.unpack('h*').first
                      ^^^^^^^^^^^^^^^^^^ Use `unpack1('h*')` instead of `unpack('h*').first`.
            "#},
            "'foo'.unpack1('h*')\n",
        );
    }

    // ----- [0] -----

    #[test]
    fn flags_unpack_index_zero() {
        test::<UnpackFirst>().expect_correction(
            indoc! {r#"
                'foo'.unpack('h*')[0]
                      ^^^^^^^^^^^^^^^ Use `unpack1('h*')` instead of `unpack('h*')[0]`.
            "#},
            "'foo'.unpack1('h*')\n",
        );
    }

    // ----- .slice(0) -----

    #[test]
    fn flags_unpack_slice_zero() {
        test::<UnpackFirst>().expect_correction(
            indoc! {r#"
                'foo'.unpack('h*').slice(0)
                      ^^^^^^^^^^^^^^^^^^^^^ Use `unpack1('h*')` instead of `unpack('h*').slice(0)`.
            "#},
            "'foo'.unpack1('h*')\n",
        );
    }

    // ----- .at(0) -----

    #[test]
    fn flags_unpack_at_zero() {
        test::<UnpackFirst>().expect_correction(
            indoc! {r#"
                'foo'.unpack('h*').at(0)
                      ^^^^^^^^^^^^^^^^^^ Use `unpack1('h*')` instead of `unpack('h*').at(0)`.
            "#},
            "'foo'.unpack1('h*')\n",
        );
    }

    // ----- csend -----

    #[test]
    fn flags_csend_unpack_first() {
        test::<UnpackFirst>().expect_correction(
            indoc! {r#"
                'foo'&.unpack('h*')&.first
                       ^^^^^^^^^^^^^^^^^^^ Use `unpack1('h*')` instead of `unpack('h*')&.first`.
            "#},
            "'foo'&.unpack1('h*')\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_unpack1() {
        test::<UnpackFirst>().expect_no_offenses("'foo'.unpack1('h*')\n");
    }

    #[test]
    fn accepts_first_with_arg() {
        // .first(1) is not equivalent to [0].
        test::<UnpackFirst>().expect_no_offenses("'foo'.unpack('h*').first(1)\n");
    }

    #[test]
    fn accepts_index_nonzero() {
        test::<UnpackFirst>().expect_no_offenses("'foo'.unpack('h*')[1]\n");
    }

    #[test]
    fn accepts_slice_nonzero() {
        test::<UnpackFirst>().expect_no_offenses("'foo'.unpack('h*').slice(1)\n");
    }

    #[test]
    fn accepts_unrelated_first() {
        // .first on something that is not .unpack -- no offense.
        test::<UnpackFirst>().expect_no_offenses("[1, 2, 3].first\n");
    }

    #[test]
    fn accepts_mixed_csend_send_unpack_first() {
        // obj&.unpack('h*').first -- inner is csend, outer is send.
        // If obj is nil, inner returns nil and outer raises NoMethodError.
        // Autocorrecting to obj&.unpack1('h*') would silently return nil -- a
        // behaviour change -- so this form is not flagged.
        test::<UnpackFirst>().expect_no_offenses("obj&.unpack('h*').first\n");
    }
}
murphy_plugin_api::submit_cop!(UnpackFirst);
