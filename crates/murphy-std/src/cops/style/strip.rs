//! `Style/Strip` — prefer `strip` over `lstrip.rstrip` or `rstrip.lstrip`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Strip
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Matches `lstrip.rstrip` and `rstrip.lstrip` chains in both send and csend
//!   variants. Offense range covers from the inner method name to the end of the
//!   outer call, matching RuboCop's range_between(first_send.loc.selector,
//!   node.source_range.end). Autocorrect replaces the range with `strip`.
//!   csend: methods filter is not supported for csend nodes by the macro;
//!   the csend handler dispatches manually based on method name.
//!   Mixed csend/send combinations: `x&.lstrip.rstrip` (inner csend, outer
//!   send) is skipped entirely. When x is nil, x&.lstrip returns nil and
//!   nil.rstrip raises NoMethodError, but autocorrecting to x&.strip would
//!   silently return nil — a behavior change. RuboCop does not filter this
//!   case; Murphy chooses the conservative stance.
//!   `x.lstrip&.rstrip` (inner send, outer csend) is still matched because
//!   x.lstrip always returns a non-nil String, making the &. redundant.
//!   Both inner and outer calls must have no arguments.
//! ```
//!
//! ## Matched shapes
//!
//! - `x.lstrip.rstrip` → offense
//! - `x.rstrip.lstrip` → offense
//! - `x&.lstrip&.rstrip` / `x&.rstrip&.lstrip` → offense (both csend)
//! - `x.lstrip&.rstrip` / `x.rstrip&.lstrip` → offense (inner send, outer csend)
//!
//! ## Non-matches
//!
//! - `x&.lstrip.rstrip` / `x&.rstrip.lstrip` — inner csend, outer send (behavior change risk)
//! - `x.lstrip.rstrip(arg)` — outer call has arguments
//! - `x.lstrip(arg).rstrip` — inner call has arguments
//!
//! ## Autocorrect
//!
//! Replace the `<inner>.<outer>` range with `strip`.
//! The receiver is preserved byte-for-byte.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

const MSG: &str = "Use `strip` instead of `%s`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct Strip;

#[cop(
    name = "Style/Strip",
    description = "Use `strip` instead of `lstrip.rstrip`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Strip {
    #[on_node(kind = "send", methods = ["lstrip", "rstrip"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// csend: `methods = [...]` is not supported for `kind = "csend"` by the
    /// macro, so we dispatch manually here.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let method = cx.method_name(node).unwrap_or_default();
        if method == "lstrip" || method == "rstrip" {
            check(node, cx);
        }
    }
}

fn check(outer: NodeId, cx: &Cx<'_>) {
    let outer_method = cx.method_name(outer).unwrap_or_default();

    // The outer call must have a receiver that is itself a lstrip/rstrip call.
    let Some(inner_id) = cx.call_receiver(outer).get() else {
        return;
    };

    let Some(inner_method) = cx.method_name(inner_id) else {
        return;
    };

    // The pair must be complementary: (lstrip, rstrip) or (rstrip, lstrip).
    if !is_complementary(inner_method, outer_method) {
        return;
    }

    // Skip when inner is csend and outer is plain send: `x&.lstrip.rstrip`.
    // In this shape, x&.lstrip short-circuits on nil (returns nil), and
    // nil.rstrip would raise NoMethodError. Autocorrecting to x&.strip changes
    // that from a runtime error to nil — a silent behavior change. The safe
    // conservative stance is to not flag this combination at all.
    let outer_is_csend = cx.is_safe_navigation(outer);
    let inner_is_csend = cx.is_safe_navigation(inner_id);
    if inner_is_csend && !outer_is_csend {
        return;
    }

    // Neither call must have arguments — guarding the inner call prevents
    // matching `x.lstrip(arg).rstrip`, and guarding the outer prevents
    // silently deleting `arg` in `x.lstrip.rstrip(arg)`.
    if !cx.call_arguments(inner_id).is_empty() {
        return;
    }
    if !cx.call_arguments(outer).is_empty() {
        return;
    }

    // Offense range: from start of inner method name to end of outer node.
    // Mirrors RuboCop: range_between(first_send.loc.selector.begin_pos, node.source_range.end_pos)
    let inner_name_start = cx.node(inner_id).loc.name.start;
    let outer_end = cx.range(outer).end;
    let offense_range = Range {
        start: inner_name_start,
        end: outer_end,
    };

    let offense_src = cx.raw_source(offense_range);
    let message = MSG.replace("%s", offense_src);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: replace `<inner_method>.<outer_method>` with `strip`.
    cx.emit_edit(offense_range, "strip");
}

fn is_complementary(inner: &str, outer: &str) -> bool {
    (inner == "lstrip" && outer == "rstrip") || (inner == "rstrip" && outer == "lstrip")
}

#[cfg(test)]
mod tests {
    use super::Strip;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_lstrip_rstrip() {
        test::<Strip>().expect_correction(
            indoc! {"
                x.lstrip.rstrip
                  ^^^^^^^^^^^^^ Use `strip` instead of `lstrip.rstrip`.
            "},
            "x.strip\n",
        );
    }

    #[test]
    fn flags_rstrip_lstrip() {
        test::<Strip>().expect_correction(
            indoc! {"
                x.rstrip.lstrip
                  ^^^^^^^^^^^^^ Use `strip` instead of `rstrip.lstrip`.
            "},
            "x.strip\n",
        );
    }

    #[test]
    fn flags_nested_chain() {
        test::<Strip>().expect_correction(
            indoc! {"
                str.strip.lstrip.rstrip
                          ^^^^^^^^^^^^^ Use `strip` instead of `lstrip.rstrip`.
            "},
            "str.strip.strip\n",
        );
    }

    #[test]
    fn accepts_strip_alone() {
        test::<Strip>().expect_no_offenses("x.strip\n");
    }

    #[test]
    fn accepts_lstrip_alone() {
        test::<Strip>().expect_no_offenses("x.lstrip\n");
    }

    #[test]
    fn accepts_rstrip_alone() {
        test::<Strip>().expect_no_offenses("x.rstrip\n");
    }

    #[test]
    fn accepts_lstrip_lstrip() {
        // Same method twice — not the complementary pair.
        test::<Strip>().expect_no_offenses("x.lstrip.lstrip\n");
    }

    #[test]
    fn accepts_rstrip_rstrip() {
        test::<Strip>().expect_no_offenses("x.rstrip.rstrip\n");
    }

    #[test]
    fn accepts_lstrip_rstrip_with_outer_arg() {
        // Outer call has an argument — do not fire to avoid silent argument deletion.
        test::<Strip>().expect_no_offenses("x.lstrip.rstrip(1)\n");
    }

    #[test]
    fn accepts_rstrip_lstrip_with_outer_arg() {
        test::<Strip>().expect_no_offenses("x.rstrip.lstrip(1)\n");
    }

    #[test]
    fn accepts_lstrip_with_inner_arg_rstrip() {
        // Inner call has an argument.
        test::<Strip>().expect_no_offenses("x.lstrip(1).rstrip\n");
    }

    #[test]
    fn accepts_csend_lstrip_send_rstrip() {
        // x&.lstrip.rstrip — inner csend, outer send.
        // When x is nil, x&.lstrip returns nil and nil.rstrip raises NoMethodError.
        // Autocorrecting to x&.strip would silently change that to nil — skip.
        test::<Strip>().expect_no_offenses("x&.lstrip.rstrip\n");
    }

    #[test]
    fn accepts_csend_rstrip_send_lstrip() {
        test::<Strip>().expect_no_offenses("x&.rstrip.lstrip\n");
    }
}
murphy_plugin_api::submit_cop!(Strip);
