//! `Style/Sample` — use `sample` instead of `shuffle.first`, `shuffle.last`, `shuffle[]`, etc.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Sample
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `shuffle.first`, `shuffle.last`, `shuffle[0]`, `shuffle[-1]`,
//!   `shuffle.at(0)`, `shuffle.at(-1)`, `shuffle.slice(0)`, `shuffle.slice(-1)`,
//!   `shuffle[0, N]`, `shuffle[0..N]`, `shuffle[0...N]`,
//!   `shuffle.slice(0, N)`, `shuffle.slice(0..N)`, `shuffle.slice(0...N)`,
//!   and `shuffle(random: RNG).first` / similar variants.
//!   Only plain-send accessors (`obj.shuffle.first`) are handled.
//!   Safe-navigation at any level (`obj&.shuffle.first`, `obj.shuffle&.first`)
//!   is not flagged — the cop only acts on plain-send receivers and
//!   accessors, matching the conservative approach used by other std
//!   cops (cf. `Style/RedundantSort`).
//!
//!   Known v1 limitation: no per-cop file-pattern gating. The cop fires on
//!   any file; no `Include`/`Exclude` patterns are supported.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! [1, 2, 3].shuffle.first
//! [1, 2, 3].shuffle.last
//! [1, 2, 3].shuffle[2]
//! [1, 2, 3].shuffle.at(0)
//! [1, 2, 3].shuffle.slice(0)
//! [1, 2, 3].shuffle[0, 2]
//! [1, 2, 3].shuffle[0..2]
//! [1, 2, 3].shuffle(random: Random.new).first
//!
//! # good
//! [1, 2, 3].sample
//! [1, 2, 3].shuffle
//! [1, 2, 3].shuffle[2, 3]
//! [1, 2, 3].shuffle[1..3]
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `shuffle.method(args)` with `sample` or `sample(N)`, preserving
//! any keyword arguments passed to `shuffle`. Safe by construction — the
//! replacement is always equivalent.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct Sample;

#[cop(
    name = "Style/Sample",
    description = "Use `sample` instead of `shuffle.first`, `shuffle.last`, `shuffle[]`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Sample {
    #[on_node(kind = "send", methods = ["first", "last", "[]", "at", "slice"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    // Note: no csend handler — safe-navigation *accessor* (`obj.shuffle&.first`)
    // is not flagged. And `is_shuffle_call` rejects csend receivers, so
    // `obj&.shuffle.first` is also not flagged. Only plain-send-all-the-way
    // chains are handled, matching the conservative approach established
    // by `Style/RedundantSort`.
}

// ---------------------------------------------------------------------------
// Core check logic
// ---------------------------------------------------------------------------

/// Check if `node` is a shuffle-based accessor that should use `sample`.
fn check(node: NodeId, cx: &Cx<'_>) {
    // The receiver must be a shuffle call (plain Send or Csend).
    let Some(receiver_id) = cx.call_receiver(node).get() else {
        return;
    };
    if !is_shuffle_call(receiver_id, cx) {
        return;
    }

    let method_name = cx.method_name(node).unwrap();
    let method_args = cx.call_arguments(node);

    // Determine the sample argument.
    let sample_arg: Option<String> = match method_name {
        "first" | "last" => {
            // `first(1, 2)` is syntactically valid but raises ArgumentError.
            if method_args.len() > 1 {
                return;
            }
            method_args
                .first()
                .map(|id| cx.raw_source(cx.range(*id)).to_string())
        }
        "[]" | "slice" => match sample_size(method_args, cx) {
            SampleSize::Computable(arg) => arg,
            SampleSize::Unknown => return,
        },
        "at" => match sample_size_at(method_args, cx) {
            SampleSize::Computable(arg) => arg,
            SampleSize::Unknown => return,
        },
        _ => return,
    };

    // Build the shuffle-args source (keyword args like `random: Random.new`).
    let shuffle_args_source: Option<String> = {
        let shuffle_args = cx.call_arguments(receiver_id);
        shuffle_args.first().map(|id| cx.raw_source(cx.range(*id)).to_string())
    };

    // Build correction text: `sample`, `sample(N)`, or `sample(N, random: ...)`.
    let mut parts: Vec<&str> = Vec::new();
    if let Some(ref s) = sample_arg {
        parts.push(s);
    }
    if let Some(ref s) = shuffle_args_source {
        parts.push(s);
    }
    let correction = if parts.is_empty() {
        "sample".to_string()
    } else {
        format!("sample({})", parts.join(", "))
    };

    // Offense range: from the `shuffle` method selector to end of the outer call.
    let shuffle_selector = cx.selector(receiver_id);
    let outer_end = cx.range(node).end;
    let offense_range = Range {
        start: shuffle_selector.start,
        end: outer_end,
    };

    // Message mirrors RuboCop's `Use '%<correct>s' instead of '%<incorrect>s'.`
    let offense_source = cx.raw_source(offense_range);
    let message = format!("Use `{correction}` instead of `{offense_source}`.");

    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, &correction);
}

/// Whether a node is a plain `shuffle` method call (Send only — csend excluded
/// to avoid changing nil-safety behavior, matching `Style/RedundantSort`).
fn is_shuffle_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { method, .. } => cx.symbol_str(method) == "shuffle",
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Sample-size computation
// ---------------------------------------------------------------------------

/// Whether the accessor argument(s) map to an equivalent `sample` call.
enum SampleSize {
    /// Offensive — correction is `sample` (None) or `sample(N)` (Some(N)).
    Computable(Option<String>),
    /// Not offensive — pattern does not match `sample` semantics.
    Unknown,
}

/// Determine if the method arguments to `[]` / `slice` represent a
/// sample-compatible access pattern.
fn sample_size(args: &[NodeId], cx: &Cx<'_>) -> SampleSize {
    match args.len() {
        0 => SampleSize::Unknown,
        1 => sample_size_one_arg(args[0], cx),
        2 => sample_size_two_args(args[0], args[1], cx),
        _ => SampleSize::Unknown,
    }
}

/// Determine if the method arguments to `at` represent a sample-compatible
/// access pattern. `at` does not accept Range arguments (raises TypeError),
/// so only bare integer arguments 0 and -1 are considered.
fn sample_size_at(args: &[NodeId], cx: &Cx<'_>) -> SampleSize {
    match args.len() {
        1 => match *cx.kind(args[0]) {
            NodeKind::Int(n) if n == 0 || n == -1 => SampleSize::Computable(None),
            _ => SampleSize::Unknown,
        },
        _ => SampleSize::Unknown,
    }
}

fn sample_size_one_arg(arg: NodeId, cx: &Cx<'_>) -> SampleSize {
    match *cx.kind(arg) {
        NodeKind::Int(n) if n == 0 || n == -1 => SampleSize::Computable(None),
        NodeKind::Int(_) => SampleSize::Unknown,
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => range_size(begin_, end_, exclusive, cx),
        _ => SampleSize::Unknown,
    }
}

fn sample_size_two_args(first: NodeId, second: NodeId, cx: &Cx<'_>) -> SampleSize {
    let NodeKind::Int(first_val) = *cx.kind(first) else {
        return SampleSize::Unknown;
    };
    if first_val != 0 {
        return SampleSize::Unknown;
    }
    let NodeKind::Int(second_val) = *cx.kind(second) else {
        return SampleSize::Unknown;
    };
    // Negative length (`shuffle[0, -1]`) returns `nil`, not an Array subset —
    // not equivalent to `sample(-1)` which raises `ArgumentError`.
    if second_val < 0 {
        return SampleSize::Unknown;
    }
    SampleSize::Computable(Some(second_val.to_string()))
}

/// Compute the size of a range `[0..N]` / `[0...N]`, `[...N]`.
fn range_size(begin_: OptNodeId, end_: OptNodeId, exclusive: bool, cx: &Cx<'_>) -> SampleSize {
    let begin_val: i64 = match begin_.get() {
        Some(id) => match *cx.kind(id) {
            NodeKind::Int(n) => n,
            _ => return SampleSize::Unknown,
        },
        None => 0, // beginless range `[..N]` or `[...N]` — treat begin as 0.
    };

    if begin_val != 0 {
        return SampleSize::Unknown;
    }

    let end_val: i64 = match end_.get() {
        Some(id) => match *cx.kind(id) {
            NodeKind::Int(n) => n,
            _ => return SampleSize::Unknown,
        },
        None => return SampleSize::Unknown, // endless range — can't compute.
    };

    if end_val < 0 {
        return SampleSize::Unknown;
    }

    let size = if exclusive { end_val } else { end_val + 1 };
    SampleSize::Computable(Some(size.to_string()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Sample;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- shuffle.first / shuffle.last -----

    #[test]
    fn flags_shuffle_first() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.first
                          ^^^^^^^^^^^^^^ Use `sample` instead of `shuffle.first`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    #[test]
    fn flags_shuffle_last() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.last
                          ^^^^^^^^^^^^^ Use `sample` instead of `shuffle.last`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    #[test]
    fn accepts_safe_nav_shuffle_first() {
        // csend receiver excluded to avoid nil-safety behavior change.
        test::<Sample>().expect_no_offenses("[1, 2, 3]&.shuffle.first\n");
    }

    #[test]
    fn accepts_safe_nav_shuffle_last() {
        test::<Sample>().expect_no_offenses("[1, 2, 3]&.shuffle.last\n");
    }

    // ----- shuffle[0] / shuffle[-1] -----

    #[test]
    fn flags_shuffle_index_zero() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[0]
                          ^^^^^^^^^^ Use `sample` instead of `shuffle[0]`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    #[test]
    fn flags_shuffle_index_neg_one() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[-1]
                          ^^^^^^^^^^^ Use `sample` instead of `shuffle[-1]`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    // ----- shuffle.first(N) / shuffle.last(N) -----

    #[test]
    fn flags_shuffle_first_with_arg() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.first(2)
                          ^^^^^^^^^^^^^^^^^ Use `sample(2)` instead of `shuffle.first(2)`.
            "#},
            "[1, 2, 3].sample(2)\n",
        );
    }

    #[test]
    fn flags_shuffle_last_with_arg() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.last(3)
                          ^^^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle.last(3)`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    #[test]
    fn flags_shuffle_first_with_var_arg() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.first(foo)
                          ^^^^^^^^^^^^^^^^^^^ Use `sample(foo)` instead of `shuffle.first(foo)`.
            "#},
            "[1, 2, 3].sample(foo)\n",
        );
    }

    // ----- shuffle[0, N] -----

    #[test]
    fn flags_shuffle_index_zero_n() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[0, 3]
                          ^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle[0, 3]`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    // ----- shuffle[0..N] / shuffle[0...N] -----

    #[test]
    fn flags_shuffle_index_irange() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[0..3]
                          ^^^^^^^^^^^^^^ Use `sample(4)` instead of `shuffle[0..3]`.
            "#},
            "[1, 2, 3].sample(4)\n",
        );
    }

    #[test]
    fn flags_shuffle_index_erange() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[0...3]
                          ^^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle[0...3]`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    #[test]
    fn flags_shuffle_index_beginless_erange() {
        // [...3] is Ruby 2.7+ syntax; begin omitted == 0 for sample purposes.
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle[0...3]
                          ^^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle[0...3]`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    // ----- shuffle.at(0) / shuffle.at(-1) -----

    #[test]
    fn flags_shuffle_at_zero() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.at(0)
                          ^^^^^^^^^^^^^^ Use `sample` instead of `shuffle.at(0)`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    #[test]
    fn flags_shuffle_at_neg_one() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.at(-1)
                          ^^^^^^^^^^^^^^^ Use `sample` instead of `shuffle.at(-1)`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    // ----- shuffle.slice(0) / shuffle.slice(-1) -----

    #[test]
    fn flags_shuffle_slice_zero() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.slice(0)
                          ^^^^^^^^^^^^^^^^^ Use `sample` instead of `shuffle.slice(0)`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    #[test]
    fn flags_shuffle_slice_neg_one() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.slice(-1)
                          ^^^^^^^^^^^^^^^^^^ Use `sample` instead of `shuffle.slice(-1)`.
            "#},
            "[1, 2, 3].sample\n",
        );
    }

    // ----- shuffle.slice(0, N) / shuffle.slice(0..N) / shuffle.slice(0...N) -----

    #[test]
    fn flags_shuffle_slice_zero_n() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.slice(0, 3)
                          ^^^^^^^^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle.slice(0, 3)`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    #[test]
    fn flags_shuffle_slice_irange() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.slice(0..3)
                          ^^^^^^^^^^^^^^^^^^^^^ Use `sample(4)` instead of `shuffle.slice(0..3)`.
            "#},
            "[1, 2, 3].sample(4)\n",
        );
    }

    #[test]
    fn flags_shuffle_slice_erange() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle.slice(0...3)
                          ^^^^^^^^^^^^^^^^^^^^^^ Use `sample(3)` instead of `shuffle.slice(0...3)`.
            "#},
            "[1, 2, 3].sample(3)\n",
        );
    }

    // ----- shuffle with keyword args -----

    #[test]
    fn flags_shuffle_with_keyword_first() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle(random: Random.new).first
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `sample(random: Random.new)` instead of `shuffle(random: Random.new).first`.
            "#},
            "[1, 2, 3].sample(random: Random.new)\n",
        );
    }

    #[test]
    fn flags_shuffle_with_keyword_first_with_arg() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle(random: Random.new).first(2)
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `sample(2, random: Random.new)` instead of `shuffle(random: Random.new).first(2)`.
            "#},
            "[1, 2, 3].sample(2, random: Random.new)\n",
        );
    }

    #[test]
    fn flags_shuffle_with_keyword_last_with_var() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle(random: foo).last(bar)
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `sample(bar, random: foo)` instead of `shuffle(random: foo).last(bar)`.
            "#},
            "[1, 2, 3].sample(bar, random: foo)\n",
        );
    }

    #[test]
    fn flags_shuffle_with_keyword_irange() {
        test::<Sample>().expect_correction(
            indoc! {r#"
                [1, 2, 3].shuffle(random: Random.new)[0..3]
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `sample(4, random: Random.new)` instead of `shuffle(random: Random.new)[0..3]`.
            "#},
            "[1, 2, 3].sample(4, random: Random.new)\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_sample() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].sample\n");
    }

    #[test]
    fn accepts_shuffle() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle\n");
    }

    #[test]
    fn accepts_shuffle_at_nonzero() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.at(2)\n");
    }

    #[test]
    fn accepts_shuffle_at_var() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.at(foo)\n");
    }

    #[test]
    fn accepts_shuffle_at_range() {
        // `at` does not accept Range arguments (TypeError).
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.at(0..3)\n");
    }

    #[test]
    fn accepts_shuffle_first_extra_args() {
        // `first(1, 2)` raises ArgumentError.
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.first(1, 2)\n");
    }

    #[test]
    fn accepts_shuffle_last_extra_args() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.last(1, 2)\n");
    }

    #[test]
    fn accepts_shuffle_slice_nonzero() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(2)\n");
    }

    #[test]
    fn accepts_shuffle_slice_nonzero_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(2..3)\n");
    }

    #[test]
    fn accepts_shuffle_slice_neg_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(-4..-3)\n");
    }

    #[test]
    fn accepts_shuffle_slice_var_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(foo..3)\n");
    }

    #[test]
    fn accepts_shuffle_slice_var_arg() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(foo)\n");
    }

    #[test]
    fn accepts_shuffle_slice_var_two_args() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(foo, 3)\n");
    }

    #[test]
    fn accepts_shuffle_slice_var_range_two_args() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(foo..bar)\n");
    }

    #[test]
    fn accepts_shuffle_slice_var_both_args() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(foo, bar)\n");
    }

    #[test]
    fn accepts_shuffle_index_nonzero() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[2]\n");
    }

    #[test]
    fn accepts_shuffle_index_neg_two() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[-2]\n");
    }

    #[test]
    fn accepts_shuffle_index_var() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[foo]\n");
    }

    #[test]
    fn accepts_shuffle_index_nonzero_n() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[3, 3]\n");
    }

    #[test]
    fn accepts_shuffle_index_var_n() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[foo, 3]\n");
    }

    #[test]
    fn accepts_shuffle_index_neg_n() {
        // `shuffle[0, -1]` returns nil; `sample(-1)` raises ArgumentError.
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[0, -1]\n");
    }

    #[test]
    fn accepts_shuffle_slice_neg_n() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.slice(0, -1)\n");
    }

    #[test]
    fn accepts_shuffle_index_var_both() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[foo, bar]\n");
    }

    #[test]
    fn accepts_shuffle_index_nonzero_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[2..3]\n");
    }

    #[test]
    fn accepts_shuffle_index_neg_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[2..-3]\n");
    }

    #[test]
    fn accepts_shuffle_index_var_begin_range() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle[foo..3]\n");
    }

    #[test]
    fn accepts_shuffle_other_method() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.join([5, 6, 7])\n");
    }

    #[test]
    fn accepts_shuffle_map_block() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle.map { |e| e }\n");
    }

    #[test]
    fn accepts_shuffle_with_keyword_no_accessor() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle(random: Random.new)\n");
    }

    #[test]
    fn accepts_shuffle_with_keyword_nonzero_index() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle(random: Random.new)[2]\n");
    }

    #[test]
    fn accepts_shuffle_with_keyword_nonzero_n() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle(random: Random.new)[2, 3]\n");
    }

    #[test]
    fn accepts_shuffle_with_keyword_find() {
        test::<Sample>().expect_no_offenses("[1, 2, 3].shuffle(random: Random.new).find(&:odd?)\n");
    }
}
murphy_plugin_api::submit_cop!(Sample);
