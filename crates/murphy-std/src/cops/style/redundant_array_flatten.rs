//! `Style/RedundantArrayFlatten` — flags `x.flatten.join` and
//! `x.flatten(n).join` where the `flatten` is redundant because
//! `Array#join` already recurses into nested arrays.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantArrayFlatten
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   This cop is marked unsafe in RuboCop (Safe: false) because the
//!   receiver of `flatten` might not be an Array, so it may not respond
//!   to `join`. Also, if the global variable `$,` is set to a value other
//!   than the default `nil`, false positives may occur.
//!   The cop is disabled by default (Enabled: pending in RuboCop).
//!   Upstream uses `alias on_csend on_send` and flags all csend variants.
//!   Murphy restricts to plain `send` for both flatten and join to avoid
//!   autocorrect behavior changes on nil receivers: x.flatten&.join → x&.join
//!   would silently return nil instead of raising when x is nil.
//!   Covered patterns:
//!     - x.flatten.join (both plain send)
//!     - x.flatten(n).join (any number of flatten args)
//!     - x.flatten.join with no arg or explicit nil arg
//!   Not flagged:
//!     - x.flatten.join(", ") (join with non-nil separator arg)
//!     - flatten with no receiver (bare `flatten.join`)
//!     - csend variants (x&.flatten.join, x.flatten&.join) — behavior change risk
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! x.flatten.join
//! x.flatten(1).join
//!
//! # good
//! x.join
//! x.flatten.join(", ")
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Remove the redundant `flatten`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantArrayFlatten;

#[cop(
    name = "Style/RedundantArrayFlatten",
    description = "Checks for redundant calls of `Array#flatten` before `Array#join`.",
    default_severity = "warning",
    default_enabled = false,
    options = murphy_plugin_api::NoOptions,
)]
impl RedundantArrayFlatten {
    #[on_node(kind = "send", methods = ["join"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(join_node: NodeId, cx: &Cx<'_>) {
    // The join call must have zero arguments or a single nil arg.
    let join_args = cx.call_arguments(join_node);
    match join_args.len() {
        0 => {}
        1 => {
            if !matches!(cx.kind(join_args[0]), NodeKind::Nil) {
                return;
            }
        }
        _ => return,
    }

    // The receiver of join must be a plain (non-safe-navigation) flatten call.
    // We only handle `send` (not `csend`) for flatten to avoid autocorrect
    // behavior changes: `x.flatten&.join` → `x&.join` changes nil semantics
    // because `x.flatten&.join` raises when x is nil, but `x&.join` returns nil.
    let Some(flatten_node) = cx.call_receiver(join_node).get() else {
        return;
    };
    let NodeKind::Send {
        receiver: flatten_receiver,
        method: flatten_method,
        ..
    } = *cx.kind(flatten_node)
    else {
        return;
    };
    if cx.symbol_str(flatten_method) != "flatten" {
        return;
    }

    // flatten must have a non-nil receiver (bare `flatten.join` is not flagged).
    let Some(flatten_receiver_id) = flatten_receiver.get() else {
        return;
    };

    // The offense range covers `.flatten` (including any args),
    // i.e. from after flatten's receiver end to flatten node end.
    let flatten_receiver_end = cx.range(flatten_receiver_id).end;
    let flatten_end = cx.range(flatten_node).end;

    let offense_range = Range {
        start: flatten_receiver_end,
        end: flatten_end,
    };

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: delete the range covering .flatten(...)
    cx.emit_edit(offense_range, "");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantArrayFlatten;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_flatten_join_no_args() {
        // offense range covers `.flatten` (dot + method name, 8 chars)
        test::<RedundantArrayFlatten>().expect_offense(indoc! {"
            x.flatten.join
             ^^^^^^^^ Remove the redundant `flatten`.
        "});
    }

    #[test]
    fn corrects_flatten_join_no_args() {
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x.flatten.join
                 ^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x.join\n",
        );
    }

    #[test]
    fn flags_flatten_with_arg_join() {
        // offense range covers `.flatten(1)` (11 chars)
        test::<RedundantArrayFlatten>().expect_offense(indoc! {"
            x.flatten(1).join
             ^^^^^^^^^^^ Remove the redundant `flatten`.
        "});
    }

    #[test]
    fn corrects_flatten_with_arg_join() {
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x.flatten(1).join
                 ^^^^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x.join\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_flatten_join_with_separator() {
        test::<RedundantArrayFlatten>().expect_no_offenses("x.flatten.join(', ')\n");
    }

    #[test]
    fn accepts_flatten_without_join() {
        test::<RedundantArrayFlatten>().expect_no_offenses("x.flatten\n");
    }

    #[test]
    fn accepts_join_without_flatten() {
        test::<RedundantArrayFlatten>().expect_no_offenses("x.join\n");
    }

    #[test]
    fn accepts_bare_flatten_join() {
        // bare flatten without explicit receiver is not flagged
        test::<RedundantArrayFlatten>().expect_no_offenses("flatten.join\n");
    }

    #[test]
    fn accepts_csend_flatten_join() {
        // x.flatten&.join is not flagged: autocorrect x&.join changes nil semantics
        test::<RedundantArrayFlatten>().expect_no_offenses("x.flatten&.join\n");
    }

    #[test]
    fn accepts_flatten_csend_join() {
        // x&.flatten.join is not flagged: autocorrect x.join changes nil semantics
        test::<RedundantArrayFlatten>().expect_no_offenses("x&.flatten.join\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantArrayFlatten);
