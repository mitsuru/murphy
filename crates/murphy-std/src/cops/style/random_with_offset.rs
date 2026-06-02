//! `Style/RandomWithOffset` — prefer ranges over random integers with offsets.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RandomWithOffset
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Enabled by default (matches upstream `Enabled: true`).
//!   All three pattern families are covered:
//!     - rand_op_integer: rand(n) + k / rand(n) - k (receiver=rand-call, arg=int)
//!     - integer_op_rand: k + rand(n) / k - rand(n) (receiver=int, arg=rand-call)
//!     - rand_modified: rand(n).succ / rand(n).pred / rand(n).next
//!   "rand-call" receiver accepted:
//!     - nil (bare `rand`)
//!     - `Random` / `::Random` constant
//!     - `Kernel` / `::Kernel` constant
//!   Rand argument accepted:
//!     - int literal (range [0, n-1])
//!     - inclusive range (irange a..b → [a, b])
//!     - exclusive range (erange a...b → [a, b-1])
//!   Beginless/endless ranges (nil endpoints) are skipped.
//!   Autocorrect: whole-node replacement (AST shuffle — falls back to the
//!   interpolation form per autocorrect-pattern.md guidance).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! rand(6) + 1         # → rand(1..6)
//! 1 + rand(6)         # → rand(1..6)
//! rand(6) - 1         # → rand(-1..4)
//! 1 - rand(6)         # → rand(-4..1)
//! rand(6).succ        # → rand(1..6)
//! rand(6).pred        # → rand(-1..4)
//! rand(6).next        # → rand(1..6)
//! Random.rand(6) + 1  # → Random.rand(1..6)
//! Kernel.rand(6) + 1  # → Kernel.rand(1..6)
//! rand(0..5) + 1      # → rand(1..6)
//!
//! # good
//! rand(1..6)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RandomWithOffset;

const MSG: &str =
    "Prefer ranges when generating random numbers instead of integers with offsets.";

#[cop(
    name = "Style/RandomWithOffset",
    description = "Prefer to use ranges when generating random numbers instead of integers with offsets.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RandomWithOffset {
    #[on_node(kind = "send", methods = ["+", "-", "succ", "pred", "next"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Pattern matching helpers
// ---------------------------------------------------------------------------

/// Shape of the matching `rand(...)` call with its computed integer boundaries.
struct RandCall {
    /// NodeId of the `rand(...)` send node (receiver side).
    node: NodeId,
    /// Left boundary (inclusive).
    left: i64,
    /// Right boundary (inclusive).
    right: i64,
}

/// Check whether a node is a qualifying "rand call" and return its boundaries.
/// Qualifying receiver: nil, `Random` const, or `Kernel` const.
/// Qualifying argument: int literal or inclusive/exclusive range with int endpoints.
fn extract_rand_call(node: NodeId, cx: &Cx<'_>) -> Option<RandCall> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "rand" {
        return None;
    }
    if !is_rand_receiver(receiver, cx) {
        return None;
    }
    let args_list = cx.list(args);
    if args_list.len() != 1 {
        return None;
    }
    let (left, right) = boundaries_from_arg(args_list[0], cx)?;
    Some(RandCall { node, left, right })
}

/// Check whether an optional receiver is nil, or a top-level `Random`/`Kernel` constant.
///
/// `My::Random` (scope is `Some`) is excluded because it may not implement the
/// same semantics as `::Random.rand`. Only bare `Random` / `Kernel` (scope is
/// `None`) and their `::Random` / `::Kernel` equivalents qualify.
fn is_rand_receiver(recv: OptNodeId, cx: &Cx<'_>) -> bool {
    match recv.get() {
        None => true,
        Some(r) => match cx.kind(r) {
            NodeKind::Const { name, scope } => {
                // Only allow bare Random/Kernel (scope == None).
                // My::Random has scope == Some(_) and is excluded.
                if scope.get().is_some() {
                    return false;
                }
                let s = cx.symbol_str(*name);
                s == "Random" || s == "Kernel"
            }
            _ => false,
        },
    }
}

/// Derive inclusive [left, right] boundaries from the rand argument.
/// Returns `None` if the arg is not an int or a range with int endpoints.
fn boundaries_from_arg(arg: NodeId, cx: &Cx<'_>) -> Option<(i64, i64)> {
    match *cx.kind(arg) {
        NodeKind::Int(n) => {
            if n <= 0 {
                return None; // rand(0) returns float; negative ints are invalid
            }
            Some((0, n - 1))
        }
        NodeKind::RangeExpr { begin_, end_, exclusive } => {
            let begin_node = begin_.get()?;
            let end_node = end_.get()?;
            let NodeKind::Int(a) = *cx.kind(begin_node) else {
                return None;
            };
            let NodeKind::Int(b) = *cx.kind(end_node) else {
                return None;
            };
            if exclusive {
                Some((a, b - 1))
            } else {
                Some((a, b))
            }
        }
        _ => None,
    }
}

/// Build the corrected `[prefix.]rand(lo..hi)` string.
fn corrected(rand_node: NodeId, lo: i64, hi: i64, cx: &Cx<'_>) -> String {
    let prefix = rand_prefix(rand_node, cx);
    format!("{prefix}({lo}..{hi})")
}

/// Build the `rand` prefix (e.g. `""`, `"Random.rand"`, `"Kernel.rand"`).
fn rand_prefix(rand_node: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Send { receiver, .. } = *cx.kind(rand_node) else {
        return "rand".to_string();
    };
    match receiver.get() {
        None => "rand".to_string(),
        Some(recv) => {
            let recv_src = cx.raw_source(cx.range(recv));
            format!("{recv_src}.rand")
        }
    }
}

// ---------------------------------------------------------------------------
// Main check
// ---------------------------------------------------------------------------

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m,
        None => return,
    };

    let corrected_str = match method {
        "succ" | "next" => check_rand_modified_succ(node, cx),
        "pred" => check_rand_modified_pred(node, cx),
        "+" | "-" => check_arithmetic(node, method, cx),
        _ => return,
    };

    let Some(replacement) = corrected_str else {
        return;
    };

    cx.emit_offense(cx.range(node), MSG, None);
    cx.emit_edit(cx.range(node), &replacement);
}

/// `rand(n).succ` / `rand(n).next` → `rand(left+1..right+1)`
fn check_rand_modified_succ(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let receiver = cx.call_receiver(node).get()?;
    // receiver must have no additional args on node side
    let args = cx.call_arguments(node);
    if !args.is_empty() {
        return None;
    }
    let rand = extract_rand_call(receiver, cx)?;
    Some(corrected(rand.node, rand.left + 1, rand.right + 1, cx))
}

/// `rand(n).pred` → `rand(left-1..right-1)`
fn check_rand_modified_pred(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let receiver = cx.call_receiver(node).get()?;
    let args = cx.call_arguments(node);
    if !args.is_empty() {
        return None;
    }
    let rand = extract_rand_call(receiver, cx)?;
    Some(corrected(rand.node, rand.left - 1, rand.right - 1, cx))
}

/// `rand(n) + k`, `rand(n) - k`, `k + rand(n)`, `k - rand(n)`
fn check_arithmetic(node: NodeId, method: &str, cx: &Cx<'_>) -> Option<String> {
    let receiver = cx.call_receiver(node).get()?;
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    let arg = args[0];

    // Case 1: rand_op_integer — receiver is rand call, arg is int
    if let Some(rand) = extract_rand_call(receiver, cx) {
        if let NodeKind::Int(offset) = *cx.kind(arg) {
            let (lo, hi) = match method {
                "+" => (rand.left + offset, rand.right + offset),
                "-" => (rand.left - offset, rand.right - offset),
                _ => return None,
            };
            return Some(corrected(rand.node, lo, hi, cx));
        }
    }

    // Case 2: integer_op_rand — receiver is int, arg is rand call
    if let NodeKind::Int(offset) = *cx.kind(receiver) {
        if let Some(rand) = extract_rand_call(arg, cx) {
            let (lo, hi) = match method {
                "+" => (offset + rand.left, offset + rand.right),
                // k - rand(n) → (k - right)..(k - left)
                "-" => (offset - rand.right, offset - rand.left),
                _ => return None,
            };
            return Some(corrected(rand.node, lo, hi, cx));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RandomWithOffset;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- rand(n) + k -----

    #[test]
    fn flags_rand_plus_int() {
        test::<RandomWithOffset>().expect_offense(indoc! {"
            rand(6) + 1
            ^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
        "});
    }

    #[test]
    fn corrects_rand_plus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(6) + 1
                ^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    // ----- k + rand(n) -----

    #[test]
    fn corrects_int_plus_rand() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                1 + rand(6)
                ^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    // ----- rand(n) - k -----

    #[test]
    fn corrects_rand_minus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(6) - 1
                ^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(-1..4)\n",
        );
    }

    // ----- k - rand(n) -----

    #[test]
    fn corrects_int_minus_rand() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                1 - rand(6)
                ^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(-4..1)\n",
        );
    }

    // ----- .succ / .next -----

    #[test]
    fn corrects_rand_succ() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(6).succ
                ^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    #[test]
    fn corrects_rand_next() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(6).next
                ^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    // ----- .pred -----

    #[test]
    fn corrects_rand_pred() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(6).pred
                ^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(-1..4)\n",
        );
    }

    // ----- Random.rand / Kernel.rand -----

    #[test]
    fn corrects_random_rand_plus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                Random.rand(6) + 1
                ^^^^^^^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "Random.rand(1..6)\n",
        );
    }

    #[test]
    fn corrects_kernel_rand_plus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                Kernel.rand(6) + 1
                ^^^^^^^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "Kernel.rand(1..6)\n",
        );
    }

    // ----- rand(range) + k -----

    #[test]
    fn corrects_rand_irange_plus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(0..5) + 1
                ^^^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    #[test]
    fn corrects_rand_erange_plus_int() {
        test::<RandomWithOffset>().expect_correction(
            indoc! {"
                rand(0...6) + 1
                ^^^^^^^^^^^^^^^ Prefer ranges when generating random numbers instead of integers with offsets.
            "},
            "rand(1..6)\n",
        );
    }

    // ----- Idempotency -----

    #[test]
    fn accepts_rand_range_already() {
        test::<RandomWithOffset>().expect_no_offenses("rand(1..6)\n");
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_rand_no_arg() {
        test::<RandomWithOffset>().expect_no_offenses("rand + 1\n");
    }

    #[test]
    fn accepts_plain_plus() {
        test::<RandomWithOffset>().expect_no_offenses("a + 1\n");
    }

    #[test]
    fn accepts_rand_succ_with_arg() {
        // succ with args is not the Integer#succ pattern
        test::<RandomWithOffset>().expect_no_offenses("rand(6).succ(1)\n");
    }

    #[test]
    fn accepts_rand_zero_plus_int() {
        // rand(0) returns a float, not an integer in [0, -1]; skip it.
        test::<RandomWithOffset>().expect_no_offenses("rand(0) + 1\n");
    }

    #[test]
    fn accepts_namespaced_random_rand() {
        // My::Random is not stdlib Random; do not flag it.
        test::<RandomWithOffset>().expect_no_offenses("My::Random.rand(6) + 1\n");
    }
}
murphy_plugin_api::submit_cop!(RandomWithOffset);
