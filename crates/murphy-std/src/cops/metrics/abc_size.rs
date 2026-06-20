//! `Metrics/AbcSize` — flag methods whose ABC (Assignment / Branch /
//! Condition) magnitude exceeds `Max`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/AbcSize
//! upstream_version_checked: 1.87.0
//! version_added: "0.27"
//! version_changed: "1.5"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.20.1, murphy-e7bz.20.2, murphy-e7bz.20.3, murphy-e7bz.20.4]
//! notes: >
//!   Mirrors RuboCop's `Metrics::Utils::AbcSizeCalculator` and the
//!   `MethodComplexity` mixin numerically (verified against rubocop 1.87.0
//!   across assignments, branches, conditions, comparison methods, csend
//!   discounts, the iterating-block gate, the else-keyword condition,
//!   `[]=`/setter/index writes, and
//!   `define_method(:name) { ... }`/numbered-param blocks via the
//!   `on_block`/`on_numblock`/`on_itblock` dispatch). The default-config
//!   path (`CountRepeatedAttributes: true`) matches rubocop; the non-default
//!   `CountRepeatedAttributes: false` discount path has two known gaps
//!   (murphy-e7bz.20.3, murphy-e7bz.20.4 — see below).
//!
//!   Known gap (murphy-e7bz.20.1): a multiple assignment whose LHS targets
//!   are *setter* or *index* writes (`self.x, self.y = 1, 2`) is undercounted
//!   because murphy's translate layer emits `Unknown` for those mlhs targets,
//!   so the calculator never sees the setter sends. RuboCop counts them
//!   (`<2, 2, 0>`); murphy currently yields `<0, 0, 0>`. Plain local-variable
//!   masgn targets (`a, b = 1, 2`) are unaffected. The fix belongs in
//!   murphy-translate, not in this cop.
//!
//!   Known gap (murphy-e7bz.20.2): inside a Ruby 3.4 `it`-param block
//!   (`[1].each { it }`), murphy translates the implicit `it` to `(lvar it)`
//!   rather than parser-gem's `(send nil :it)`, so the implicit-parameter
//!   reference is not counted as a branch. RuboCop: `<0, 2, 1>`; murphy:
//!   `<0, 1, 0>`. Numbered-param blocks (`_1`) and regular blocks match.
//!   The fix belongs in murphy-translate.
//!
//!   Known gap (murphy-e7bz.20.3): with `CountRepeatedAttributes: false`,
//!   a shorthand op-assign onto an attribute (`foo.bar ||= x`, `foo.bar +=
//!   x`) does not invalidate the tracked getter chain. RuboCop's
//!   `setter_to_getter` treats `node.shorthand_asgn?` as a setter, so a
//!   later `foo.bar` re-counts; murphy's `update_repeated_attribute` only
//!   handles var-asgn and setter sends, so the later read stays discounted
//!   and the branch count is undercounted (`foo.bar; foo.bar ||= baz;
//!   foo.bar` → rubocop `<2, 4, 1>`). The fix belongs in this cop.
//!
//!   Known gap (murphy-e7bz.20.4): with `CountRepeatedAttributes: false`,
//!   scoped-constant receivers collapse to their terminal name in the
//!   discount key, so `A::B.foo` and `C::B.foo` are treated as the same
//!   attribute and the second is wrongly discounted (rubocop keys by the
//!   const AST node, keeping them distinct: `<0, 2, 0>`). The fix belongs
//!   in this cop (`receiver_chain_key`).
//!
//!   The calculator walks the method body in post-order
//!   (`visit_depth_last`) and accumulates three counters:
//!
//!   - A (assignment): non-underscore local-variable assignments
//!     (`lvasgn` whose name does not start with `_`), every non-lvar
//!     `equals_asgn` write (`ivasgn`/`cvasgn`/`gvasgn`/`casgn`/index/
//!     attribute `=`), setter-method sends (`obj.foo = x`), `for`
//!     loops, and capturing argument nodes (block params not starting
//!     with `_`). Multiple assignment (`masgn`) and shorthand op-assign
//!     (`+=`, `||=`, `&&=`) are counted via `compound_assignment`: each
//!     assignment target that is *not* a setter method contributes one.
//!
//!   - B (branch): `send`, `csend`, and `yield` nodes. A comparison-method
//!     send (`==`, `!=`, `<=`, `>=`, `<`, `>`, `===`) counts as a
//!     *condition* instead, not a branch.
//!
//!   - C (condition): the cyclomatic `COUNTED_NODES`
//!     (`if while until for csend block block_pass rescue when
//!     in_pattern and or or_asgn and_asgn`), gated so that a non-iterating
//!     block (a block whose method is not a known iterating method such
//!     as `each`/`map`) is excluded. A `case`/`if` with a literal `else`
//!     keyword adds one extra condition. A `csend` adds a condition only
//!     on its first occurrence per receiver lvar (RuboCop's
//!     `RepeatedCsendDiscount`, always active).
//!
//!   Magnitude = `sqrt(A^2 + B^2 + C^2)` rounded to two decimals. An
//!   offense fires when the magnitude is strictly greater than `Max`
//!   (default 17). The whole `def`/`defs` node is the offense range
//!   (RuboCop non-LSP `node.source_range`). Message:
//!   "Assignment Branch Condition size for `m` is too high. [<A, B, C> calc/max]"
//!   where `calc`/`max` use Ruby's `%.4g` formatting (trailing `.0`
//!   stripped, e.g. `17` not `17.0`).
//!
//!   Options: `Max` (default 17), `CountRepeatedAttributes` (default true;
//!   when false, repeated no-argument attribute reads on the same receiver
//!   chain are discounted — RuboCop's `RepeatedAttributeDiscount`),
//!   `AllowedMethods` and `AllowedPatterns` (skip matching method names).
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};
use std::collections::{HashMap, HashSet};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct AbcSize;

/// Options for [`AbcSize`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct AbcSizeOptions {
    #[option(
        name = "Max",
        default = 17,
        description = "Maximum allowed ABC magnitude for a method."
    )]
    pub max: i64,
    #[option(
        name = "CountRepeatedAttributes",
        default = true,
        description = "Count repeated attribute accesses; when false they are discounted."
    )]
    pub count_repeated_attributes: bool,
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Method names exempt from the ABC-size check."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns; methods whose name matches are exempt."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Metrics/AbcSize",
    description = "A calculated magnitude based on number of assignments, branches, and conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = AbcSizeOptions,
)]
impl AbcSize {
    /// RuboCop `MethodComplexity#on_def`.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_method(node, cx);
    }

    /// RuboCop `alias on_defs on_def`.
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_method(node, cx);
    }

    /// RuboCop `MethodComplexity#on_block` — `define_method(:name) { ... }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_numblock on_block` — `define_method(:n) { _1 }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_itblock on_block` — `define_method(:n) { it }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }
}

/// RuboCop `MethodComplexity#on_def`/`on_defs` → `check_complexity`.
fn check_method(node: NodeId, cx: &Cx<'_>) {
    let Some(method_name) = cx.method_name(node) else {
        return;
    };
    check_complexity(node, cx.def_body(node), method_name, cx);
}

/// RuboCop `MethodComplexity#on_block`: the `define_method?` matcher
/// `(any_block (send nil? :define_method ({sym str} $_)) _ _)` — a block
/// whose call is a receiverless `define_method` with a symbol/string name.
fn check_define_method_block(node: NodeId, cx: &Cx<'_>) {
    let Some(call) = cx.block_call(node).get() else {
        return;
    };
    if cx.call_receiver(call).get().is_some() || cx.method_name(call) != Some("define_method") {
        return;
    }
    let Some(&name_arg) = cx.call_arguments(call).first() else {
        return;
    };
    let method_name = match cx.kind(name_arg) {
        NodeKind::Sym(s) => cx.symbol_str(*s),
        NodeKind::Str(id) => cx.string_str(*id),
        _ => return,
    };
    check_complexity(node, cx.block_body(node), method_name, cx);
}

/// RuboCop `MethodComplexity#check_complexity`: skip allowed methods, accept
/// empty bodies, compute the ABC magnitude, and emit on the whole node when it
/// exceeds `Max`.
fn check_complexity(
    node: NodeId,
    body: murphy_plugin_api::OptNodeId,
    method_name: &str,
    cx: &Cx<'_>,
) {
    let opts = cx.options_or_default::<AbcSizeOptions>();

    // `allowed_method?(name) || matches_allowed_pattern?(name)`.
    if opts.allowed_methods.iter().any(|m| m == method_name)
        || cx.matches_any_pattern(method_name, &opts.allowed_patterns)
    {
        return;
    }

    // `check_complexity`: accepts empty methods always (`return unless node.body`).
    let Some(body) = body.get() else {
        return;
    };

    let mut calc = AbcCalculator::new(opts.count_repeated_attributes);
    calc.visit_depth_last(body, cx);
    let magnitude = calc.magnitude();

    // `return unless complexity > max`.
    if magnitude <= opts.max as f64 {
        return;
    }

    let vector = format!("<{}, {}, {}>", calc.assignment, calc.branch, calc.condition);
    let message = format!(
        "Assignment Branch Condition size for `{method_name}` is too high. [{vector} {}/{}]",
        format_g(magnitude),
        format_g(opts.max as f64),
    );
    cx.emit_offense(cx.range(node), &message, None);
}

/// RuboCop's `Metrics::Utils::AbcSizeCalculator`, ported with the
/// always-active `RepeatedCsendDiscount` and the optional
/// `RepeatedAttributeDiscount`.
struct AbcCalculator<'a> {
    assignment: u32,
    branch: u32,
    condition: u32,
    /// `RepeatedCsendDiscount#@repeated_csend`: maps an lvar receiver name to
    /// the first `csend` seen on it. Always tracked.
    repeated_csend: HashMap<&'a str, NodeId>,
    /// `RepeatedAttributeDiscount`: only enabled when `CountRepeatedAttributes`
    /// is `false`. Tracks no-argument attribute-call receiver chains.
    discount_repeated_attributes: bool,
    /// Known attribute chains keyed by a canonical receiver-chain string.
    known_attributes: HashSet<String>,
}

impl<'a> AbcCalculator<'a> {
    fn new(count_repeated_attributes: bool) -> Self {
        Self {
            assignment: 0,
            branch: 0,
            condition: 0,
            repeated_csend: HashMap::new(),
            discount_repeated_attributes: !count_repeated_attributes,
            known_attributes: HashSet::new(),
        }
    }

    /// `Math.sqrt(a^2 + b^2 + c^2).round(2)`.
    fn magnitude(&self) -> f64 {
        let a = f64::from(self.assignment);
        let b = f64::from(self.branch);
        let c = f64::from(self.condition);
        let raw = (a * a + b * b + c * c).sqrt();
        (raw * 100.0).round() / 100.0
    }

    /// `visit_depth_last`: recurse into children, then yield the node itself
    /// (post-order). Matches RuboCop's traversal order, which is load-bearing
    /// for the `csend`/attribute discount state machine.
    fn visit_depth_last(&mut self, node: NodeId, cx: &Cx<'a>) {
        for child in cx.children(node) {
            self.visit_depth_last(child, cx);
        }
        self.calculate_node(node, cx);
    }

    /// `calculate_node` + the `RepeatedAttributeDiscount#calculate_node`
    /// prepend (`update_repeated_attribute` before the super call).
    fn calculate_node(&mut self, node: NodeId, cx: &Cx<'a>) {
        if self.discount_repeated_attributes {
            self.update_repeated_attribute(node, cx);
        }

        if self.is_assignment(node, cx) {
            self.assignment += 1;
        }

        if is_branch(node, cx) {
            self.evaluate_branch_nodes(node, cx);
        } else if is_condition(node, cx) {
            self.evaluate_condition_node(node, cx);
        }
    }

    /// `evaluate_branch_nodes` plus the `RepeatedAttributeDiscount` override.
    fn evaluate_branch_nodes(&mut self, node: NodeId, cx: &Cx<'a>) {
        if self.discount_repeated_attributes && self.discount_repeated_attribute(node, cx) {
            return;
        }

        if cx.is_comparison_method(node) {
            self.condition += 1;
        } else {
            self.branch += 1;
            if matches!(cx.kind(node), NodeKind::Csend { .. })
                && !self.discount_for_repeated_csend(node, cx)
            {
                self.condition += 1;
            }
        }
    }

    /// `evaluate_condition_node`.
    fn evaluate_condition_node(&mut self, node: NodeId, cx: &Cx<'a>) {
        if self.is_else_branch(node, cx) {
            self.condition += 1;
        }
        self.condition += 1;
    }

    /// `else_branch?`: `%i[case if].include?(type) && node.else? &&
    /// node.loc.else.is?('else')`. The `loc.else.is?('else')` guard rejects
    /// `elsif` chains (whose `loc.else` is the `elsif` keyword).
    fn is_else_branch(&self, node: NodeId, cx: &Cx<'a>) -> bool {
        match cx.kind(node) {
            NodeKind::If { else_, then_, .. } => {
                let Some(else_id) = else_.get() else {
                    return false;
                };
                // Locate the keyword in the gap between the then-branch end
                // (or condition end) and the else-branch start: it is `else`
                // for a literal else, `elsif` for an elsif chain.
                let then_end = then_
                    .get()
                    .map_or(cx.range(node).start, |t| cx.range(t).end);
                keyword_in_gap_is_else(then_end, cx.range(else_id).start, cx)
            }
            NodeKind::Case { else_, .. } => else_.get().is_some(),
            _ => false,
        }
    }

    /// `assignment?`.
    fn is_assignment(&mut self, node: NodeId, cx: &Cx<'a>) -> bool {
        // `if node.masgn_type? || node.shorthand_asgn? ... return false`.
        match cx.kind(node) {
            NodeKind::Masgn { .. }
            | NodeKind::OpAsgn { .. }
            | NodeKind::OrAsgn { .. }
            | NodeKind::AndAsgn { .. } => {
                self.compound_assignment(node, cx);
                return false;
            }
            _ => {}
        }

        // `node.for_type?`
        if matches!(cx.kind(node), NodeKind::For { .. }) {
            return true;
        }

        // `node.respond_to?(:setter_method?) && node.setter_method?`
        if is_setter_send(node, cx) {
            return true;
        }

        self.simple_assignment(node, cx) || is_capturing_argument(node, cx)
    }

    /// `compound_assignment`: count assignment targets that are *not* setter
    /// methods (those would be miscounted because the setter `=` is not a
    /// separate send under masgn/shorthand).
    fn compound_assignment(&mut self, node: NodeId, cx: &Cx<'a>) {
        let children: Vec<NodeId> = match cx.kind(node) {
            // `masgn`: `node.assignments` — the lhs targets.
            NodeKind::Masgn { lhs, .. } => match cx.kind(*lhs) {
                NodeKind::Mlhs(list) => cx.list(*list).to_vec(),
                _ => vec![*lhs],
            },
            // shorthand op-assign: `node.children` — the write target plus value.
            NodeKind::OpAsgn { target, value, .. } => vec![*target, *value],
            NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
                vec![*target, *value]
            }
            _ => return,
        };

        let miscounted = children
            .iter()
            .filter(|&&child| {
                // `child.respond_to?(:setter_method?) && !child.setter_method?`
                matches!(cx.kind(child), NodeKind::Send { .. } | NodeKind::Csend { .. })
                    && !is_setter_send(child, cx)
            })
            .count() as u32;
        self.assignment += miscounted;
    }

    /// `simple_assignment?`.
    fn simple_assignment(&mut self, node: NodeId, cx: &Cx<'a>) -> bool {
        if !is_equals_asgn(node, cx) {
            return false;
        }
        if let NodeKind::Lvasgn { name, .. } = cx.kind(node) {
            // `reset_on_lvasgn(node)`: invalidate any tracked csend on this var.
            let var = cx.symbol_str(*name);
            self.repeated_csend.remove(var);
            // `capturing_variable?(node.children.first)`
            return is_capturing_name(var);
        }
        true
    }

    /// `discount_for_repeated_csend?`.
    fn discount_for_repeated_csend(&mut self, node: NodeId, cx: &Cx<'a>) -> bool {
        let NodeKind::Csend { receiver, .. } = cx.kind(node) else {
            return false;
        };
        // `return false unless receiver.lvar_type?`
        let NodeKind::Lvar(var_sym) = cx.kind(*receiver) else {
            return false;
        };
        let var_name = cx.symbol_str(*var_sym);
        match self.repeated_csend.get(var_name) {
            // First occurrence: record and report not-repeated.
            None => {
                self.repeated_csend.insert(var_name, node);
                false
            }
            // `!seen.equal?(csend_node)` — repeated iff a *different* node.
            Some(&seen) => seen != node,
        }
    }

    // ── RepeatedAttributeDiscount (only when CountRepeatedAttributes: false) ──

    /// `discount_repeated_attribute?`: a no-argument attribute call that has
    /// already been seen on the same receiver chain.
    fn discount_repeated_attribute(&mut self, node: NodeId, cx: &Cx<'a>) -> bool {
        let Some(chain) = attribute_chain_key(node, cx) else {
            return false;
        };
        // `insert` returns `false` if the chain was already tracked → repeated.
        !self.known_attributes.insert(chain)
    }

    /// `update_repeated_attribute`: a setter (`var = x`, `self.foo = x`,
    /// `var ||= x`) invalidates tracked chains rooted at that target.
    ///
    /// KNOWN GAP (murphy-e7bz.20.3): RuboCop's `setter_to_getter` also treats
    /// `node.shorthand_asgn?` onto an attribute (`foo.bar ||= x`) as a setter
    /// and invalidates the getter chain. This impl does not yet invalidate on
    /// `OpAsgn`/`OrAsgn`/`AndAsgn` attribute targets, so a later identical
    /// read stays discounted (undercounts branch under `CountRepeatedAttributes:
    /// false`). Default config is unaffected.
    fn update_repeated_attribute(&mut self, node: NodeId, cx: &Cx<'a>) {
        match cx.kind(node) {
            // Variable reassignment clears everything rooted at that var.
            NodeKind::Lvasgn { name, .. } => {
                let prefix = format!("lvar:{}", cx.symbol_str(*name));
                self.invalidate_prefix(&prefix);
            }
            NodeKind::Ivasgn { name, .. } => {
                let prefix = format!("ivar:{}", cx.symbol_str(*name));
                self.invalidate_prefix(&prefix);
            }
            NodeKind::Cvasgn { name, .. } => {
                let prefix = format!("cvar:{}", cx.symbol_str(*name));
                self.invalidate_prefix(&prefix);
            }
            NodeKind::Gvasgn { name, .. } => {
                let prefix = format!("gvar:{}", cx.symbol_str(*name));
                self.invalidate_prefix(&prefix);
            }
            // `self.foo = x` / `obj.foo = x`: delete the specific method.
            NodeKind::Send { .. } | NodeKind::Csend { .. } if cx.is_setter_method(node) => {
                if let Some(receiver) = cx.call_receiver(node).get()
                    && let Some(recv_key) = receiver_chain_key(receiver, cx)
                    && let Some(method) = cx.method_name(node)
                {
                    let getter = method.strip_suffix('=').unwrap_or(method);
                    let full = format!("{recv_key}.{getter}");
                    self.invalidate_prefix(&full);
                }
            }
            _ => {}
        }
    }

    /// Remove every tracked chain that is `prefix` or extends it
    /// (`prefix.method...`). RuboCop deletes the exact getter (and its
    /// subtree) or clears the whole receiver subtree on var reassignment.
    fn invalidate_prefix(&mut self, prefix: &str) {
        let nested = format!("{prefix}.");
        self.known_attributes
            .retain(|k| k != prefix && !k.starts_with(&nested));
    }
}

/// RuboCop `node.respond_to?(:setter_method?) && node.setter_method?` for a
/// `send`/`csend`: an attribute write (`obj.foo = x`) or an index write
/// (`obj[k] = x`, selector `[]=`). The loc-based [`Cx::is_setter_method`]
/// covers the `obj.foo = x` shape; the name-based [`Cx::is_assignment_method`]
/// (`!comparison && ends_with('=')`) additionally covers `[]=`, which murphy
/// parses as a plain `send :[]=` whose `=` sits after the index argument.
fn is_setter_send(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && (cx.is_setter_method(node) || cx.is_assignment_method(node))
}

/// `equals_asgn?`: a `lvasgn`/`ivasgn`/`cvasgn`/`gvasgn`/`casgn`/`index_asgn`
/// with a value (not the op-assign target form, which has no value), plus the
/// `casgn`. (`masgn`/op-assign are handled earlier.)
fn is_equals_asgn(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
            | NodeKind::IndexAsgn { .. }
    )
}

/// `branch?`: `BRANCH_NODES = %i[send csend yield]`.
fn is_branch(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. } | NodeKind::Yield(_)
    )
}

/// `condition?`: `return false if iterating_block?(node) == false;
/// CONDITION_NODES.include?(node.type)`.
///
/// `CONDITION_NODES` (= `CyclomaticComplexity::COUNTED_NODES`) contains
/// `:block` and `:block_pass` but **not** `:numblock`/`:itblock`, so numbered-
/// and `it`-parameter blocks are never conditions. The `iterating_block?` gate
/// only fires for `:block`/`:block_pass` (the only types RuboCop's
/// `block_method_name` answers for) and returns `false` exactly when such a
/// block's method is not a known iterating method.
fn is_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    // The gate returns early only when `iterating_block?(node) == false`, i.e.
    // a `:block`/`:block_pass` whose method name is *not* iterating. For other
    // node types `iterating_block?` is `nil` and the COUNTED_NODES check runs.
    if is_gated_block(node, cx) && !is_iterating_block(node, cx) {
        return false;
    }
    matches!(
        cx.kind(node),
        NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::For { .. }
            | NodeKind::Csend { .. }
            | NodeKind::Block { .. }
            | NodeKind::BlockPass(_)
            | NodeKind::Rescue { .. }
            | NodeKind::When { .. }
            | NodeKind::InPattern { .. }
            | NodeKind::And { .. }
            | NodeKind::Or { .. }
            | NodeKind::OrAsgn { .. }
            | NodeKind::AndAsgn { .. }
    )
}

/// The node kinds RuboCop's `block_method_name` answers for — `:block` and
/// `:block_pass`. These are the only kinds where `iterating_block?` can return
/// the literal `false` that short-circuits `condition?`.
fn is_gated_block(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Block { .. } | NodeKind::BlockPass(_))
}

/// `iterating_block?`: the block's method name is a known iterating method.
/// For a `block_pass` (`&:sym`), RuboCop reads `node.parent.method_name`.
fn is_iterating_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let name = match cx.kind(node) {
        NodeKind::Block { .. } => cx.method_name(node),
        NodeKind::BlockPass(_) => cx.parent(node).get().and_then(|p| cx.method_name(p)),
        _ => None,
    };
    name.is_some_and(is_iterating_method)
}

/// `IteratingBlock::KNOWN_ITERATING_METHODS` — verbatim from rubocop 1.87.0
/// (`enumerable + enumerator + array + hash`, deduplicated).
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
            | "d_permutation" | "delete_if" | "each_index" | "keep_if" | "map!"
            | "permutation" | "product" | "reject!" | "repeat"
            | "repeated_combination" | "select!" | "sort!"
            // hash
            | "each_key" | "each_pair" | "each_value" | "fetch" | "fetch_values"
            | "has_key?" | "merge" | "merge!" | "transform_keys"
            | "transform_keys!" | "transform_values" | "transform_values!"
    )
}

/// `capturing_variable?(name)`: `name && !name.start_with?('_')`.
fn is_capturing_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('_')
}

/// `argument?`: `node.argument_type? && capturing_variable?(children.first)`.
/// Argument node kinds whose first child is the parameter name.
fn is_capturing_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    let name = match cx.kind(node) {
        NodeKind::Arg(sym)
        | NodeKind::Restarg(sym)
        | NodeKind::Kwarg(sym)
        | NodeKind::Kwrestarg(sym)
        | NodeKind::Blockarg(sym) => Some(cx.symbol_str(*sym)),
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => {
            Some(cx.symbol_str(*name))
        }
        _ => None,
    };
    name.is_some_and(is_capturing_name)
}

/// Canonical key for a no-argument attribute call (`call _receiver _method`
/// with no parameters), used by the repeated-attribute discount. `None` if the
/// node is not such a call.
fn attribute_chain_key(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let (receiver, method) = match cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            if !cx.list(*args).is_empty() {
                return None;
            }
            (receiver.get(), cx.symbol_str(*method))
        }
        NodeKind::Csend { receiver, method, args } => {
            if !cx.list(*args).is_empty() {
                return None;
            }
            (Some(*receiver), cx.symbol_str(*method))
        }
        _ => return None,
    };
    let recv_key = match receiver {
        Some(r) => receiver_chain_key(r, cx)?,
        // Receiverless call: `(send nil :foo)` shares the `self` namespace.
        None => "self".to_string(),
    };
    Some(format!("{recv_key}.{method}"))
}

/// Canonical key for a receiver in an attribute chain: a root node (nil/self/
/// lvar/ivar/cvar/gvar/const) or a nested no-argument attribute call.
fn receiver_chain_key(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    match cx.kind(node) {
        NodeKind::SelfExpr => Some("self".to_string()),
        NodeKind::Lvar(s) => Some(format!("lvar:{}", cx.symbol_str(*s))),
        NodeKind::Ivar(s) => Some(format!("ivar:{}", cx.symbol_str(*s))),
        NodeKind::Cvar(s) => Some(format!("cvar:{}", cx.symbol_str(*s))),
        NodeKind::Gvar(s) => Some(format!("gvar:{}", cx.symbol_str(*s))),
        // KNOWN GAP (murphy-e7bz.20.4): RuboCop keys const receivers by the
        // const AST node, keeping scoped constants distinct (`A::B` != `C::B`).
        // Using only the terminal name collapses them, so the second is wrongly
        // discounted under `CountRepeatedAttributes: false`. Default unaffected.
        NodeKind::Const { name, .. } => Some(format!("const:{}", cx.symbol_str(*name))),
        NodeKind::Send { .. } | NodeKind::Csend { .. } => attribute_chain_key(node, cx),
        _ => None,
    }
}

/// `node.loc.else.is?('else')` for an `if` node: is the keyword in the gap a
/// literal `else` (not `elsif`)?
fn keyword_in_gap_is_else(from: u32, to: u32, cx: &Cx<'_>) -> bool {
    use murphy_plugin_api::SourceTokenKind;
    if from >= to {
        return false;
    }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other {
            let text = cx.raw_source(tok.range);
            if text == "else" {
                return true;
            }
            if text == "elsif" {
                return false;
            }
        }
    }
    false
}

/// Ruby's `"%.4g"` for a non-negative ABC magnitude / `Max`: 4 significant
/// figures, trailing zeros and a bare `.` stripped (`17.0` → `"17"`,
/// `4.36` → `"4.36"`, `1234.5` → `"1234"`).
///
/// `%g` switches to scientific notation only when the decimal exponent `e`
/// satisfies `e < -4 || e >= precision`. A non-zero ABC magnitude is always
/// `>= 1.0` (so `e >= 0`) and `Max` is a small non-negative threshold, so the
/// scientific branch is unreachable here — we only implement the fixed branch.
///
/// Tie-rounding micro-divergence: ABC magnitudes are always `sqrt(...).round(2)`
/// (≤ 2 decimals). For magnitudes `< 100` such a value already fits in ≤ 4
/// significant figures, so no rounding occurs and this is byte-identical to
/// Ruby's `%.4g`. Only magnitudes `>= 100.00` with a non-zero hundredths digit
/// hit the 4th-sig-fig rounding point, where glibc applies round-half-to-even
/// (`100.25` → `100.2`) while Rust rounds the exact binary value (`100.25` →
/// `100.3`). Such magnitudes do not occur in real Ruby methods, so this is an
/// accepted, documented edge.
fn format_g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    // Decimal exponent of the most-significant digit.
    let exponent = value.abs().log10().floor() as i32;
    // `%.4g` keeps 4 significant figures → `4 - 1 - exponent` fractional digits
    // (clamped at 0); for `exponent >= 4` this is 0 and the number is rounded
    // to an integer (no scientific notation needed for our value range).
    let frac_digits = (3 - exponent).max(0) as usize;
    let s = format!("{value:.frac_digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

murphy_plugin_api::submit_cop!(AbcSize);

#[cfg(test)]
mod tests {
    use super::{AbcSize, AbcSizeOptions, format_g};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn format_g_matches_ruby_percent_4g() {
        // Verified against `ruby -e 'printf("%.4g", x)'` (ruby 3.3.5). These
        // cover the entire realistic ABC range (magnitudes are `round(2)`,
        // and real-world magnitudes stay well under 100, where `%.4g` never
        // rounds because a ≤2-decimal value < 100 already fits in 4 sig figs).
        assert_eq!(format_g(0.0), "0");
        assert_eq!(format_g(4.0), "4");
        assert_eq!(format_g(17.0), "17");
        assert_eq!(format_g(2.449), "2.449");
        assert_eq!(format_g(2.45), "2.45");
        assert_eq!(format_g(18.03), "18.03");
        assert_eq!(format_g(99.99), "99.99");
        assert_eq!(format_g(1234.5), "1234");
    }

    // ABC vectors and magnitudes below are verified verbatim against
    // rubocop 1.87.0 (Metrics/AbcSize, Max: 0 to surface every method).

    #[test]
    fn assignment_branch_condition_basics() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def m1; x = 1; y = foo; x == y; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m1` is too high. [<2, 1, 1> 2.45/0]
            "});
    }

    #[test]
    fn if_else_branch_counts_else_keyword() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def m2; a = compute; if a > 0 then bar(a) else baz end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m2` is too high. [<1, 3, 3> 4.36/0]
            "});
    }

    #[test]
    fn csend_adds_branch_and_condition() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def m3; obj&.foo; obj&.bar; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m3` is too high. [<0, 4, 2> 4.47/0]
            "});
    }

    #[test]
    fn iterating_block_counts_condition() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def m4; total = 0; [1, 2, 3].each { |n| total += n }; total; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m4` is too high. [<3, 1, 1> 3.32/0]
            "});
    }

    #[test]
    fn setter_and_ivasgn_and_masgn() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def m5; self.value = 10; @count = 1; data, rest = split; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m5` is too high. [<4, 2, 0> 4.47/0]
            "});
    }

    #[test]
    fn define_method_symbol_name_dispatched() {
        // rubocop 1.87.0: define_method block is checked via on_block.
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                define_method(:foo) { x = bar }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `foo` is too high. [<1, 1, 0> 1.41/0]
            "});
    }

    #[test]
    fn define_method_string_name_dispatched() {
        // rubocop 1.87.0: a string method name also dispatches.
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                define_method(\"sname\") { y = compute; z = process }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `sname` is too high. [<2, 2, 0> 2.83/0]
            "});
    }

    #[test]
    fn numbered_param_block_not_counted_as_condition() {
        // rubocop 1.87.0: a numblock is NOT in COUNTED_NODES → no extra condition.
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def nb; [1].each { _1 + bar }; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `nb` is too high. [<0, 3, 0> 3/0]
            "});
    }

    #[test]
    fn index_and_attribute_writes_count_as_assignments() {
        // rubocop 1.87.0: `<2, 5, 0> 5.39` — `[]=` and `obj.attr =` are both
        // setter sends (A), `@data`/`obj`/`compute`/`val` are branches (B).
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def ix; @data[:k] = compute; obj.attr = val; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `ix` is too high. [<2, 5, 0> 5.39/0]
            "});
    }

    #[test]
    fn case_when_and_else() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d3; case x when 1 then a when 2 then b end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d3` is too high. [<0, 3, 2> 3.61/0]
            "});
    }

    #[test]
    fn for_loop_counts_assignment_and_condition() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d4; for i in 1..10 do puts i end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d4` is too high. [<2, 1, 1> 2.45/0]
            "});
    }

    #[test]
    fn shorthand_op_assign_counts() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d5; x = 1 while cond; y &&= 2; z ||= 3; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d5` is too high. [<3, 1, 3> 4.36/0]
            "});
    }

    #[test]
    fn ternary_counts_condition() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d6; result = a ? b : c; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d6` is too high. [<1, 3, 1> 3.32/0]
            "});
    }

    #[test]
    fn block_args_capturing_underscore() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def a1; [1].each { |x, _y, z| x }; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `a1` is too high. [<2, 1, 1> 2.45/0]
            "});
    }

    #[test]
    fn underscore_lvasgn_not_counted() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def u1; _ = foo; _bar = baz; real = qux; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `u1` is too high. [<1, 3, 0> 3.16/0]
            "});
    }

    #[test]
    fn repeated_csend_on_lvar_discounts_condition() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def c2; v = obj; v&.foo; v&.bar; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `c2` is too high. [<1, 3, 1> 3.32/0]
            "});
    }

    #[test]
    fn repeated_csend_reset_on_reassignment() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def c3; v = obj; v&.foo; v = other; v&.bar; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `c3` is too high. [<2, 4, 2> 4.9/0]
            "});
    }

    #[test]
    fn count_repeated_attributes_true_no_discount() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d1; foo.bar; foo.bar; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d1` is too high. [<0, 4, 0> 4/0]
            "});
    }

    #[test]
    fn count_repeated_attributes_false_discounts() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: false,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d1; foo.bar; foo.bar; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d1` is too high. [<0, 2, 0> 2/0]
            "});
    }

    #[test]
    fn distinct_attributes_not_discounted_when_false() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: false,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def d2; foo.bar; foo.baz; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `d2` is too high. [<0, 3, 0> 3/0]
            "});
    }

    #[test]
    fn singleton_def_checked() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec![],
            })
            .expect_offense(indoc! {"
                def self.m1; x = 1; y = foo; x == y; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Assignment Branch Condition size for `m1` is too high. [<2, 1, 1> 2.45/0]
            "});
    }

    #[test]
    fn empty_method_accepted() {
        test::<AbcSize>().with_options(&max0()).expect_no_offenses("def foo; end\n");
    }

    #[test]
    fn under_max_accepted() {
        // Default Max: 17 — a tiny method is well under.
        test::<AbcSize>().expect_no_offenses("def foo; x = 1; bar(x); end\n");
    }

    #[test]
    fn allowed_method_skipped() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec!["m1".to_string()],
                allowed_patterns: vec![],
            })
            .expect_no_offenses("def m1; x = 1; y = foo; x == y; end\n");
    }

    #[test]
    fn allowed_pattern_skipped() {
        test::<AbcSize>()
            .with_options(&AbcSizeOptions {
                max: 0,
                count_repeated_attributes: true,
                allowed_methods: vec![],
                allowed_patterns: vec!["\\Am".to_string()],
            })
            .expect_no_offenses("def m1; x = 1; y = foo; x == y; end\n");
    }

    fn max0() -> AbcSizeOptions {
        AbcSizeOptions {
            max: 0,
            count_repeated_attributes: true,
            allowed_methods: vec![],
            allowed_patterns: vec![],
        }
    }
}
