//! `Metrics/PerceivedComplexity` — flag methods/`define_method` blocks whose
//! *perceived* complexity (a human-reader-oriented variant of cyclomatic
//! complexity) exceeds `Max`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/PerceivedComplexity
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "0.81"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop's `PerceivedComplexity < CyclomaticComplexity` overrides only
//!   `COUNTED_NODES` and `complexity_score_for`; everything else (the
//!   `MethodComplexity` mixin, measured scopes, the `each_node` walk, iterating
//!   blocks, the repeated-csend discount, `define_method` block detection,
//!   `AllowedMethods`/`AllowedPatterns`) is inherited verbatim from
//!   `Metrics/CyclomaticComplexity`. This port mirrors that: the measured-scope
//!   and walk machinery is identical to murphy's `CyclomaticComplexity`; only
//!   the per-node score differs. Verified numerically against standalone
//!   rubocop 1.87.0 (`--only Metrics/PerceivedComplexity`, `Max: 0`).
//!
//!   COUNTED_NODES = `CyclomaticComplexity::COUNTED_NODES - [:when] + [:case]`:
//!   drop `when` (now scores 0), add `case`. So vs CyclomaticComplexity:
//!   - `if`: scores 2 when it has an `else` (`else?`) and is not itself an
//!     `elsif` (`!elsif?`), else 1. RuboCop's `else?` is `loc?(:else)`, which
//!     is true even when the else-branch is an `elsif` (an `if a/elsif b/end`
//!     outer `if` scores 2); murphy's `cx.is_else` matches that. Ternary,
//!     modifier-`if`, plain `if`, and `unless`-without-else all score 1; an
//!     `unless`-with-`else` scores 2 (keyword is `unless`, not `elsif`).
//!   - `case` (subject form, `when` arms): `nb_branches = when count + (else ?
//!     1 : 0)`. A no-subject `case` (`case; when …`) is just if/elsif sugar, so
//!     it scores `nb_branches`. A subject `case` scores `((nb_branches * 0.2) +
//!     0.8).round`, computed exactly as the integer `(nb_branches + 6) / 5`
//!     (Ruby `.round` is round-half-up but no integer `nb_branches` lands on a
//!     half here). `case/in` is a distinct node (`case_match`/`in_pattern`),
//!     NOT in COUNTED_NODES: the wrapper scores 0 and each `in_pattern` scores
//!     1 — identical to CyclomaticComplexity.
//!   - `when`: NOT counted (scores 0); the `case` formula accounts for arms.
//!   - everything else (`while until for csend block block_pass rescue
//!     in_pattern and or or_asgn and_asgn`): inherited from
//!     CyclomaticComplexity via `super` — iterating-block / block_pass scoring
//!     and the repeated-csend discount are unchanged.
//!
//!   Fires when score > Max (default 8). Message:
//!   "Perceived complexity for `name` is too high. [score/Max]".
//!   Offense range is the whole measured node. No autocorrect.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 8) — score 9 (verified against rubocop 1.87.0)
//! def complicated_method(x)
//!   if x > 0                      # outer if has an elsif/else -> +2
//!     case x                      # subject case, 8 whens -> (8+6)/5 = +2
//!     when 1 then a
//!     when 2 then b
//!     when 3 then c
//!     when 4 then d
//!     when 5 then e
//!     when 6 then f
//!     when 7 then g
//!     when 8 then h
//!     end
//!   elsif y                       # +1 (elsif)
//!     foo until z && w && v       # +1 until, +1 &&, +1 &&
//!   else
//!     bar
//!   end
//! end                            # base 1 + 2 + 2 + 1 + 3 = 9 complexity points
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};
use std::collections::HashMap;

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct PerceivedComplexity;

/// Options for [`PerceivedComplexity`]. Defaults mirror RuboCop's `default.yml`
/// (`Max: 8`, empty allow-lists).
#[derive(CopOptions)]
pub struct PerceivedComplexityOptions {
    #[option(
        name = "Max",
        default = 8,
        description = "Maximum allowed perceived complexity for a method."
    )]
    pub max: i64,
    #[option(
        name = "AllowedMethods",
        description = "Methods to ignore when measuring perceived complexity."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        description = "Method-name patterns to ignore when measuring perceived complexity."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Metrics/PerceivedComplexity",
    description = "A complexity metric geared towards measuring complexity for a human reader.",
    default_severity = "warning",
    default_enabled = true,
    options = PerceivedComplexityOptions,
)]
impl PerceivedComplexity {
    /// RuboCop `on_def`.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(name) = cx.method_name(node) else {
            return;
        };
        check_complexity(node, name, cx.def_body(node).get(), cx);
    }

    /// RuboCop `alias on_defs on_def`.
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(name) = cx.method_name(node) else {
            return;
        };
        check_complexity(node, name, cx.def_body(node).get(), cx);
    }

    /// RuboCop `on_block`: only `define_method(:name) { ... }` blocks are
    /// measured (the `define_method?` node-matcher).
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_numblock on_block`: `define_method(:name) { _1 }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_itblock on_block`: `define_method(:name) { it }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }
}

/// RuboCop `on_block` body: measure the block only when it matches the
/// `define_method?` node-matcher. Shared by `block`/`numblock`/`itblock`.
fn check_define_method_block(node: NodeId, cx: &Cx<'_>) {
    let Some(name) = define_method_name(node, cx) else {
        return;
    };
    check_complexity(node, name, cx.block_body(node).get(), cx);
}

/// RuboCop `define_method?`: `(any_block (send nil? :define_method ({sym str} $_)) _ _)`.
/// Returns the captured method name if `node` is a `define_method` block with a
/// single symbol/string argument and no receiver.
fn define_method_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let call = cx.block_call(node).get()?;
    if cx.method_name(call) != Some("define_method") {
        return None;
    }
    if cx.call_receiver(call).get().is_some() {
        return None;
    }
    let args = cx.call_arguments(call);
    let [arg] = args else {
        return None;
    };
    match cx.kind(*arg) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(*sym)),
        NodeKind::Str(s) => Some(cx.string_str(*s)),
        _ => None,
    }
}

/// RuboCop `check_complexity`: skip empty bodies, compute the score, and emit an
/// offense when it exceeds `Max`. Mirrors `Metrics/CyclomaticComplexity`'s
/// inherited `MethodComplexity` machinery.
fn check_complexity(node: NodeId, method_name: &str, body: Option<NodeId>, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<PerceivedComplexityOptions>();

    // RuboCop checks AllowedMethods/AllowedPatterns before measuring.
    if opts.allowed_methods.iter().any(|m| m == method_name)
        || cx.matches_any_pattern(method_name, &opts.allowed_patterns)
    {
        return;
    }

    // RuboCop: "Accepts empty methods always."
    let Some(body) = body else {
        return;
    };

    let score = complexity(body, cx);
    if score <= opts.max {
        return;
    }

    let message = format!(
        "Perceived complexity for `{method_name}` is too high. [{score}/{}]",
        opts.max
    );
    cx.emit_offense(cx.range(node), &message, None);
}

/// RuboCop `complexity`: score starts at 1, then walks `body.each_node(:lvasgn,
/// *COUNTED_NODES)` in pre-order DFS. `lvasgn` nodes reset the repeated-csend
/// tracking without scoring; all other counted nodes add `complexity_score_for`.
fn complexity(body: NodeId, cx: &Cx<'_>) -> i64 {
    let mut score: i64 = 1;
    // `discount_for_repeated_csend?` state: lvar name -> the first csend NodeId
    // seen on that variable since the last assignment to it.
    let mut repeated_csend: HashMap<&str, NodeId> = HashMap::new();

    // RuboCop's `each_node` yields `self` first if it matches, then descendants.
    // `cx.descendants` excludes the root, so prepend the body node itself.
    let nodes = std::iter::once(body).chain(cx.descendants(body));

    for n in nodes {
        match cx.kind(n) {
            // `lvasgn`: never scores; resets repeated-csend for that variable.
            NodeKind::Lvasgn { name, .. } => {
                repeated_csend.remove(cx.symbol_str(*name));
            }
            _ => {
                score += complexity_score_for(n, &mut repeated_csend, cx);
            }
        }
    }
    score
}

/// RuboCop `PerceivedComplexity#complexity_score_for`: overrides `if` and
/// `case`; delegates everything else to `super` (the CyclomaticComplexity
/// scoring). `when` is no longer in COUNTED_NODES and so scores 0.
fn complexity_score_for<'a>(
    node: NodeId,
    repeated_csend: &mut HashMap<&'a str, NodeId>,
    cx: &Cx<'a>,
) -> i64 {
    match cx.kind(node) {
        // `if`: `node.else? && !node.elsif? ? 2 : 1`. `cx.is_else` mirrors
        // RuboCop's `loc?(:else)` (true even when the else-branch is an elsif).
        NodeKind::If { .. } => {
            if cx.is_else(node) && !cx.is_elsif(node) {
                2
            } else {
                1
            }
        }

        // `case` (subject/`when` form only — `case_match` is a distinct kind).
        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            let nb_branches = cx.list(*whens).len() as i64 + i64::from(else_.get().is_some());
            if subject.get().is_none() {
                // No-subject case: each `when` counts (if/elsif sugar).
                nb_branches
            } else {
                // `((nb_branches * 0.2) + 0.8).round`, computed exactly in
                // integer arithmetic (no integer nb_branches lands on a half).
                (nb_branches + 6) / 5
            }
        }

        // Everything else delegates to CyclomaticComplexity's scoring (`super`).
        _ => super_complexity_score_for(node, repeated_csend, cx),
    }
}

/// RuboCop `CyclomaticComplexity#complexity_score_for` — the `super` call from
/// `PerceivedComplexity#complexity_score_for`. `when` is NOT counted here
/// (removed from `COUNTED_NODES`); `case`/`if` never reach this path. Identical
/// to murphy's `Metrics/CyclomaticComplexity` for the shared node kinds.
fn super_complexity_score_for<'a>(
    node: NodeId,
    repeated_csend: &mut HashMap<&'a str, NodeId>,
    cx: &Cx<'a>,
) -> i64 {
    match cx.kind(node) {
        // COUNTED_NODES that always score 1 (`when` deliberately excluded).
        NodeKind::While { .. }
        | NodeKind::Until { .. }
        | NodeKind::For { .. }
        | NodeKind::Rescue { .. }
        | NodeKind::InPattern { .. }
        | NodeKind::And { .. }
        | NodeKind::Or { .. }
        | NodeKind::OrAsgn { .. }
        | NodeKind::AndAsgn { .. } => 1,

        // `block` scores 1 only when its method is a known iterating method.
        // (numblock/itblock are NOT in COUNTED_NODES — they never score.)
        NodeKind::Block { .. } => i64::from(is_iterating_block(node, cx)),

        // `block_pass` (`&:sym`) scores 1 only when its enclosing call's method
        // is iterating.
        NodeKind::BlockPass(_) => i64::from(is_iterating_block_pass(node, cx)),

        // `csend` (`&.`) scores 1 unless it is a repeated safe-navigation on the
        // same local variable since the last assignment.
        NodeKind::Csend { .. } => i64::from(!discount_for_repeated_csend(node, repeated_csend, cx)),

        _ => 0,
    }
}

/// RuboCop `iterating_block?` for a `block` node: its method name is in
/// `KNOWN_ITERATING_METHODS`.
fn is_iterating_block(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(node).is_some_and(is_iterating_method)
}

/// RuboCop `iterating_block?` for a `block_pass` node: the method name of the
/// call it is an argument to is in `KNOWN_ITERATING_METHODS` (RuboCop keys on
/// `node.parent.method_name`).
fn is_iterating_block_pass(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node)
        .get()
        .and_then(|parent| cx.method_name(parent))
        .is_some_and(is_iterating_method)
}

/// RuboCop `discount_for_repeated_csend?`: returns true (discount to 0) when
/// `node` is a `csend` whose receiver is a local variable that already had a
/// safe-navigation since its last assignment. The first such csend per variable
/// records itself and is NOT discounted.
fn discount_for_repeated_csend<'a>(
    node: NodeId,
    repeated_csend: &mut HashMap<&'a str, NodeId>,
    cx: &Cx<'a>,
) -> bool {
    let NodeKind::Csend { receiver, .. } = cx.kind(node) else {
        return false;
    };
    let NodeKind::Lvar(var) = cx.kind(*receiver) else {
        return false;
    };
    let var_name = cx.symbol_str(*var);
    match repeated_csend.get(var_name) {
        // Seen before on this variable → discount (unless it is somehow the very
        // same node, which never recurs in a single traversal).
        Some(&seen) => seen != node,
        // First csend on this variable → record and count.
        None => {
            repeated_csend.insert(var_name, node);
            false
        }
    }
}

/// RuboCop `KNOWN_ITERATING_METHODS` (`IteratingBlock`): the union of the
/// enumerable, enumerator, array, and hash iterating-method name sets. Copied
/// verbatim from rubocop 1.87.0 (deduped — `sort`/`sort_by` appear in multiple
/// upstream groups; `times` is deliberately absent).
fn is_iterating_method(name: &str) -> bool {
    matches!(
        name,
        // enumerable
        "all?" | "any?" | "chain" | "chunk" | "chunk_while" | "collect"
            | "collect_concat" | "count" | "cycle" | "detect" | "drop"
            | "drop_while" | "each" | "each_cons" | "each_entry" | "each_slice"
            | "each_with_index" | "each_with_object" | "entries" | "filter"
            | "filter_map" | "find" | "find_all" | "find_index" | "flat_map"
            | "grep" | "grep_v" | "group_by" | "inject" | "lazy" | "map"
            | "max" | "max_by" | "min" | "min_by" | "minmax" | "minmax_by"
            | "none?" | "one?" | "partition" | "reduce" | "reject"
            | "reverse_each" | "select" | "slice_after" | "slice_before"
            | "slice_when" | "sort" | "sort_by" | "sum" | "take" | "take_while"
            | "tally" | "to_h" | "uniq" | "zip"
            // enumerator
            | "with_index" | "with_object"
            // array
            // NOTE: `d_permutation` and `repeat` are upstream typos in
            // rubocop 1.87.0's `IteratingBlock::KNOWN_ITERATING_METHODS`
            // (`repeated_permutation` was split into `repeat` + `d_permutation`).
            // Reproduced verbatim for parity — do NOT "fix" to
            // `repeated_permutation` or that would diverge from rubocop.
            | "bsearch" | "bsearch_index" | "collect!" | "combination"
            | "d_permutation" | "delete_if" | "each_index" | "keep_if"
            | "map!" | "permutation" | "product" | "reject!" | "repeat"
            | "repeated_combination" | "select!" | "sort!"
            // hash
            | "each_key" | "each_pair" | "each_value" | "fetch"
            | "fetch_values" | "has_key?" | "merge" | "merge!"
            | "transform_keys" | "transform_keys!" | "transform_values"
            | "transform_values!"
    )
}

#[cfg(test)]
mod tests {
    use super::{PerceivedComplexity, PerceivedComplexityOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(max: i64) -> PerceivedComplexityOptions {
        PerceivedComplexityOptions {
            max,
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    // All numeric scores below verified against standalone rubocop 1.87.0
    // (`--only Metrics/PerceivedComplexity`, `Max: 0`).

    #[test]
    fn if_with_else_scores_two() {
        // if/else (not elsif) → if scores 2 → 1 + 2 = 3. (rubocop f1)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f1; if a; 1; else; 2; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f1` is too high. [3/2]
            "});
    }

    #[test]
    fn if_elsif_else_chain() {
        // outer if else? & !elsif? → +2; inner elsif → +1 → 1+2+1 = 4. (rubocop f2)
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def f2; if a; 1; elsif b; 2; else; 3; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f2` is too high. [4/3]
            "});
    }

    #[test]
    fn if_elsif_no_final_else() {
        // outer if has an elsif branch → else? true (loc?(:else)) → +2;
        // inner elsif → +1 → 1+2+1 = 4. (rubocop f3) This is the structural
        // trap: `(if a 1 (if b 2 nil))` must score 4, not 3.
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def f3; if a; 1; elsif b; 2; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f3` is too high. [4/3]
            "});
    }

    #[test]
    fn plain_if_scores_one() {
        // if without else → +1 → 2. (rubocop f4)
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def f4; if a; 1; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f4` is too high. [2/1]
            "});
    }

    #[test]
    fn unless_without_else_scores_one() {
        // unless, no else → +1 → 2. The else slot holds the body but there is
        // no `else` keyword, so else? is false. (rubocop f5)
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def f5; unless a; 1; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f5` is too high. [2/1]
            "});
    }

    #[test]
    fn unless_with_else_scores_two() {
        // unless/else → else? true, keyword `unless` (not elsif) → +2 → 3.
        // (rubocop f6)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f6; unless a; 1; else; 2; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f6` is too high. [3/2]
            "});
    }

    #[test]
    fn ternary_scores_one() {
        // ternary → +1 → 2 (no `else` keyword). (rubocop f7)
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def f7; a ? b : c; end
                ^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f7` is too high. [2/1]
            "});
    }

    #[test]
    fn subject_case_four_whens() {
        // subject case, 4 whens, no else → nb=4 → (4+6)/5 = 2 → 1+2 = 3.
        // (rubocop f8)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f8; case var; when 1 then a; when 2 then b; when 3 then c; when 4 then d; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f8` is too high. [3/2]
            "});
    }

    #[test]
    fn subject_case_four_whens_plus_else() {
        // subject case, 4 whens + else → nb=5 → (5+6)/5 = 2 → 1+2 = 3.
        // (rubocop f9)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f9; case var; when 1 then a; when 2 then b; when 3 then c; when 4 then d; else e; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f9` is too high. [3/2]
            "});
    }

    #[test]
    fn subject_case_one_when() {
        // subject case, 1 when → nb=1 → (1+6)/5 = 1 → 1+1 = 2. (rubocop g1)
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def g1; case var; when 1 then a; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `g1` is too high. [2/1]
            "});
    }

    #[test]
    fn subject_case_eight_whens_plus_else() {
        // subject case, 8 whens + else → nb=9 → (9+6)/5 = 3 → 1+3 = 4.
        // (rubocop g9)
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def g9; case var; when 1 then a; when 2 then b; when 3 then c; when 4 then d; when 5 then e; when 6 then f; when 7 then g; when 8 then h; else z; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `g9` is too high. [4/3]
            "});
    }

    #[test]
    fn no_subject_case_counts_each_when() {
        // no-subject case, 2 whens → nb=2 → +2 → 3 (if/elsif sugar). (rubocop f10)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f10; case; when a then 1; when b then 2; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f10` is too high. [3/2]
            "});
    }

    #[test]
    fn no_subject_case_with_else() {
        // no-subject case, 2 whens + else → nb=3 → +3 → 4. (rubocop f11)
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def f11; case; when a then 1; when b then 2; else 3; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f11` is too high. [4/3]
            "});
    }

    #[test]
    fn case_in_not_counted_as_case() {
        // case/in (case_match) is NOT in COUNTED_NODES; each in_pattern +1.
        // 2 in arms → +2 → 3. (rubocop f12)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def f12; case var; in 1 then a; in 2 then b; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `f12` is too high. [3/2]
            "});
    }

    #[test]
    fn nested_subject_case() {
        // outer case nb=1 (+1), inner case nb=2 (+1) → 1+1+1 = 3.
        // whens score 0; nested cases each apply their own formula. (rubocop nested_case)
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def n; case a; when 1; case b; when 2 then x; when 3 then y; end; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `n` is too high. [3/2]
            "});
    }

    #[test]
    fn doc_example_scores_seven() {
        // RuboCop's own doc example. if (+2 outer-if has else), case (+2),
        // until (+1), && (+1) → 1 + 2 + 2 + 1 + 1 = 7. (rubocop my_method)
        test::<PerceivedComplexity>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                def my_method; if cond; case var; when 1 then func_one; when 2 then func_two; when 3 then func_three; when 4..10 then func_other; end; else; do_something until a && b; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `my_method` is too high. [7/0]
            "});
    }

    #[test]
    fn accepts_at_max() {
        // if/else → score 3; with Max 3 it is not > 3.
        test::<PerceivedComplexity>()
            .expect_no_offenses("def foo; if a; 1; else; 2; end; end\n");
    }

    #[test]
    fn accepts_empty_method() {
        test::<PerceivedComplexity>()
            .with_options(&opts(0))
            .expect_no_offenses("def foo; end\n");
    }

    #[test]
    fn when_not_counted_directly() {
        // A subject case with 1 when scores via the formula ((1+6)/5 = 1), NOT
        // +1 per when. With Max 1 → score 2 fires; this pins that `when` itself
        // adds 0 (otherwise we'd double-count).
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def w; case var; when 1 then a; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `w` is too high. [2/1]
            "});
    }

    #[test]
    fn outer_if_no_own_else_with_nested_if_else() {
        // The OUTER `if` has no `else` of its own; the nested `if` has one.
        // RuboCop: base 1 + outer if (no else → +1) + inner if (else → +2) = 4.
        // `cx.is_else(outer)` must NOT catch the nested if's `else` token.
        // (verified == rubocop 1.87.0: [4/0])
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def m; if a; if b; 1; else; 2; end; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `m` is too high. [4/3]
            "});
    }

    #[test]
    fn outer_unless_no_own_else_with_nested_if_else() {
        // Same as above but the outer scope is `unless` (no own else → +1).
        // RuboCop: 1 + 1 + 2 = 4 (verified == rubocop 1.87.0: [4/0]).
        test::<PerceivedComplexity>()
            .with_options(&opts(3))
            .expect_offense(indoc! {"
                def u; unless a; if b; 1; else; 2; end; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `u` is too high. [4/3]
            "});
    }

    #[test]
    fn iterating_block_still_counts() {
        // Inherited from CyclomaticComplexity: each {} → +1 → 2.
        test::<PerceivedComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; ary.each { |x| x }; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn singleton_def_measured() {
        test::<PerceivedComplexity>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                def self.foo; a if b; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `foo` is too high. [2/0]
            "});
    }

    #[test]
    fn define_method_block_measured() {
        // define_method(:foo) { a if b; c while d } → if +1, while +1 → 3.
        test::<PerceivedComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                define_method(:foo) { a if b; c while d }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Perceived complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn allowed_methods_skips() {
        test::<PerceivedComplexity>()
            .with_options(&PerceivedComplexityOptions {
                max: 0,
                allowed_methods: vec!["foo".to_string()],
                allowed_patterns: Vec::new(),
            })
            .expect_no_offenses("def foo; a if b; end\n");
    }

    #[test]
    fn allowed_patterns_skips() {
        test::<PerceivedComplexity>()
            .with_options(&PerceivedComplexityOptions {
                max: 0,
                allowed_methods: Vec::new(),
                allowed_patterns: vec!["\\Afoo".to_string()],
            })
            .expect_no_offenses("def foo_bar; a if b; end\n");
    }
}

murphy_plugin_api::submit_cop!(PerceivedComplexity);
