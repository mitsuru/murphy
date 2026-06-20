//! `Metrics/CyclomaticComplexity` — flag methods/`define_method` blocks whose
//! cyclomatic complexity (number of linearly independent paths through the
//! method) exceeds `Max`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/CyclomaticComplexity
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "0.81"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `MethodComplexity` mixin + `CyclomaticComplexity`
//!   `COUNTED_NODES`/`complexity_score_for`, verified numerically against
//!   standalone rubocop 1.87.0 (`--only Metrics/CyclomaticComplexity`).
//!
//!   Measured scopes (RuboCop `on_def`/`on_defs`/`on_block`):
//!   - every `def`/`defs` whose body is non-empty;
//!   - `define_method(:name) { ... }` blocks (RuboCop's `define_method?`
//!     node-matcher: receiverless `define_method` with a single symbol/string
//!     argument) — including numblock (`{ _1 }`) and itblock (`{ it }`) forms,
//!     mirroring RuboCop's `alias on_numblock on_block`/`alias on_itblock
//!     on_block`. Plain non-`define_method` blocks (`foo.each { }`) are NOT
//!     measured as their own scope.
//!   Empty bodies are always accepted (`return unless node.body`).
//!
//!   Score: starts at 1, then `body.each_node(:lvasgn, *COUNTED_NODES)` in
//!   pre-order DFS (the body node itself counts first, then descendants).
//!   COUNTED_NODES = `if while until for csend block block_pass rescue when
//!   in_pattern and or or_asgn and_asgn`. In murphy's AST: `unless`, ternary
//!   `?:` and modifier-`if` all fold into `If`; `&&`/`and` → `And`; `||`/`or`
//!   → `Or`; a multi-clause `begin/rescue` is one `Rescue` wrapper (counts +1,
//!   not per-clause); `case/in` arms are `InPattern`.
//!
//!   `complexity_score_for` exceptions (everything else scores +1):
//!   - `block` (and only `block` — NOT numblock/itblock) scores +1 only when
//!     its method name is in `KNOWN_ITERATING_METHODS` (e.g. `each`, `map`),
//!     else 0;
//!   - `block_pass` (`&:sym`) scores +1 only when its enclosing call's method
//!     name is iterating, else 0;
//!   - `csend` (`&.`) scores +1 unless it is a repeated safe-navigation on the
//!     same local variable since the last assignment to it
//!     (`discount_for_repeated_csend?`): the first `x&.…` on an lvar `x` counts,
//!     subsequent ones are discounted to 0 until an `lvasgn` of `x` resets the
//!     tracking. `lvasgn` itself never adds to the score — it only resets the
//!     repeated-csend state for that variable.
//!
//!   Fires when score > Max (default 7). Message:
//!   "Cyclomatic complexity for `name` is too high. [score/Max]".
//!   Offense range is the whole measured node (RuboCop's non-LSP
//!   `node.source_range`). `AllowedMethods`/`AllowedPatterns` skip by name.
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 7) — score 9
//! def foo
//!   a && b && c && d && e && f && g && h && i
//! end
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};
use std::collections::HashMap;

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct CyclomaticComplexity;

/// Options for [`CyclomaticComplexity`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct CyclomaticComplexityOptions {
    #[option(
        name = "Max",
        default = 7,
        description = "Maximum allowed cyclomatic complexity for a method."
    )]
    pub max: i64,
    #[option(
        name = "AllowedMethods",
        description = "Methods to ignore when measuring cyclomatic complexity."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        description = "Method-name patterns to ignore when measuring cyclomatic complexity."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Metrics/CyclomaticComplexity",
    description = "A complexity metric that is strongly correlated to the number of test cases needed to validate a method.",
    default_severity = "warning",
    default_enabled = true,
    options = CyclomaticComplexityOptions,
)]
impl CyclomaticComplexity {
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
/// `define_method?` node-matcher. Shared by `block`/`numblock`/`itblock` (the
/// `any_block` alias group). `cx.block_call`/`cx.block_body` resolve the wrapped
/// send and body uniformly across all three block kinds.
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
/// offense when it exceeds `Max`.
fn check_complexity(node: NodeId, method_name: &str, body: Option<NodeId>, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<CyclomaticComplexityOptions>();

    // RuboCop checks AllowedMethods/AllowedPatterns in `on_def`/`on_block`
    // before measuring.
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
        "Cyclomatic complexity for `{method_name}` is too high. [{score}/{}]",
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

/// RuboCop `complexity_score_for`: each counted node scores 1, with three
/// exceptions (blocks/block_pass only when iterating; csend discounted when
/// repeated). Nodes outside `COUNTED_NODES` score 0.
fn complexity_score_for<'a>(
    node: NodeId,
    repeated_csend: &mut HashMap<&'a str, NodeId>,
    cx: &Cx<'a>,
) -> i64 {
    match cx.kind(node) {
        // COUNTED_NODES that always score 1.
        NodeKind::If { .. }
        | NodeKind::While { .. }
        | NodeKind::Until { .. }
        | NodeKind::For { .. }
        | NodeKind::Rescue { .. }
        | NodeKind::When { .. }
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
    use super::{CyclomaticComplexity, CyclomaticComplexityOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(max: i64) -> CyclomaticComplexityOptions {
        CyclomaticComplexityOptions {
            max,
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    #[test]
    fn flags_and_chain_over_max() {
        // 8 `&&` operators → score 9 > 7. Whole-def caret on one line.
        test::<CyclomaticComplexity>().expect_offense(indoc! {"
            def foo; a && b && c && d && e && f && g && h && i; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [9/7]
        "});
    }

    #[test]
    fn accepts_at_max() {
        // 6 `&&` → score 7, not > 7.
        test::<CyclomaticComplexity>().expect_no_offenses("def foo; a && b && c && d && e && f && g; end\n");
    }

    #[test]
    fn accepts_empty_method() {
        test::<CyclomaticComplexity>()
            .with_options(&opts(0))
            .expect_no_offenses("def foo; end\n");
    }

    #[test]
    fn doc_example_scores_six() {
        // RuboCop's own doc-comment example, compressed to one line to keep the
        // whole-def offense range on a single caret row. Decision points:
        // `unless` (+1), `each {}` iterating block (+1), inner `unless` (+1),
        // `if` (+1), `||` (+1) → base 1 + 5 = 6. Verified == rubocop 1.87.0.
        test::<CyclomaticComplexity>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                def each_child_node(*types); return to_enum(__method__, *types) unless block_given?; children.each { |child| next unless child.is_a?(Node); yield child if types.empty? || types.include?(child.type) }; self; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `each_child_node` is too high. [6/0]
            "});
    }

    #[test]
    fn ternary_counts_as_if() {
        // ternary `?:` → If node (+1) → score 2.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; a ? b : c; end
                ^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn modifier_unless_counts_as_if() {
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; bar unless baz; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn two_clause_rescue_counts_once() {
        // begin/rescue with two clauses → single Rescue node (+1) → score 2.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; begin; a; rescue A; b; rescue B; c; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn case_when_counts_per_clause() {
        // 2 when clauses → +2 → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def foo; case x; when 1 then a; when 2 then b; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn case_in_counts_per_pattern() {
        // 2 `in` patterns → +2 → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def foo; case x; in 1 then a; in 2 then b; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn or_and_asgn_count() {
        // a ||= 1 (+1), b &&= 2 (+1) → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def foo; a ||= 1; b &&= 2; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn iterating_block_counts() {
        // each {} → +1 → score 2.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; ary.each { |x| x }; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn non_iterating_block_not_counted() {
        // tap {} → +0 → score 1, not > 1.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_no_offenses("def foo; ary.tap { |x| x }; end\n");
    }

    #[test]
    fn numblock_not_counted() {
        // numblock `each { _1 }` → NOT in COUNTED_NODES → score 1, not > 1.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_no_offenses("def foo; ary.each { _1 }; end\n");
    }

    #[test]
    fn iterating_block_pass_counts() {
        // map(&:to_s) → +1 → score 2.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; ary.map(&:to_s); end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn non_iterating_block_pass_not_counted() {
        // tap(&:to_s) → +0 → score 1, not > 1.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_no_offenses("def foo; ary.tap(&:to_s); end\n");
    }

    #[test]
    fn repeated_csend_discounted() {
        // x&.a (+1), x&.b (discounted), x&.c (discounted) → score 2.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                def foo; x = 1; x&.a; x&.b; x&.c; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn csend_reset_by_reassignment() {
        // x&.a (+1), x&.b (discounted), x = 2 (reset), x&.c (+1) → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def foo; x = 1; x&.a; x&.b; x = 2; x&.c; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn for_and_until_count() {
        // for (+1), until (+1) → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def foo; for i in 1..10; until x; y; end; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn singleton_def_measured() {
        test::<CyclomaticComplexity>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                def self.foo; a if b; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/0]
            "});
    }

    #[test]
    fn define_method_block_measured() {
        // define_method(:foo) { a if b; c while d } → score 3.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                define_method(:foo) { a if b; c while d }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [3/2]
            "});
    }

    #[test]
    fn define_method_numblock_measured() {
        // numblock `define_method(:foo) { _1 if _1 }` → if (+1) → score 2.
        // Mirrors RuboCop's `alias on_numblock on_block`.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                define_method(:foo) { _1 if _1 }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn define_method_itblock_measured() {
        // itblock `define_method(:foo) { it if it }` → if (+1) → score 2.
        // Mirrors RuboCop's `alias on_itblock on_block`.
        test::<CyclomaticComplexity>()
            .with_options(&opts(1))
            .expect_offense(indoc! {"
                define_method(:foo) { it if it }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `foo` is too high. [2/1]
            "});
    }

    #[test]
    fn plain_block_not_measured_as_scope() {
        // A plain `each` block is never measured as its own scope.
        test::<CyclomaticComplexity>()
            .with_options(&opts(0))
            .expect_no_offenses(indoc! {"
                ary.each do |x|
                  a if b
                  c if d
                end
            "});
    }

    #[test]
    fn nested_def_counts_into_outer() {
        // outer: a if b (+1) + inner's c if d (+1) → outer score 3 > 2.
        // inner's own score is 2 (base 1 + `c if d`), not > 2, so only outer
        // fires — a single offense.
        test::<CyclomaticComplexity>()
            .with_options(&opts(2))
            .expect_offense(indoc! {"
                def outer; a if b; def inner; c if d; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Cyclomatic complexity for `outer` is too high. [3/2]
            "});
    }

    #[test]
    fn allowed_methods_skips() {
        test::<CyclomaticComplexity>()
            .with_options(&CyclomaticComplexityOptions {
                max: 0,
                allowed_methods: vec!["foo".to_string()],
                allowed_patterns: Vec::new(),
            })
            .expect_no_offenses("def foo; a if b; end\n");
    }

    #[test]
    fn allowed_patterns_skips() {
        test::<CyclomaticComplexity>()
            .with_options(&CyclomaticComplexityOptions {
                max: 0,
                allowed_methods: Vec::new(),
                allowed_patterns: vec!["\\Afoo".to_string()],
            })
            .expect_no_offenses("def foo_bar; a if b; end\n");
    }
}

murphy_plugin_api::submit_cop!(CyclomaticComplexity);
