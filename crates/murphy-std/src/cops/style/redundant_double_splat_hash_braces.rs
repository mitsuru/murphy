//! `Style/RedundantDoubleSplatHashBraces` — flags redundant uses of double
//! splat hash braces in method calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantDoubleSplatHashBraces
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Detection: `**{key: val, ...}` where the hash has non-empty
//!       symbol-key pairs (no hash rockets), is brace-delimited,
//!       is inside a kwsplat ancestor, and the parent chain is
//!       call/csend or kwsplat (mergeable?).
//!     - Autocorrect for the simple case: `**{foo: bar, baz: qux}` →
//!       `foo: bar, baz: qux` (removes `**`, `{`, and `}`).
//!     - The offense range is the kwsplat node.
//!   Gaps:
//!     - Autocorrect for the `.merge`/`.merge!` chain shape
//!       (`**{foo: bar}.merge(options)` → `foo: bar, **options`) is not
//!       implemented. The rewrite is correct in RuboCop because it
//!       understands that all merge args are kwargs-compatible, but
//!       implementing it safely in Murphy requires reconstructing the
//!       full argument list — deferred to a follow-up. The offense is
//!       still emitted (no autocorrect edit for that shape).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantDoubleSplatHashBraces;

const MSG: &str = "Remove the redundant double splat and braces, use keyword arguments directly.";

/// Method names that are considered "mergeable" on a call node.
const MERGE_METHODS: &[&str] = &["merge", "merge!"];

#[cop(
    name = "Style/RedundantDoubleSplatHashBraces",
    description = "Checks for redundant uses of double splat hash braces.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantDoubleSplatHashBraces {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must have at least one pair, and none may be hash-rocket style.
    let pairs = cx.hash_pairs(node);
    if pairs.is_empty() {
        return;
    }
    if pairs.iter().any(|&p| cx.is_hash_rocket(p)) {
        return;
    }

    // The hash's parent must be a kwsplat or a call/csend (mergeable chain).
    let Some(parent_id) = cx.parent(node).get() else {
        return;
    };
    if !mergeable(parent_id, cx) {
        return;
    }

    // There must be a kwsplat ancestor.
    let kwsplat = cx
        .ancestors(node)
        .find(|&a| matches!(cx.kind(a), NodeKind::Kwsplat(_)));
    let Some(kwsplat) = kwsplat else {
        return;
    };

    // The hash must be brace-delimited (has LeftBrace + RightBrace tokens
    // within its range — the same check as `node.braces?` in RuboCop).
    if !hash_has_braces(node, cx) {
        return;
    }

    // The kwsplat's first child must not be a block, and if it is a call,
    // the root receiver must not be a hash (mirrors `allowed_double_splat_receiver?`).
    if allowed_double_splat_receiver(kwsplat, cx) {
        return;
    }

    cx.emit_offense(cx.range(kwsplat), MSG, None);

    // Autocorrect only the simple (no-merge) shape.
    // For the merge shape the offense is emitted but no edit is applied.
    if is_simple_shape(kwsplat, node, cx) {
        autocorrect_simple(kwsplat, node, cx);
    }
}

/// Returns true if `node` (the kwsplat's first child) is a block, or if it
/// is a call whose root receiver is a hash — those shapes are allowed.
/// Mirrors RuboCop's `allowed_double_splat_receiver?`.
fn allowed_double_splat_receiver(kwsplat: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Kwsplat(child_opt) = *cx.kind(kwsplat) else {
        return false;
    };
    let Some(child) = child_opt.get() else {
        return false;
    };
    match cx.kind(child) {
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => true,
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            // Walk to the root receiver of the call chain.
            let root = root_receiver(child, cx);
            // If the root receiver is a hash literal, this is the merge shape:
            // `**{foo: bar}.merge(opts)`. That's NOT allowed_double_splat_receiver.
            // If root is nil (no receiver) or not a hash, it IS allowed (skip offense).
            match root {
                Some(r) => !matches!(cx.kind(r), NodeKind::Hash(_)),
                None => true, // no receiver at the root — e.g. `**foo.merge(x)` where foo is lvar
            }
        }
        _ => false,
    }
}

/// Walk the receiver chain to the bottom-most receiver of a call chain.
fn root_receiver(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.call_receiver(node).get() {
        None => None,
        Some(recv) => match cx.kind(recv) {
            NodeKind::Send { .. } | NodeKind::Csend { .. } => {
                // Recurse: there is a deeper receiver.
                root_receiver(recv, cx).or(Some(recv))
            }
            _ => Some(recv),
        },
    }
}

/// Returns true if the hash node has `{` and `}` tokens within its range.
fn hash_has_braces(node: NodeId, cx: &Cx<'_>) -> bool {
    let toks = cx.tokens_in(cx.range(node));
    toks.iter().any(|t| t.kind == SourceTokenKind::LeftBrace)
        && toks.iter().any(|t| t.kind == SourceTokenKind::RightBrace)
}

/// `mergeable?` from RuboCop: a node is mergeable if it is a kwsplat, or
/// a call to `merge`/`merge!` whose parent is also mergeable.
fn mergeable(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Kwsplat(_) => true,
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            let method = cx.method_name(node).unwrap_or("");
            if !MERGE_METHODS.contains(&method) {
                return false;
            }
            match cx.parent(node).get() {
                Some(p) => mergeable(p, cx),
                None => false,
            }
        }
        _ => false,
    }
}

/// Returns true if the kwsplat's direct child is the hash itself —
/// i.e. `**{...}` with no `.merge` call in between.
fn is_simple_shape(kwsplat: NodeId, hash: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Kwsplat(child_opt) = *cx.kind(kwsplat) else {
        return false;
    };
    child_opt.get() == Some(hash)
}

/// Autocorrect for the simple shape `**{foo: bar, baz: qux}`:
/// - Remove the `**` operator (kwsplat.start .. hash.start)
/// - Remove the opening `{` (and any space between `{` and first pair)
/// - Remove the closing `}` (and any space between last pair and `}`)
fn autocorrect_simple(kwsplat: NodeId, hash: NodeId, cx: &Cx<'_>) {
    // Edit 1: remove `**` — from kwsplat start to hash start.
    let double_splat = murphy_plugin_api::Range {
        start: cx.range(kwsplat).start,
        end: cx.range(hash).start,
    };
    cx.emit_edit(double_splat, "");

    // Find the LeftBrace and RightBrace tokens within the hash range.
    let toks = cx.tokens_in(cx.range(hash));
    if let Some(lbrace) = toks.iter().find(|t| t.kind == SourceTokenKind::LeftBrace) {
        // Edit 2: remove `{` + any whitespace up to the first pair.
        let pairs = cx.hash_pairs(hash);
        let lbrace_removal_end = if let Some(&first_pair) = pairs.first() {
            cx.range(first_pair).start
        } else {
            lbrace.range.end
        };
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: lbrace.range.start,
                end: lbrace_removal_end,
            },
            "",
        );
    }
    if let Some(rbrace) = toks
        .iter()
        .rev()
        .find(|t| t.kind == SourceTokenKind::RightBrace)
    {
        // Edit 3: remove any whitespace before `}` + the `}` itself.
        let pairs = cx.hash_pairs(hash);
        let rbrace_removal_start = if let Some(&last_pair) = pairs.last() {
            cx.range(last_pair).end
        } else {
            rbrace.range.start
        };
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: rbrace_removal_start,
                end: rbrace.range.end,
            },
            "",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantDoubleSplatHashBraces;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_empty_hash() {
        // Empty hash: no pairs, so no offense.
        test::<RedundantDoubleSplatHashBraces>().expect_no_offenses("do_something(**{})");
    }

    #[test]
    fn no_offense_hash_rocket_pair() {
        // Hash with hash-rocket pair: not flagged.
        test::<RedundantDoubleSplatHashBraces>()
            .expect_no_offenses(r#"do_something(**{"foo" => bar})"#);
    }

    #[test]
    fn no_offense_no_braces() {
        // **variable (no hash literal at all).
        test::<RedundantDoubleSplatHashBraces>().expect_no_offenses("do_something(**options)");
    }

    #[test]
    fn no_offense_variable_merge() {
        // `**foo.merge(x)` — root receiver is not a hash literal.
        test::<RedundantDoubleSplatHashBraces>()
            .expect_no_offenses("do_something(**foo.merge(options))");
    }

    #[test]
    fn no_offense_hash_value_in_pair() {
        // A hash used as a pair value: parent is Pair, not kwsplat or call.
        test::<RedundantDoubleSplatHashBraces>().expect_no_offenses("x = {a: {b: 1}}");
    }

    // --- Offense cases: simple shape ---

    #[test]
    fn flags_simple_double_splat_hash() {
        test::<RedundantDoubleSplatHashBraces>().expect_offense(indoc! {r#"
            do_something(**{foo: bar, baz: qux})
                         ^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant double splat and braces, use keyword arguments directly.
        "#});
    }

    #[test]
    fn flags_single_pair() {
        test::<RedundantDoubleSplatHashBraces>().expect_offense(indoc! {r#"
            f(**{a: 1})
              ^^^^^^^^ Remove the redundant double splat and braces, use keyword arguments directly.
        "#});
    }

    // --- Offense cases: merge shape (offense emitted, no autocorrect) ---

    #[test]
    fn flags_merge_shape() {
        test::<RedundantDoubleSplatHashBraces>().expect_offense(indoc! {r#"
            do_something(**{foo: bar, baz: qux}.merge(options))
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant double splat and braces, use keyword arguments directly.
        "#});
    }

    // --- Autocorrect: simple shape ---

    #[test]
    fn corrects_simple_double_splat_hash() {
        test::<RedundantDoubleSplatHashBraces>().expect_correction(
            indoc! {r#"
                do_something(**{foo: bar, baz: qux})
                             ^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant double splat and braces, use keyword arguments directly.
            "#},
            "do_something(foo: bar, baz: qux)\n",
        );
    }

    #[test]
    fn corrects_single_pair() {
        test::<RedundantDoubleSplatHashBraces>().expect_correction(
            indoc! {r#"
                f(**{a: 1})
                  ^^^^^^^^ Remove the redundant double splat and braces, use keyword arguments directly.
            "#},
            "f(a: 1)\n",
        );
    }

    #[test]
    fn corrects_idempotent() {
        // After correction the result has no offense.
        test::<RedundantDoubleSplatHashBraces>()
            .expect_no_offenses("do_something(foo: bar, baz: qux)");
    }
}

murphy_plugin_api::submit_cop!(RedundantDoubleSplatHashBraces);
