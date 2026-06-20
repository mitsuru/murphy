//! `Naming/PredicateMethod` — predicate methods (returning a boolean) should
//! end with `?`, and non-predicate methods should not.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/PredicateMethod
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-e7bz.69]
//! notes: >
//!   Faithful port of RuboCop's `on_def` (aliased to `on_defs`):
//!
//!     return if allowed?(node)
//!     return_values = return_values(node.body)
//!     return if acceptable?(return_values)
//!     if node.predicate_method? && potential_non_predicate?(return_values)
//!       add_offense(node.loc.name, MSG_NON_PREDICATE)
//!     elsif !node.predicate_method? && all_return_values_boolean?(return_values)
//!       add_offense(node.loc.name, MSG_PREDICATE)
//!
//!   `allowed?` mirrors RuboCop exactly: `initialize` | AllowedMethods |
//!   AllowedPatterns | (AllowBangMethods && bang_method?) | operator_method? |
//!   body.nil?. AllowedMethods defaults to `["call"]`, AllowBangMethods to
//!   `false` (verified: `def call` with `x == y` → no offense; `def save!`
//!   with `true` → MSG_PREDICATE by default, no offense with
//!   AllowBangMethods: true).
//!
//!   Return-value collection (`return_values`) reproduces the RuboCop set:
//!     * the body's own value (or `[]` for a `begin`/multi-statement body),
//!     * every descendant explicit `return`'s extracted value
//!       (bare `return` → synthetic `nil`; `return a` → a; `return a, b` →
//!       Murphy wraps the args in an `array` node, matching RuboCop's
//!       `s(:array)`),
//!     * the body's last value (last child of a begin, or the body itself).
//!   Then `process_return_values` recursively expands conditionals (if /
//!   while / until / case) into branch last-values and `and`/`or` into their
//!   clauses. Missing `else`/`when`-less branches and bodyless loops act as
//!   implicit synthetic `nil` (verified column-for-column against rubocop
//!   1.87.0).
//!
//!   Classification mirrors rubocop-ast predicates:
//!     * `boolean_type?` — node is `true`/`false`;
//!     * `literal?` — rubocop-ast LITERALS (int, float, str, dstr, sym, dsym,
//!       array, hash, regexp, range, rational, complex, true, false, nil);
//!       MSG_NON_PREDICATE fires on `literal? && !boolean_type?` (so `nil`,
//!       `5`, `"x"`, arrays … all qualify — verified `def returns_nil?; nil`
//!       → MSG_NON_PREDICATE);
//!     * `method_returning_boolean?` — a call that is a comparison method
//!       (`==`, `<`, …), a predicate method (ends `?`), or a negation method
//!       (`!`), MINUS WaywardPredicates (default `[infinite?, nonzero?]`).
//!       WaywardPredicates is load-bearing: `def x; num.nonzero?` must NOT be
//!       treated as boolean (verified → no offense).
//!
//!   Mode (default `conservative`): in conservative mode `acceptable?` skips
//!   the def when any return value is `super`/`zsuper` or an unknown
//!   (non-boolean) method call, and `potential_non_predicate?` is suppressed
//!   when any return value is boolean. `aggressive` removes both escapes
//!   (verified: `def with_return?; return unless bar?; true` and
//!   `def cond_mixed?; if x; true; end` → no offense conservative,
//!   MSG_NON_PREDICATE aggressive).
//!
//!   Offense range mirrors `node.loc.name`: the bare method-name token,
//!   INCLUDING the trailing `?`/`!`. Murphy leaves `loc.name == ZERO` on
//!   `def`/`defs`, so the name is located by source search past any singleton
//!   receiver (shared with `Naming/MethodName`). Verified `def foo` col 5..7,
//!   `def foo?` col 5..8, `def calls_pred` col 5..14, `def save!` col 5..9.
//!
//!   Known gaps vs RuboCop (gap issue murphy-e7bz.69; all are sound — they
//!   under-report, never false-positive, because an unrecognised value is
//!   classified non-boolean):
//!     * `case`/`in` pattern matching (`case_match`) branches are not
//!       expanded — `conditional?` covers it in RuboCop but Murphy treats the
//!       whole `case_match` node as a single opaque (non-boolean) value;
//!     * `rescue`/`ensure` (`kwbegin`) bodies are treated as their begin's
//!       last value only, not per-branch like RuboCop's wider analysis;
//!     * explicit `return` nodes nested inside a block/lambda literal inside
//!       the method are still collected (matching RuboCop's `each_descendant`,
//!       which does not stop at block boundaries) — this is intentional parity.
//! ```
//!
//! ## Offense range
//!
//! `node.loc.name`: the bare method name including a trailing `?`/`!`,
//! excluding a singleton receiver (`def self.foo?` → `foo?`).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop, method_predicates};

const MSG_PREDICATE: &str = "Predicate method names should end with `?`.";
const MSG_NON_PREDICATE: &str = "Non-predicate method names should not end with `?`.";

#[derive(Default)]
pub struct PredicateMethod;

/// `Mode` option — RuboCop's `conservative` (default) vs `aggressive`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Allow a `?` name as long as at least one return value is boolean; skip
    /// when any return is `super`/`zsuper` or an unknown method call.
    #[default]
    #[option(value = "conservative")]
    Conservative,
    /// Register an offense for predicate methods that may return a non-boolean.
    #[option(value = "aggressive")]
    Aggressive,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "Mode",
        default = "conservative",
        description = "`conservative` (default) tolerates non-boolean returns on `?` methods; `aggressive` does not."
    )]
    pub mode: Mode,
    #[option(
        name = "AllowedMethods",
        default = ["call"],
        description = "Exact method names that are always allowed."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a method whose name matches any is always allowed."
    )]
    pub allowed_patterns: Vec<String>,
    #[option(
        name = "AllowBangMethods",
        default = false,
        description = "When true, methods ending with `!` are always allowed."
    )]
    pub allow_bang_methods: bool,
    #[option(
        name = "WaywardPredicates",
        default = ["infinite?", "nonzero?"],
        description = "Methods that end in `?` but are known not to return a boolean."
    )]
    pub wayward_predicates: Vec<String>,
}

/// A return value in the analysis. Mirrors RuboCop's mix of real AST nodes and
/// synthetic `s(:nil)` / `s(:array)` sexps produced by `extract_return_value`
/// and missing-branch handling.
#[derive(Clone, Copy)]
enum Value {
    /// A real AST node.
    Node(NodeId),
    /// Synthetic `s(:nil)` — bare `return`, missing `else`, bodyless loop.
    Nil,
}

#[cop(
    name = "Naming/PredicateMethod",
    description = "Predicate method names should end with `?`.",
    default_severity = "warning",
    default_enabled = false,
    options = Options
)]
impl PredicateMethod {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root; chain it so a lone top-level `def`
        // (whose root *is* the def) is also inspected.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            let (name, body) = match *cx.kind(id) {
                NodeKind::Def { name, body, .. } | NodeKind::Defs { name, body, .. } => {
                    (cx.symbol_str(name), body.get())
                }
                _ => continue,
            };

            // `allowed?` — note `body.nil?` is part of it.
            if allowed(name, body, &opts, cx) {
                continue;
            }
            let Some(body) = body else { continue };

            let return_values = return_values(body, cx);

            // `acceptable?` — conservative-only early skip.
            if acceptable(&return_values, &opts, cx) {
                continue;
            }

            let is_predicate = method_predicates::is_predicate_method(name);
            if is_predicate && potential_non_predicate(&return_values, &opts, cx) {
                cx.emit_offense(def_name_range(id, name, cx), MSG_NON_PREDICATE, None);
            } else if !is_predicate && all_return_values_boolean(&return_values, &opts, cx) {
                cx.emit_offense(def_name_range(id, name, cx), MSG_PREDICATE, None);
            }
        }
    }
}

/// RuboCop's `allowed?`. `initialize` | AllowedMethods | AllowedPatterns |
/// (AllowBangMethods && bang) | operator_method? | body.nil?.
fn allowed(name: &str, body: Option<NodeId>, opts: &Options, cx: &Cx<'_>) -> bool {
    name == "initialize"
        || opts.allowed_methods.iter().any(|m| m == name)
        || cx.matches_any_pattern(name, &opts.allowed_patterns)
        || (opts.allow_bang_methods && method_predicates::is_bang_method(name))
        || method_predicates::is_operator_method(name)
        || body.is_none()
}

/// RuboCop's `acceptable?`: conservative-only. Skip when any return value is
/// `super`/`zsuper` or an unknown (non-boolean) method call.
fn acceptable(values: &[Value], opts: &Options, cx: &Cx<'_>) -> bool {
    if opts.mode != Mode::Conservative {
        return false;
    }
    values
        .iter()
        .any(|&v| is_super(v, cx) || unknown_method_call(v, opts, cx))
}

/// RuboCop's `unknown_method_call?`: a call that does not return boolean.
fn unknown_method_call(value: Value, opts: &Options, cx: &Cx<'_>) -> bool {
    let Value::Node(node) = value else {
        return false;
    };
    if !is_call(node, cx) {
        return false;
    }
    !method_returning_boolean(node, opts, cx)
}

/// RuboCop's `all_return_values_boolean?`: reject super/zsuper; non-empty; all
/// boolean.
fn all_return_values_boolean(values: &[Value], opts: &Options, cx: &Cx<'_>) -> bool {
    let mut any = false;
    for &v in values {
        if is_super(v, cx) {
            continue;
        }
        any = true;
        if !boolean_return(v, opts, cx) {
            return false;
        }
    }
    any
}

/// RuboCop's `boolean_return?`: `boolean_type? || method_returning_boolean?`.
fn boolean_return(value: Value, opts: &Options, cx: &Cx<'_>) -> bool {
    match value {
        Value::Nil => false,
        Value::Node(node) => is_boolean_literal(node, cx) || method_returning_boolean(node, opts, cx),
    }
}

/// RuboCop's `method_returning_boolean?`: a call that is a comparison /
/// predicate / negation method, minus WaywardPredicates.
fn method_returning_boolean(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if !is_call(node, cx) {
        return false;
    }
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    if opts.wayward_predicates.iter().any(|w| w == method) {
        return false;
    }
    method_predicates::is_comparison_method(method)
        || method_predicates::is_predicate_method(method)
        || method == "!"
}

/// RuboCop's `potential_non_predicate?`: any return value is a non-boolean
/// literal (suppressed in conservative mode when any return is boolean).
fn potential_non_predicate(values: &[Value], opts: &Options, cx: &Cx<'_>) -> bool {
    if opts.mode == Mode::Conservative && values.iter().any(|&v| boolean_return(v, opts, cx)) {
        return false;
    }
    values.iter().any(|&v| match v {
        // Synthetic `s(:nil)` is `literal? && !boolean_type?`.
        Value::Nil => true,
        Value::Node(node) => is_literal(node, cx) && !is_boolean_literal(node, cx),
    })
}

// --- return-value collection -------------------------------------------------

/// RuboCop's `return_values(body)`, deduplicated like a `Set` is unnecessary
/// for correctness here (every method is `any`/`all` over the collection), so a
/// `Vec` suffices and preserves order for stable behavior.
fn return_values(body: NodeId, cx: &Cx<'_>) -> Vec<Value> {
    let mut values: Vec<Value> = Vec::new();

    // `Set.new(node.begin_type? ? [] : [extract_return_value(node)])`.
    if !is_begin(body, cx) {
        values.push(extract_return_value(body, cx));
    }

    // `node.each_descendant(:return)`. `descendants` excludes the node itself,
    // so a single-statement body that *is* a `return` is already covered by the
    // `!is_begin` push above (and by `last_value` below) — no double count.
    for &desc in &cx.descendants(body) {
        if matches!(*cx.kind(desc), NodeKind::Return(_)) {
            values.push(extract_return_value(desc, cx));
        }
    }

    // `return_values << last_value(node)`.
    values.push(last_value(body, cx));

    process_return_values(&values, cx)
}

/// RuboCop's `extract_return_value`. For a `return` node: bare → synthetic
/// `nil`; otherwise the single argument (Murphy already wraps `return a, b` in
/// an `array` node, matching `s(:array)`). For non-return nodes: the node.
fn extract_return_value(node: NodeId, cx: &Cx<'_>) -> Value {
    match *cx.kind(node) {
        NodeKind::Return(arg) => match arg.get() {
            None => Value::Nil,
            Some(v) => Value::Node(v),
        },
        _ => Value::Node(node),
    }
}

/// RuboCop's `last_value`. For a begin: last child (or synthetic nil); unwrap a
/// trailing `return`.
fn last_value(node: NodeId, cx: &Cx<'_>) -> Value {
    let value = if let NodeKind::Begin(list) = *cx.kind(node) {
        match cx.list(list).last() {
            Some(&last) => last,
            None => return Value::Nil,
        }
    } else {
        node
    };

    if matches!(*cx.kind(value), NodeKind::Return(_)) {
        extract_return_value(value, cx)
    } else {
        Value::Node(value)
    }
}

/// RuboCop's `process_return_values`: recursively expand conditionals and
/// `and`/`or`.
fn process_return_values(values: &[Value], cx: &Cx<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for &v in values {
        match v {
            Value::Nil => out.push(v),
            Value::Node(node) => {
                if is_conditional(node, cx) {
                    let branches = extract_conditional_branches(node, cx);
                    out.extend(process_return_values(&branches, cx));
                } else if is_and_or(node, cx) {
                    let clauses = extract_and_or_clauses(node, cx);
                    out.extend(process_return_values(&clauses, cx));
                } else {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// RuboCop's `extract_and_or_clauses`: flatten nested `and`/`or` into leaves.
fn extract_and_or_clauses(node: NodeId, cx: &Cx<'_>) -> Vec<Value> {
    match *cx.kind(node) {
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            let mut out = extract_and_or_clauses(lhs, cx);
            out.extend(extract_and_or_clauses(rhs, cx));
            out
        }
        _ => vec![Value::Node(node)],
    }
}

/// RuboCop's `extract_conditional_branches`: per-branch last values, with
/// missing branches acting as synthetic `nil`.
fn extract_conditional_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Value> {
    match *cx.kind(node) {
        NodeKind::While { body, .. } | NodeKind::Until { body, .. } => match body.get() {
            Some(b) => vec![last_value(b, cx)],
            None => vec![Value::Nil],
        },
        NodeKind::If {
            then_, else_, ..
        } => {
            let mut branches = vec![branch_last_value(then_.get(), cx)];
            match else_.get() {
                Some(e) => branches.push(branch_last_value(Some(e), cx)),
                None => branches.push(Value::Nil),
            }
            branches
        }
        NodeKind::Case { whens, else_, .. } => {
            let mut branches: Vec<Value> = cx
                .list(whens)
                .iter()
                .map(|&w| match *cx.kind(w) {
                    NodeKind::When { body, .. } => branch_last_value(body.get(), cx),
                    _ => Value::Nil,
                })
                .collect();
            match else_.get() {
                Some(e) => branches.push(branch_last_value(Some(e), cx)),
                None => branches.push(Value::Nil),
            }
            branches
        }
        _ => vec![Value::Node(node)],
    }
}

/// `branch ? last_value(branch) : s(:nil)`.
fn branch_last_value(branch: Option<NodeId>, cx: &Cx<'_>) -> Value {
    match branch {
        Some(b) => last_value(b, cx),
        None => Value::Nil,
    }
}

// --- classification helpers --------------------------------------------------

fn is_super(value: Value, cx: &Cx<'_>) -> bool {
    match value {
        Value::Nil => false,
        Value::Node(node) => matches!(*cx.kind(node), NodeKind::Super(_) | NodeKind::Zsuper),
    }
}

/// `call_type?` — a `send`/`csend`.
fn is_call(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. })
}

fn is_begin(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Begin(_))
}

fn is_and_or(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::And { .. } | NodeKind::Or { .. })
}

/// rubocop-ast `conditional?`: if / while / until / case. (`case_match` is a
/// documented gap.)
fn is_conditional(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::If { .. } | NodeKind::While { .. } | NodeKind::Until { .. } | NodeKind::Case { .. }
    )
}

/// rubocop-ast `boolean_type?`: node is `true`/`false`.
fn is_boolean_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::True_ | NodeKind::False_)
}

/// rubocop-ast `literal?`: the LITERALS set (basic + composite).
fn is_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Str(_)
            | NodeKind::Dstr(_)
            | NodeKind::Sym(_)
            | NodeKind::Dsym(_)
            | NodeKind::Array(_)
            | NodeKind::Hash(_)
            | NodeKind::Regexp { .. }
            | NodeKind::RangeExpr { .. }
            | NodeKind::Rational(_)
            | NodeKind::Complex(_)
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Nil
    )
}

/// Byte range of the method name within a `def`/`defs`, mirroring
/// `node.loc.name`. Murphy leaves `loc.name == ZERO` on defs, so the name is
/// located by source search starting past any singleton receiver. The interned
/// def symbol already carries the trailing `?`/`!`, so the range spans it.
/// Shared in spirit with `Naming/MethodName`.
fn def_name_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    let from = cx
        .def_receiver(id)
        .get()
        .map_or(0, |r| (cx.range(r).end - expr.start) as usize);
    match src[from..].find(name) {
        Some(off) => {
            let start = expr.start + (from + off) as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, Options, PredicateMethod};
    use murphy_plugin_api::test_support::{indoc, test};

    fn aggressive() -> Options {
        Options {
            mode: Mode::Aggressive,
            allowed_methods: vec!["call".to_string()],
            allowed_patterns: vec![],
            allow_bang_methods: false,
            wayward_predicates: vec!["infinite?".to_string(), "nonzero?".to_string()],
        }
    }

    // --- MSG_PREDICATE: non-predicate name, all returns boolean ---
    // (carets derived from rubocop 1.87.0 column/last_column.)

    #[test]
    fn flags_comparison_body_without_question_mark() {
        // rubocop: L1 col5..7 (`foo`).
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def foo
                ^^^ Predicate method names should end with `?`.
              bar == baz
            end
        "#});
    }

    #[test]
    fn flags_negation_body_without_question_mark() {
        // rubocop: `def neg; !x; end` → col5..7 (`neg`).
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def neg
                ^^^ Predicate method names should end with `?`.
              !x
            end
        "#});
    }

    #[test]
    fn flags_predicate_method_call_body() {
        // rubocop: `def calls_pred; bar?; end` → col5..14.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def calls_pred
                ^^^^^^^^^^ Predicate method names should end with `?`.
              bar?
            end
        "#});
    }

    #[test]
    fn flags_and_or_all_boolean() {
        // rubocop: `def andor; a == b && c == d; end` → col5..9.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def andor
                ^^^^^ Predicate method names should end with `?`.
              a == b && c == d
            end
        "#});
    }

    #[test]
    fn flags_if_else_all_branches_boolean() {
        // rubocop: `def cond; if x; a == b; else; c == d; end; end` → col5..8.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def cond
                ^^^^ Predicate method names should end with `?`.
              if x
                a == b
              else
                c == d
              end
            end
        "#});
    }

    #[test]
    fn flags_bang_method_with_boolean_body() {
        // rubocop: `def save!; true; end` → col5..9 (default AllowBangMethods).
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def save!
                ^^^^^ Predicate method names should end with `?`.
              true
            end
        "#});
    }

    // --- MSG_NON_PREDICATE: predicate name, non-boolean literal return ---

    #[test]
    fn flags_predicate_name_with_integer_literal() {
        // rubocop: `def foo?; 5; end` → col5..8.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def foo?
                ^^^^ Non-predicate method names should not end with `?`.
              5
            end
        "#});
    }

    #[test]
    fn flags_predicate_name_with_nil_literal() {
        // rubocop: `def returns_nil?; nil; end` → col5..16.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def returns_nil?
                ^^^^^^^^^^^^ Non-predicate method names should not end with `?`.
              nil
            end
        "#});
    }

    // --- conservative-mode "ok" cases (verified: no offense) ---

    #[test]
    fn allows_unknown_return_for_predicate_name() {
        // `def predicate?; bar; end` — unknown method call, conservative skip.
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def predicate?
              bar
            end
        "#});
    }

    #[test]
    fn allows_unknown_return_for_plain_name() {
        // `def foo; bar; end` — `bar` is an unknown method call (not a
        // comparison/predicate/negation), so its return type is unknown →
        // conservative `acceptable?` skips. rubocop: NO offense.
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def foo
              bar
            end
        "#});
    }

    #[test]
    fn allows_wayward_predicate_return() {
        // `def non_pred?; num.nonzero?; end` — wayward predicate is not boolean,
        // so it's an unknown method call → conservative acceptable. No offense.
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def non_pred?
              num.nonzero?
            end
        "#});
    }

    #[test]
    fn allows_predicate_with_one_boolean_return_conservative() {
        // `def with_return?; return unless bar?; true; end` — return values are
        // {nil, true}; conservative tolerates the `?` name (true is boolean).
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def with_return?
              return unless bar?
              true
            end
        "#});
    }

    #[test]
    fn allows_predicate_with_partial_boolean_branch_conservative() {
        // `def cond_mixed?; if x; true; end; end` — branches {true, nil};
        // conservative tolerates (true is boolean).
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def cond_mixed?
              if x
                true
              end
            end
        "#});
    }

    #[test]
    fn allows_super_return_conservative() {
        // `def super_case?; super; end` — super → conservative acceptable.
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def super_case?
              super
            end
        "#});
    }

    // --- allowed?: initialize, operator, AllowedMethods, body.nil? ---

    #[test]
    fn ignores_initialize() {
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def initialize
              5
            end
        "#});
    }

    #[test]
    fn ignores_operator_method() {
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def ==(other)
              true
            end
        "#});
    }

    #[test]
    fn ignores_allowed_method_call() {
        // `call` is in AllowedMethods by default.
        test::<PredicateMethod>().expect_no_offenses(indoc! {r#"
            def call
              x == y
            end
        "#});
    }

    #[test]
    fn ignores_empty_body() {
        // body.nil? → allowed (no offense even though name lacks `?`).
        test::<PredicateMethod>().expect_no_offenses("def foo\nend\n");
    }

    // --- aggressive mode (verified against rubocop -c Mode: aggressive) ---

    #[test]
    fn aggressive_flags_predicate_with_nil_return() {
        // `def with_return?; return unless bar?; true; end` → MSG_NON_PREDICATE
        // in aggressive (nil literal among returns). rubocop: L1 col5..16.
        test::<PredicateMethod>()
            .with_options(&aggressive())
            .expect_offense(indoc! {r#"
                def with_return?
                    ^^^^^^^^^^^^ Non-predicate method names should not end with `?`.
                  return unless bar?
                  true
                end
            "#});
    }

    #[test]
    fn aggressive_flags_predicate_with_missing_else() {
        // `def cond_mixed?; if x; true; end; end` → MSG_NON_PREDICATE in
        // aggressive (missing-else nil). rubocop: col5..15.
        test::<PredicateMethod>()
            .with_options(&aggressive())
            .expect_offense(indoc! {r#"
                def cond_mixed?
                    ^^^^^^^^^^^ Non-predicate method names should not end with `?`.
                  if x
                    true
                  end
                end
            "#});
    }

    #[test]
    fn aggressive_flags_super_mixed_with_boolean_return() {
        // `def foo; return super if c; true; end` — RuboCop's
        // `all_return_values_boolean?` *rejects* (skips) super/zsuper returns,
        // then judges the remaining returns: here only `true`, so the method is
        // treated as a predicate and flagged for the missing `?`. Verified vs
        // rubocop 1.87.0 (Mode: aggressive): L1 col5 "Predicate method names
        // should end with `?`." Returning `false` on super instead of skipping
        // it would diverge from RuboCop (under-report). Pins this against a
        // regression toward the wrong reading.
        test::<PredicateMethod>()
            .with_options(&aggressive())
            .expect_offense(indoc! {r#"
                def foo
                    ^^^ Predicate method names should end with `?`.
                  return super if c
                  true
                end
            "#});
    }

    // --- AllowBangMethods ---

    #[test]
    fn allow_bang_methods_true_skips_bang() {
        test::<PredicateMethod>()
            .with_options(&Options {
                allow_bang_methods: true,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {r#"
                def save!
                  true
                end
            "#});
    }

    // --- singleton def (def self.foo?) ---

    #[test]
    fn flags_singleton_predicate_with_literal() {
        // `def self.foo?; 5; end` — name `foo?` excludes receiver. rubocop
        // flags col 10..13.
        test::<PredicateMethod>().expect_offense(indoc! {r#"
            def self.foo?
                     ^^^^ Non-predicate method names should not end with `?`.
              5
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(PredicateMethod);
