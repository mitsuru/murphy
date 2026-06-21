//! `Style/NegativeArrayIndex` — prefer negative array indices over computing
//! the array length minus a value.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NegativeArrayIndex
//! upstream_version_checked: 1.87.0
//! version_added: "1.84"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `arr[arr.size - n]`, `arr[arr.length - n]`, `arr[arr.count - n]`
//!   (`n` a positive integer) → `arr[-n]`, and the range forms
//!   `arr[0..(arr.length - n)]` / `arr[0...(arr.length - n)]` → `arr[0..-n]`.
//!   Mirrors RuboCop's `receivers_match?`: when the length call has no receiver
//!   (`self[size - 1]`) the array receiver must be `self`; otherwise both the
//!   array receiver and the length receiver must be preserving-method chains
//!   (`sort`, `reverse`, `shuffle`, `rotate`, or a bare receiver), and either
//!   their sources match or the array receiver itself has a receiver
//!   (base-receiver fallback). Range mode uses the stricter
//!   `receivers_match_strict?` (sources must match exactly, no fallback) and
//!   the begin side must also be a preserving chain. Disabled by default
//!   (`Enabled: pending` in RuboCop). Both plain send (`arr[...]`) and csend
//!   (`arr&.[](...)`) are handled, mirroring `alias on_csend on_send`; the
//!   displayed message always uses the bracket form (`arr[-1]`).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! arr[arr.count - 2]
//! arr[0..(arr.length - 2)]
//! arr[0...(arr.length - 4)]
//! arr.sort[arr.reverse.length - 2]
//! self[size - 1]
//!
//! # good
//! arr[-2]
//! arr[0..-2]
//! arr[0...-4]
//! arr.sort[-2]
//! self[-1]
//! ```
//!
//! ## Autocorrect
//!
//! Simple form: replaces the subtraction expression (`arr.size - n`) with
//! `-n`. Range form: replaces the whole index argument (`0..(arr.length - n)`)
//! with `0..-n` (or `0...-n`), preserving any wrapping parentheses.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

const PRESERVING_METHODS: [&str; 4] = ["sort", "reverse", "shuffle", "rotate"];

#[derive(Default)]
pub struct NegativeArrayIndex;

#[cop(
    name = "Style/NegativeArrayIndex",
    description = "Use negative array indices instead of calculating array length minus a value.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl NegativeArrayIndex {
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

murphy_plugin_api::submit_cop!(NegativeArrayIndex);

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(array_receiver) = cx.call_receiver(node).get() else {
        return;
    };
    let args = cx.call_arguments(node);
    let [index_arg] = args else {
        return;
    };
    let index_arg = *index_arg;

    // Range path takes precedence: `arr[0..(arr.length - n)]`.
    if let Some(range_node) = range_with_length_subtraction(index_arg, array_receiver, cx) {
        handle_range_pattern(array_receiver, range_node, index_arg, cx);
        return;
    }

    handle_simple_index_pattern(array_receiver, index_arg, cx);
}

// --- simple index: `arr[arr.size - n]` -------------------------------------

fn handle_simple_index_pattern(array_receiver: NodeId, index_arg: NodeId, cx: &Cx<'_>) {
    let Some((length_receiver, negative_index)) = length_subtraction(index_arg, cx) else {
        return;
    };
    if negative_index <= 0 {
        return;
    }
    if !receivers_match(length_receiver, array_receiver, cx) {
        return;
    }

    let receiver_src = cx.raw_source(cx.range(array_receiver));
    let index_src = cx.raw_source(cx.range(index_arg));
    let current = format!("{receiver_src}[{index_src}]");
    let message =
        format!("Use `{receiver_src}[-{negative_index}]` instead of `{current}`.");

    let offense_range = cx.range(index_arg);
    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, &format!("-{negative_index}"));
}

// --- range index: `arr[0..(arr.length - n)]` -------------------------------

struct RangeInfo {
    /// The begin side source (`0`).
    range_start_src_range: Range,
    /// `true` for `...`.
    exclusive: bool,
    /// The positive integer to negate.
    negative_index: i64,
    /// Whether the index argument is parenthesized (`begin` wrapping the range).
    index_parenthesized: bool,
    /// The `end` child of the range (may itself be a `begin`-wrapped subtraction).
    range_end: NodeId,
    /// The unwrapped subtraction send.
    inner_end: NodeId,
}

/// Returns the range info iff `index_arg` is a range whose end is a length
/// subtraction with a strict-matching, preserving receiver and whose begin is
/// also a preserving chain. Mirrors `range_with_length_subtraction?`.
fn range_with_length_subtraction(
    index_arg: NodeId,
    array_receiver: NodeId,
    cx: &Cx<'_>,
) -> Option<RangeInfo> {
    // `extract_range_from_begin`: unwrap a single `begin` (parentheses) wrapper.
    let (range_node, index_parenthesized) = match cx.kind(index_arg) {
        NodeKind::Begin(list) => match cx.list(*list) {
            [single] => (*single, true),
            _ => (index_arg, false),
        },
        _ => (index_arg, false),
    };

    let NodeKind::RangeExpr {
        begin_,
        end_,
        exclusive,
    } = *cx.kind(range_node)
    else {
        return None;
    };
    let range_start = begin_.get()?;
    let range_end = end_.get()?;

    // The begin side must be a preserving chain.
    if !preserving_method(range_start, cx) {
        return None;
    }

    // `extract_inner_end`: unwrap a single-child `begin` around the subtraction.
    let inner_end = unwrap_single_begin(range_end, cx);
    let (length_receiver, negative_index) = length_subtraction(inner_end, cx)?;
    if negative_index <= 0 {
        return None;
    }

    if !receivers_match_strict(length_receiver, array_receiver, cx) {
        return None;
    }

    Some(RangeInfo {
        range_start_src_range: cx.range(range_start),
        exclusive,
        negative_index,
        index_parenthesized,
        range_end,
        inner_end,
    })
}

fn handle_range_pattern(array_receiver: NodeId, info: RangeInfo, index_arg: NodeId, cx: &Cx<'_>) {
    let receiver_src = cx.raw_source(cx.range(array_receiver));
    let range_op = if info.exclusive { "..." } else { ".." };
    let range_start_src = cx.raw_source(info.range_start_src_range);
    let negative_index = info.negative_index;

    // `build_range_without_parens`: when the range end is itself parenthesized
    // use its full (paren-inclusive) source; otherwise use the bare inner end.
    let end_expression = if matches!(cx.kind(info.range_end), NodeKind::Begin(_)) {
        cx.raw_source(cx.range(info.range_end))
    } else {
        cx.raw_source(cx.range(info.inner_end))
    };
    let range_without_parens = format!("{range_start_src}{range_op}{end_expression}");

    // `current` and message-part assembly mirror RuboCop's `format_range_*`.
    let current = if info.index_parenthesized {
        format!("{receiver_src}[({range_without_parens})]")
    } else {
        format!("{receiver_src}[{range_without_parens}]")
    };

    let (start, index_part) = if info.index_parenthesized {
        (
            format!("({range_start_src}"),
            format!("{negative_index})"),
        )
    } else {
        (range_start_src.to_string(), negative_index.to_string())
    };

    let message = format!(
        "Use `{receiver_src}[{start}{range_op}-{index_part}]` instead of `{current}`."
    );

    let replacement = if info.index_parenthesized {
        format!("({range_start_src}{range_op}-{negative_index})")
    } else {
        format!("{range_start_src}{range_op}-{negative_index}")
    };

    // Offense location is the range end (`(arr.length - n)`), but the edit
    // replaces the whole index argument.
    cx.emit_offense(cx.range(info.range_end), &message, None);
    cx.emit_edit(cx.range(index_arg), &replacement);
}

// --- shared helpers --------------------------------------------------------

/// Matches `(send (send $recv {length|size|count}) :- (int $n))`.
/// Returns `(length_receiver, n)`. `length_receiver` is `None` for a bare
/// length call (`size - 1`).
fn length_subtraction(node: NodeId, cx: &Cx<'_>) -> Option<(OptNodeId, i64)> {
    if cx.method_name(node) != Some("-") {
        return None;
    }
    let sub_receiver = cx.call_receiver(node).get()?;
    let args = cx.call_arguments(node);
    let [arg] = args else {
        return None;
    };
    let NodeKind::Int(n) = *cx.kind(*arg) else {
        return None;
    };

    let length_method = cx.method_name(sub_receiver)?;
    if !matches!(length_method, "length" | "size" | "count") {
        return None;
    }
    // The length call must take no arguments (`arr.count(x)` is not a length).
    if !cx.call_arguments(sub_receiver).is_empty() {
        return None;
    }

    Some((cx.call_receiver(sub_receiver), n))
}

/// Mirrors `receivers_match?` for the simple-index path.
fn receivers_match(length_receiver: OptNodeId, array_receiver: NodeId, cx: &Cx<'_>) -> bool {
    let Some(length_receiver) = length_receiver.get() else {
        // `return array_receiver.self_type? unless length_receiver`
        return matches!(cx.kind(array_receiver), NodeKind::SelfExpr);
    };

    if !(preserving_method(array_receiver, cx) && preserving_method(length_receiver, cx)) {
        return false;
    }
    if cx.raw_source(cx.range(length_receiver)) == cx.raw_source(cx.range(array_receiver)) {
        return true;
    }
    // `!extract_base_receiver(array_receiver).nil?` — true iff the array
    // receiver itself has a receiver.
    has_receiver(array_receiver, cx)
}

/// Mirrors `receivers_match_strict?` for the range path.
fn receivers_match_strict(
    length_receiver: OptNodeId,
    array_receiver: NodeId,
    cx: &Cx<'_>,
) -> bool {
    let Some(length_receiver) = length_receiver.get() else {
        return false;
    };
    preserving_method(array_receiver, cx)
        && cx.raw_source(cx.range(length_receiver)) == cx.raw_source(cx.range(array_receiver))
}

/// Mirrors `preserving_method?`: true if the node has no receiver, or its
/// method is a preserving method and its receiver also preserves.
fn preserving_method(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(receiver) = cx.call_receiver(node).get() else {
        return true;
    };
    let Some(method_name) = cx.method_name(node) else {
        return false;
    };
    if !PRESERVING_METHODS.contains(&method_name) {
        return false;
    }
    preserving_method(receiver, cx)
}

/// `extract_base_receiver(node).nil?` is false iff `node` has a receiver.
fn has_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_receiver(node).get().is_some()
}

/// `extract_inner_end`: unwrap a single-child `begin` (parentheses).
fn unwrap_single_begin(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = cx.kind(node)
        && let [single] = cx.list(*list)
    {
        return *single;
    }
    node
}

#[cfg(test)]
mod tests {
    use super::NegativeArrayIndex;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- simple index ---

    #[test]
    fn flags_size_minus_one() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[arr.size - 1]
                    ^^^^^^^^^^^^ Use `arr[-1]` instead of `arr[arr.size - 1]`.
            "#},
            "arr[-1]\n",
        );
    }

    #[test]
    fn flags_length_minus_two() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[arr.length - 2]
                    ^^^^^^^^^^^^^^ Use `arr[-2]` instead of `arr[arr.length - 2]`.
            "#},
            "arr[-2]\n",
        );
    }

    #[test]
    fn flags_count_minus_three() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[arr.count - 3]
                    ^^^^^^^^^^^^^ Use `arr[-3]` instead of `arr[arr.count - 3]`.
            "#},
            "arr[-3]\n",
        );
    }

    #[test]
    fn flags_ivar_receiver() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                @foo[@foo.size - 1]
                     ^^^^^^^^^^^^^ Use `@foo[-1]` instead of `@foo[@foo.size - 1]`.
            "#},
            "@foo[-1]\n",
        );
    }

    #[test]
    fn flags_self_bare_size() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                self[size - 1]
                     ^^^^^^^^ Use `self[-1]` instead of `self[size - 1]`.
            "#},
            "self[-1]\n",
        );
    }

    #[test]
    fn flags_preserving_chain_matching_source() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr.sort[arr.sort.length - 2]
                         ^^^^^^^^^^^^^^^^^^^ Use `arr.sort[-2]` instead of `arr.sort[arr.sort.length - 2]`.
            "#},
            "arr.sort[-2]\n",
        );
    }

    #[test]
    fn flags_preserving_chain_base_receiver_fallback() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr.sort[arr.reverse.length - 2]
                         ^^^^^^^^^^^^^^^^^^^^^^ Use `arr.sort[-2]` instead of `arr.sort[arr.reverse.length - 2]`.
            "#},
            "arr.sort[-2]\n",
        );
    }

    #[test]
    fn flags_csend() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr&.[](arr.size - 1)
                        ^^^^^^^^^^^^ Use `arr[-1]` instead of `arr[arr.size - 1]`.
            "#},
            "arr&.[](-1)\n",
        );
    }

    #[test]
    fn flags_const_with_scope_receiver() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                Foo::Bar[Foo::Bar.size - 1]
                         ^^^^^^^^^^^^^^^^^ Use `Foo::Bar[-1]` instead of `Foo::Bar[Foo::Bar.size - 1]`.
            "#},
            "Foo::Bar[-1]\n",
        );
    }

    #[test]
    fn flags_gvar_receiver() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                $global[$global.size - 1]
                        ^^^^^^^^^^^^^^^^ Use `$global[-1]` instead of `$global[$global.size - 1]`.
            "#},
            "$global[-1]\n",
        );
    }

    #[test]
    fn flags_cvar_receiver() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                @@cvar[@@cvar.size - 1]
                       ^^^^^^^^^^^^^^^ Use `@@cvar[-1]` instead of `@@cvar[@@cvar.size - 1]`.
            "#},
            "@@cvar[-1]\n",
        );
    }

    // --- simple index: non-offenses ---

    #[test]
    fn no_offense_zero_index() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[arr.size - 0]\n");
    }

    #[test]
    fn no_offense_plus() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[arr.size + 1]\n");
    }

    #[test]
    fn no_offense_float_index() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[arr.size - 1.0]\n");
    }

    #[test]
    fn no_offense_receiver_mismatch() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[other.size - 1]\n");
    }

    #[test]
    fn no_offense_non_preserving_length_chain() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr.sort[arr.foo.length - 2]\n");
    }

    #[test]
    fn no_offense_non_preserving_array_chain() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr.foo[arr.foo.length - 2]\n");
    }

    #[test]
    fn no_offense_bare_size_non_self() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[size - 1]\n");
    }

    #[test]
    fn no_offense_plain_index() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[1]\n");
    }

    // --- range index ---

    #[test]
    fn flags_inclusive_range_parenthesized() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[0..(arr.length - 2)]
                       ^^^^^^^^^^^^^^^^ Use `arr[0..-2]` instead of `arr[0..(arr.length - 2)]`.
            "#},
            "arr[0..-2]\n",
        );
    }

    #[test]
    fn flags_exclusive_range_parenthesized() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[0...(arr.length - 4)]
                        ^^^^^^^^^^^^^^^^ Use `arr[0...-4]` instead of `arr[0...(arr.length - 4)]`.
            "#},
            "arr[0...-4]\n",
        );
    }

    #[test]
    fn flags_inclusive_range_no_parens() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[0..arr.length - 2]
                       ^^^^^^^^^^^^^^ Use `arr[0..-2]` instead of `arr[0..arr.length - 2]`.
            "#},
            "arr[0..-2]\n",
        );
    }

    #[test]
    fn flags_range_nonzero_start() {
        test::<NegativeArrayIndex>().expect_correction(
            indoc! {r#"
                arr[1..(arr.length - 2)]
                       ^^^^^^^^^^^^^^^^ Use `arr[1..-2]` instead of `arr[1..(arr.length - 2)]`.
            "#},
            "arr[1..-2]\n",
        );
    }

    // --- range index: non-offenses ---

    #[test]
    fn no_offense_range_receiver_mismatch() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[0..(other.length - 2)]\n");
    }

    #[test]
    fn no_offense_range_zero_index() {
        test::<NegativeArrayIndex>().expect_no_offenses("arr[0..(arr.length - 0)]\n");
    }
}
