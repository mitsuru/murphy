//! `Style/RedundantArrayFlatten` — flags `x.flatten.join` and
//! `x.flatten(n).join` where the `flatten` is redundant because
//! `Array#join` already recurses into nested arrays.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantArrayFlatten
//! upstream_version_checked: 1.91.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Verbatim port of `(call (call !nil? :flatten _?) :join (nil)?)`
//!   (murphy-s1yc.43): inner-flatten dispatch (RESTRICT_ON_SEND=[flatten]
//!   plus parent check, mirroring `alias on_csend on_send`) covering both
//!   csend variants; `_?` restricts flatten to 0-or-1 args; `nil ?`
//!   (Quantifier over LitNil) spells upstream `(nil)?` for join (0 or a
//!   single nil arg). The pattern uses `_` for the
//!   inner receiver (binds absent or present per murphy-if9y) with a
//!   hand-rolled `!nil?` guard requiring a present receiver, because
//!   `(call !nil? ...)` does not compile (`call` plus `!` lexes into a
//!   bare-predicate head; `send` is exempt as a concrete kind). The guard
//!   accepts a present nil literal (`nil.flatten.join` flags per upstream)
//!   and rejects bare `flatten.join`, matching upstream `!nil?` exactly.
//!   This cop is marked unsafe in RuboCop (Safe: false) because the
//!   receiver of `flatten` might not be an Array, so it may not respond
//!   to `join`. Also, if the global variable `$,` is set to a value other
//!   than the default `nil`, false positives may occur.
//!   The cop is disabled by default (Enabled: pending in RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! x.flatten.join
//! x.flatten(1).join
//! x&.flatten.join
//! x.flatten&.join
//!
//! # good
//! x.join
//! x.flatten.join(", ")
//! x.flatten(1, 2).join
//! ```

use murphy_plugin_api::{Cx, NodeId, Range, cop, def_node_matcher};

const MSG: &str = "Remove the redundant `flatten`.";

// Verbatim port of the upstream matcher (murphy-s1yc.43):
// `(call (call !nil? :flatten _?) :join (nil)?)` — `call` = `{send csend}`
// covers safe-navigation on both the inner `flatten` and the outer `join`
// (`x&.flatten.join`, `x.flatten&.join`), mirroring upstream
// `alias on_csend on_send`. The inner receiver uses `_` (binds absent or
// present per murphy-if9y) with the `!nil?` presence guard applied
// separately in `check` (see above); `_?` restricts `flatten` to 0-or-1
// args and `nil ?` (space before `?`) spells upstream `(nil)?` — a
// Quantifier over LitNil for 0 or a single nil join arg (bare `nil?`
// without space is NilTest, which would require exactly one arg). Both
// arities are enforced by the pattern itself.
def_node_matcher!(flatten_join, "(call (call _ :flatten _?) :join nil ?)");

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
    /// Inner `flatten` (plain send): mirrors upstream `on_send` with
    /// `RESTRICT_ON_SEND=[flatten]`. Triggered on all sends; the verbatim
    /// parent pattern filters to `flatten` under `join`.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Inner `flatten` (safe navigation): mirrors upstream
    /// `alias on_csend on_send` (`x&.flatten.join` flags).
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(flatten_node: NodeId, cx: &Cx<'_>) {
    // Upstream `!nil?`: the inner `flatten` must have a present receiver.
    // The pattern uses `_` (binds absent or present), so bare
    // `flatten.join` (absent receiver) is rejected here while a present
    // nil literal (`nil.flatten.join`) still flags, matching upstream
    // exactly (verified true vs standalone NodePattern).
    if cx.call_receiver(flatten_node).get().is_none() {
        return;
    }

    // Upstream `flatten_join?(node.parent)`: the parent must match the full
    // `(call (call _ :flatten _?) :join (nil)?)` shape. This enforces the
    // inner `flatten` method plus `_?` arity, and the outer `join` method
    // (send or csend) plus `(nil)?` args, in one verbatim check.
    let Some(parent) = cx.parent(flatten_node).get() else {
        return;
    };
    if !flatten_join(parent, cx) {
        return;
    }

    // The offense range covers `.flatten` (including any args),
    // i.e. from after flatten's receiver end to flatten node end.
    // For safe-navigation (`x&.flatten.join`) this covers `&.flatten`,
    // matching upstream `node.loc.dot.begin.join(node.source_range.end)`.
    let Some(flatten_receiver_id) = cx.call_receiver(flatten_node).get() else {
        return;
    };
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
        // (upstream `!nil?` rejects the absent receiver)
        test::<RedundantArrayFlatten>().expect_no_offenses("flatten.join\n");
    }

    // --- Design flip (murphy-s1yc.43): adopt upstream exactly. The two
    // csend axes newly flag per `alias on_csend on_send` (pre-port they
    // accepted to avoid nil-semantics changes; upstream flags despite
    // Safe: false), and flatten arity narrows to `_?` (0-or-1 args).
    // Verified vs standalone NodePattern (1.91.0): x.flatten.join=>true,
    // x.flatten(1).join=>true, x.flatten(1,2).join=>nil,
    // x&.flatten.join=>true, x.flatten&.join=>true, flatten.join=>nil,
    // x.flatten.join(nil)=>true, x.flatten.join(", ")=>nil,
    // nil.flatten.join=>true.

    #[test]
    fn s1yc43_flags_csend_flatten_join() {
        // Safe-navigation on inner `flatten`: `call` covers `csend` at the
        // pattern level, mirroring upstream `alias on_csend on_send`.
        // Offense covers `&.flatten` (9 chars); autocorrect removes it.
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x&.flatten.join
                 ^^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x.join\n",
        );
    }

    #[test]
    fn s1yc43_flags_flatten_csend_join() {
        // Safe-navigation on outer `join`: `call` covers `csend` at the
        // pattern level. Offense covers `.flatten` (8 chars); autocorrect
        // removes it, leaving `x&.join`.
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x.flatten&.join
                 ^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x&.join\n",
        );
    }

    #[test]
    fn s1yc43_flags_double_csend() {
        // Both inner and outer safe-navigation flag; autocorrect removes
        // `&.flatten`, leaving `x&.join`.
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x&.flatten&.join
                 ^^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x&.join\n",
        );
    }

    #[test]
    fn s1yc43_accepts_flatten_two_args_join() {
        // Upstream `_?` = 0-or-1 flatten args: two args do NOT match.
        // Pre-port flagged any number of flatten args; verbatim narrows.
        test::<RedundantArrayFlatten>().expect_no_offenses("x.flatten(1, 2).join\n");
    }

    #[test]
    fn s1yc43_flags_flatten_join_nil_arg() {
        // Upstream `(nil)?`: an explicit nil join arg still flags.
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                x.flatten.join(nil)
                 ^^^^^^^^ Remove the redundant `flatten`.
            "},
            "x.join(nil)\n",
        );
    }

    #[test]
    fn s1yc43_flags_nil_receiver_flatten_join() {
        // Upstream `!nil?` means present (not absent): a nil-literal
        // receiver is present, so `nil.flatten.join` flags.
        test::<RedundantArrayFlatten>().expect_correction(
            indoc! {"
                nil.flatten.join
                   ^^^^^^^^ Remove the redundant `flatten`.
            "},
            "nil.join\n",
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantArrayFlatten);
