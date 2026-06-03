//! `Style/ArrayFirstLast` — use `arr.first` and `arr.last` instead of `arr[0]` and `arr[-1]`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArrayFirstLast
//! upstream_version_checked: 1.86.2
//! version_added: "1.58"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `arr[0]` → `arr.first` and `arr[-1]` → `arr.last`. Disabled by
//!   default (Enabled: false in RuboCop) because `[0]` and `[-1]` on a Hash
//!   return the value for that key, while `.first`/`.last` return the first/last
//!   tuple; also String has no `first`/`last`.
//!   Chain guard: nested bracket chains like `arr[0][-2]` are not flagged —
//!   this mirrors `innermost_braces_node` + `brace_method?` from RuboCop.
//!   Assignment form `arr[0] = x` is `[]=` and is never dispatched to this cop.
//!   Both plain send (`arr[0]`) and csend (`arr&.[](0)`) are handled, mirroring
//!   RuboCop's `alias on_csend on_send`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! arr[0]
//! arr[-1]
//!
//! # good
//! arr.first
//! arr.last
//! arr[0] = 2    # assignment — never dispatched
//! arr[0][-2]    # chained brackets — not flagged
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the bracket-and-argument portion (`[0]` or `[-1]`) with `.first` or
//! `.last`. Uses the bracket region (`receiver.end..node.end`) as the offense
//! range, matching RuboCop's `loc.selector` for bracket-notation calls.
//! For the explicit dot form (`arr.[](0)`), the offense starts at the dot.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct ArrayFirstLast;

#[cop(
    name = "Style/ArrayFirstLast",
    description = "Use `arr.first` and `arr.last` instead of `arr[0]` and `arr[-1]`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ArrayFirstLast {
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("[]") {
            check(node, cx);
        }
    }
}

/// Returns `true` if `node` is a plain `Send` call to `[]` or `[]=`.
fn is_brace_method(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && matches!(cx.method_name(node), Some("[]") | Some("[]="))
}

/// Walk up the receiver chain: while the current node's receiver is itself a
/// `[]` Send, move to that receiver. Returns the innermost such node.
/// Mirrors RuboCop's `innermost_braces_node`.
fn innermost_braces_node(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = node;
    while let Some(recv) = cx.call_receiver(current).get() {
        if matches!(cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. })
            && cx.method_name(recv) == Some("[]")
        {
            current = recv;
        } else {
            break;
        }
    }
    current
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must have exactly one argument.
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    // Argument must be an integer literal 0 or -1.
    let value = match cx.kind(args[0]) {
        NodeKind::Int(n) => *n,
        _ => return,
    };
    if value != 0 && value != -1 {
        return;
    }

    // Walk receiver chain to find the innermost brace node.
    let inner = innermost_braces_node(node, cx);

    // If the inner node's parent is a brace method (`[]` or `[]=`), skip.
    if cx.parent(inner).get().is_some_and(|p| is_brace_method(p, cx)) {
        return;
    }

    let preferred = if value == 0 { "first" } else { "last" };

    // Offense range: the bracket region (from end of receiver to end of node).
    // For bracket notation (`arr[0]`), this is `[0]` — matches RuboCop's loc.selector.
    // For explicit dot notation (`arr.[](0)`), the dot is included.
    let offense_range = offense_range(inner, cx);

    let message = format!("Use `{preferred}`.");
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: replace the bracket region with `.first` or `.last`.
    // When `loc.dot` is present (explicit dot form), omit the leading dot.
    let dot_range = cx.loc(inner).dot();
    let replacement = if dot_range != Range::ZERO {
        // Already has a dot: offense covers from dot to end, replacement is bare name.
        preferred.to_string()
    } else {
        // No dot: bracket form, replacement adds the dot.
        format!(".{preferred}")
    };
    cx.emit_edit(offense_range, &replacement);
}

/// Computes the offense range for `inner` node.
/// - No explicit dot (bracket notation `arr[0]`): range from end of receiver to end of node → `[0]`.
/// - Explicit dot (`arr.[](0)` or `arr&.[](0)`): range from selector start to end of node → `[](0)`.
///   The dot/&. is already present; we replace only the selector+args portion with the bare name.
fn offense_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let dot_range = cx.loc(node).dot();
    let node_end = cx.range(node).end;

    if dot_range != Range::ZERO {
        // Explicit dot form (`.[]()` or `&.[]()` notation).
        // Offense starts at the selector (`[]`), not at the dot/&.
        // Replacement is bare `first`/`last`; the dot/&. stays in place.
        let selector = cx.selector(node);
        Range {
            start: selector.start,
            end: node_end,
        }
    } else {
        // Bracket notation: offense covers from end of receiver through end of node.
        let recv_end = cx
            .call_receiver(node)
            .get()
            .map(|r| cx.range(r).end)
            .unwrap_or(cx.range(node).start);
        Range {
            start: recv_end,
            end: node_end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ArrayFirstLast;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- arr[0] → arr.first ---

    #[test]
    fn flags_index_zero() {
        test::<ArrayFirstLast>().expect_correction(
            indoc! {r#"
                arr[0]
                   ^^^ Use `first`.
            "#},
            "arr.first\n",
        );
    }

    // --- arr[-1] → arr.last ---

    #[test]
    fn flags_index_minus_one() {
        test::<ArrayFirstLast>().expect_correction(
            indoc! {r#"
                arr[-1]
                   ^^^^ Use `last`.
            "#},
            "arr.last\n",
        );
    }

    // --- csend ---

    #[test]
    fn flags_csend_index_zero() {
        test::<ArrayFirstLast>().expect_correction(
            indoc! {r#"
                arr&.[](0)
                     ^^^^^ Use `first`.
            "#},
            "arr&.first\n",
        );
    }

    #[test]
    fn flags_csend_index_minus_one() {
        test::<ArrayFirstLast>().expect_correction(
            indoc! {r#"
                arr&.[](-1)
                     ^^^^^^ Use `last`.
            "#},
            "arr&.last\n",
        );
    }

    // --- negative cases: non-zero/non-negative-one indices ---

    #[test]
    fn accepts_index_one() {
        test::<ArrayFirstLast>().expect_no_offenses("arr[1]\n");
    }

    #[test]
    fn accepts_index_minus_two() {
        test::<ArrayFirstLast>().expect_no_offenses("arr[-2]\n");
    }

    // --- negative cases: chained brackets ---

    #[test]
    fn accepts_chained_brackets() {
        // arr[0][-2]: outer [-2] gets innermost = arr[0], whose parent is arr[0][-2] ([] send).
        // arr[0] itself gets innermost = arr[0], whose parent is arr[0][-2] ([] send).
        // Neither should be flagged.
        test::<ArrayFirstLast>().expect_no_offenses("arr[0][-2]\n");
    }

    #[test]
    fn accepts_nested_bracket_argument() {
        // foo[bar[0]]: inner bar[0] has parent foo[bar[0]] which is a [] send — not flagged.
        test::<ArrayFirstLast>().expect_no_offenses("foo[bar[0]]\n");
    }

    // --- negative cases: already good forms ---

    #[test]
    fn accepts_first() {
        test::<ArrayFirstLast>().expect_no_offenses("arr.first\n");
    }

    #[test]
    fn accepts_last() {
        test::<ArrayFirstLast>().expect_no_offenses("arr.last\n");
    }

    // --- receiver is a method call ---

    #[test]
    fn flags_method_call_receiver() {
        test::<ArrayFirstLast>().expect_correction(
            indoc! {r#"
                foo.bar[0]
                       ^^^ Use `first`.
            "#},
            "foo.bar.first\n",
        );
    }

    // --- default_enabled: false ---

    #[test]
    fn is_disabled_by_default() {
        use murphy_plugin_api::Cop;
        assert_eq!(ArrayFirstLast::DEFAULT_ENABLED, Some(false));
    }
}

murphy_plugin_api::submit_cop!(ArrayFirstLast);
