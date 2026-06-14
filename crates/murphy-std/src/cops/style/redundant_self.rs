//! `Style/RedundantSelf` — flags `self.foo` calls where the `self`
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantSelf
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Pattern-matching scope (MatchAs capture, ArrayPattern, HashPattern) and
//!   self.it in parameterless blocks (Ruby 3.3+) are now handled (murphy-tpet).
//!   `self.x += 1` (op_asgn) exposes a `Send` LHS and is correctly left alone
//!   (the `self.` is required). Remaining v1 limitation: `self.x ||= 42` /
//!   `&&=` parse to Unknown (no Send visible), a translator-level gap.
//! ```
//!
//! receiver is not needed for disambiguation. Mirrors RuboCop's
//! same-named cop (autocorrect-equivalent).
//!
//! ## Matched shapes
//!
//! `Send` nodes whose receiver is a bare `SelfExpr`, when the method
//! name is a regular method (not a keyword, operator, setter,
//! constant-style, or implicit call) **and** no enclosing-scope
//! local variable / argument shadows the name.
//!
//! ## Why this shape
//!
//! Murphy mirrors RuboCop's `Style/RedundantSelf`: `self.foo` is only
//! needed when `foo` would otherwise resolve to a local variable, an
//! argument, or block-arg (Ruby's variable / method-call
//! ambiguity). Outside those scopes, `self.foo` and `foo` invoke the
//! same method, and the explicit receiver is noise.
//!
//! The scope check uses `cx.ancestors` to locate the enclosing
//! `Def` / `Defs` / `Block` (or falls back to the file root) and
//! `cx.descendants` to collect every introduced local-variable name
//! inside that scope: `Lvasgn`, `Arg`, `Optarg`, `Restarg`, `Kwarg`,
//! `Kwoptarg`, `Kwrestarg`, `Blockarg`. RuboCop ties scope to a
//! shared mutable array on the enclosing `def` / `block` so every
//! descendant Send sees the same set of names; Murphy's
//! enumerate-on-demand walk yields the same answer for the cases the
//! v1 spec covers.
//!
//! ## Autocorrect
//!
//! Replaces `receiver.start..name.start` with `""` — deletes the
//! `self.` prefix bytes in a single edit. Range is computed from the
//! receiver's expression range (the `self` text) and the selector's
//! `loc.name.start`. Idempotent: a second pass sees `foo` without a
//! receiver and emits nothing.
//!
//! ## Known v1 limitations
//!
//! - **`self.x ||= 42` / `&&=` / `op_asgn` with `self` LHS.** Prism
//!   reports these as `Unknown` nodes (no `Send` descendant), so the
//!   cop simply never sees them — matching the user-visible "no
//!   offense" expectation but not exercising the
//!   `on_or_asgn` / `on_op_asgn` "allow `self` LHS" branch from
//!   RuboCop. `foo ||= self.foo` (lvar-style LHS) is handled.
//! - **`self.it` inside parameterless blocks.** Ruby 3.3+'s
//!   `Lint/ItWithoutArgumentsInBlock` interplay is not yet wired.
//! - **Pattern-matching `in`-clauses (`case .. in`).** Match-var,
//!   array-pattern, and hash-pattern names are not collected into
//!   scope yet, and capture (`=> x`) patterns are not lowered. To avoid
//!   a meaning-changing autocorrect, the cop conservatively skips every
//!   self-send under an `InPattern` (pattern / guard / body), trading a
//!   possible missed offense for safety. The `case` subject is still
//!   checked (it runs before any binding). Real match-var scope handling
//!   is deferred.

use murphy_plugin_api::method_predicates::{is_camel_case_method, is_operator_method};
use murphy_plugin_api::{
    Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, Symbol, cop,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantSelf;

#[cop(
    name = "Style/RedundantSelf",
    description = "Avoid redundant `self.` prefixes on method calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSelf {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Ruby keywords — `self.<keyword>` must keep the receiver to remain
/// parseable. The list matches RuboCop's `KEYWORDS` constant verbatim.
const KEYWORDS: &[&str] = &[
    "alias",
    "and",
    "begin",
    "break",
    "case",
    "class",
    "def",
    "defined?",
    "do",
    "else",
    "elsif",
    "end",
    "ensure",
    "false",
    "for",
    "if",
    "in",
    "module",
    "next",
    "nil",
    "not",
    "or",
    "redo",
    "rescue",
    "retry",
    "return",
    "self",
    "super",
    "then",
    "true",
    "undef",
    "unless",
    "until",
    "when",
    "while",
    "yield",
    "__FILE__",
    "__LINE__",
    "__ENCODING__",
];

/// Non-CamelCase names in `Kernel.methods(false)`. CamelCase names
/// (`Array`, `Complex`, `Float`, `Hash`, `Integer`, `Rational`,
/// `String`) are already filtered by [`is_camel_case_method`]; the
/// backtick operator (`` ` ``) is filtered by [`is_operator_method`]
/// (now faithfully via the shared `MethodIdentifierPredicates` set,
/// which — unlike the previous hand-rolled copy — actually includes
/// `` ` ``, `!@`, and `~@`).
/// Enumerated against MRI 4.0 — keep in sync with the upstream surface.
const KERNEL_METHODS: &[&str] = &[
    "__callee__",
    "__dir__",
    "__method__",
    "abort",
    "at_exit",
    "autoload",
    "autoload?",
    "binding",
    "block_given?",
    "caller",
    "caller_locations",
    "catch",
    "eval",
    "exec",
    "exit",
    "exit!",
    "fail",
    "fork",
    "format",
    "gets",
    "global_variables",
    "iterator?",
    "lambda",
    "load",
    "local_variables",
    "loop",
    "open",
    "p",
    "pp",
    "print",
    "printf",
    "proc",
    "putc",
    "puts",
    "raise",
    "rand",
    "readline",
    "readlines",
    "select",
    "set_trace_func",
    "sleep",
    "spawn",
    "sprintf",
    "srand",
    "syscall",
    "system",
    "test",
    "throw",
    "trace_var",
    "trap",
    "untrace_var",
    "warn",
];

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return;
    };

    // Receiver must be a bare `self` literal. `is_self_receiver` routes
    // through `call_receiver` (the shared dispatch surface, es99.1); we
    // still keep `receiver_id` for the autocorrect range below.
    let OptNodeId(idx) = receiver;
    if idx == u32::MAX {
        return;
    }
    let receiver_id = NodeId(idx);
    if !cx.is_self_receiver(node) {
        return;
    }

    // No selector range ⇒ implicit `self.()` call. The dot operator
    // is still required; nothing to remove.
    let name_range = cx.loc(node).name;
    if name_range == Range::ZERO {
        return;
    }

    let Some(method_name) = cx.method_name(node) else {
        return;
    };

    // Setter (`self.foo = bar`): method ends with `=`. Removing
    // `self.` would change the meaning to `foo = bar` (local
    // assignment), not an attr-writer call.
    if method_name.ends_with('=') {
        return;
    }

    // Op-assign target (`self.foo += bar`, `self.foo -= 1`): the Send is the
    // assignment target. Removing `self.` would read/write a local variable,
    // so `self.` is required (RuboCop's `on_op_asgn` allows the self LHS).
    if cx.parent(node).get().is_some_and(|parent_id| {
        matches!(
            *cx.kind(parent_id),
            NodeKind::OpAsgn { target, .. }
                | NodeKind::OrAsgn { target, .. }
                | NodeKind::AndAsgn { target, .. }
            if target == node
        )
    }) {
        return;
    }

    // Operator method (`self.+`, `self.<<`, `self.[]`, …).
    if is_operator_method(method_name) {
        return;
    }

    // CamelCase method (`self.Foo`) — disambiguates from a constant
    // reference of the same name.
    if is_camel_case_method(method_name) {
        return;
    }

    // Ruby keyword (`self.if`, `self.class`, …).
    if KEYWORDS.contains(&method_name) {
        return;
    }

    // `Kernel.methods(false)` — RuboCop intentionally skips these
    // because the bare call may not resolve the same way as the
    // explicit-self call (e.g. `puts` reaches Kernel#puts, but a
    // private/protected override on the receiver could shadow it).
    if KERNEL_METHODS.contains(&method_name) {
        return;
    }

    // Parallel-assignment LHS (`a, self.b = c, d`). RuboCop's gate is
    // `node.parent&.mlhs_type?`; we walk the immediate parent only.
    if let Some(parent) = cx.parent(node).get()
        && matches!(cx.kind(parent), NodeKind::Mlhs(_))
    {
        return;
    }

    // Pattern matching (`case .. in`) binds match-vars as locals visible in
    // the clause's pattern, guard, and body. Mirror RuboCop's `on_in_pattern`:
    // collect every `MatchVar` descendant of the pattern slot and skip
    // `self.x` only when `x` is among those bindings.
    //
    // When the enclosing `InPattern` contains an `Unknown` node (i.e. a
    // pattern construct not yet lowered, like `MatchAs`/pin), a binding may
    // be invisible — fall back to the conservative full-skip to avoid a
    // meaning-changing autocorrect. The `case` subject is deliberately *not*
    // covered — it runs before any binding, so `case self.foo` is still
    // flagged.
    // Walk every enclosing InPattern (handles nested `case/in`). For each:
    // - Unknown child → conservative skip (binding may be invisible)
    // - binds the name → skip
    // - otherwise → continue to outer InPatterns
    // Fall through only when no enclosing InPattern binds `method_name`.
    for ancestor in cx.ancestors(node) {
        if let NodeKind::InPattern { pattern, .. } = *cx.kind(ancestor) {
            if pattern_has_unknown(cx, pattern) {
                return;
            }
            if pattern_binds_name(cx, pattern, method_name) {
                return;
            }
            // This in-clause doesn't bind the name; check outer ones.
        }
    }

    // Ruby 3.3+ implicit `it` block param: inside a parameterless block
    // (no `|...|` delimiters), `self.it` with no arguments cannot be
    // simplified to bare `it` because that would reference the implicit
    // block parameter rather than calling the method. Allow `self.it`
    // only when the block has explicit pipe delimiters or named params.
    if it_method_in_parameterless_block(node, method_name, cx) {
        return;
    }

    // Enclosing scope: the nearest `Def` / `Defs` / `Block` ancestor,
    // or the file root for top-level code.
    let scope = enclosing_scope(cx, node).unwrap_or_else(|| cx.root());
    if scope_introduces_name(cx, scope, method_name) {
        return;
    }

    // Offense and autocorrect. Offense range is the receiver text
    // (`self`); the edit removes `self.` up to the selector start.
    let recv_range = cx.range(receiver_id);
    cx.emit_offense(recv_range, "Redundant `self` detected.", None);
    cx.emit_edit(
        Range {
            start: recv_range.start,
            end: name_range.start,
        },
        "",
    );
}

/// Nearest `Def` (instance or singleton) or `Block` ancestor of `node`,
/// or `None` for top-level code. Murphy uses one `Def` variant with an
/// optional receiver (the `def self.foo` case); RuboCop splits these as
/// `on_def` + `on_defs` aliased to the same handler — same scope shape.
fn enclosing_scope(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for ancestor in cx.ancestors(node) {
        if matches!(
            cx.kind(ancestor),
            NodeKind::Def { .. } | NodeKind::Block { .. }
        ) {
            return Some(ancestor);
        }
    }
    None
}

/// `true` when `scope` or any of its descendants (excluding nested
/// `Def` / `Block` subtrees) introduces a local-variable name equal
/// to `name`. Mirrors RuboCop's shared-array trick: every lvasgn /
/// parameter inside the enclosing scope contributes to the scope's
/// visible-names set, regardless of source position; but
/// name-introductions inside a nested `Def` / `Block` belong to that
/// inner scope, not the outer one. The walk descends children
/// directly so it can stop at nested-scope boundaries.
///
/// The scope node itself is also checked because for top-level code
/// the fallback scope is the AST root, and the root can be the
/// name-introducing node directly (e.g. `a = self.a` whose root is
/// the `Lvasgn` for `a`).
fn scope_introduces_name(cx: &Cx<'_>, scope: NodeId, name: &str) -> bool {
    if descendant_introduces_name(cx, scope, name) {
        return true;
    }
    let mut stack: Vec<NodeId> = cx.children(scope);
    stack.reverse();
    while let Some(n) = stack.pop() {
        if descendant_introduces_name(cx, n, name) {
            return true;
        }
        // Nested `Def` / `Block` starts a fresh scope — its
        // parameters and lvasgns are not visible to the outer Send.
        if matches!(cx.kind(n), NodeKind::Def { .. } | NodeKind::Block { .. }) {
            continue;
        }
        let mut kids = cx.children(n);
        kids.reverse();
        stack.extend(kids);
    }
    false
}

fn descendant_introduces_name(cx: &Cx<'_>, desc: NodeId, name: &str) -> bool {
    let matches_sym = |sym: Symbol| cx.symbol_str(sym) == name;
    match *cx.kind(desc) {
        NodeKind::Lvasgn { name: n, .. } => matches_sym(n),
        NodeKind::Arg(n) => matches_sym(n),
        NodeKind::Restarg(n) => matches_sym(n),
        NodeKind::Kwarg(n) => matches_sym(n),
        NodeKind::Kwrestarg(n) => matches_sym(n),
        NodeKind::Blockarg(n) => matches_sym(n),
        NodeKind::Optarg { name: n, .. } => matches_sym(n),
        NodeKind::Kwoptarg { name: n, .. } => matches_sym(n),
        _ => false,
    }
}

/// `true` when the pattern subtree contains an `Unknown` node — meaning a
/// pattern construct is not yet lowered (e.g. `MatchAs`/pin), so a match-var
/// binding may be invisible.
fn pattern_has_unknown(cx: &Cx<'_>, pattern: NodeId) -> bool {
    if matches!(cx.kind(pattern), NodeKind::Unknown) {
        return true;
    }
    cx.descendants(pattern)
        .iter()
        .any(|&d| matches!(cx.kind(d), NodeKind::Unknown))
}

/// `true` when the pattern subtree contains a `MatchVar` whose name is `name`.
/// Mirrors RuboCop's `add_match_var_scopes` descendant collection.
fn pattern_binds_name(cx: &Cx<'_>, pattern: NodeId, name: &str) -> bool {
    let is_match_var = |id: NodeId| {
        if let NodeKind::MatchVar(s) = *cx.kind(id) {
            cx.symbol_str(s) == name
        } else {
            false
        }
    };
    is_match_var(pattern) || cx.descendants(pattern).iter().any(|&d| is_match_var(d))
}

/// Returns `true` when `node` is a `self.it()` send (no arguments, no block)
/// inside a block that has no explicit `|...|` pipe delimiters — the case
/// where bare `it` would resolve to Ruby 3.3+'s implicit block parameter
/// instead of calling the method. Mirrors RuboCop's `it_method_in_block?`.
///
/// The ancestor walk stops at the first `Def` / `Defs` boundary: a `self.it`
/// inside a `def` nested inside a parameterless block is not covered by the
/// outer block's implicit `it` parameter — the `def` starts a fresh scope.
fn it_method_in_parameterless_block(node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    if method_name != "it" {
        return false;
    }

    // self.it must take no arguments
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return false;
    };
    if !cx.list(args).is_empty() {
        return false;
    }

    // Walk ancestors looking for the nearest Block. Stop immediately if we
    // hit a Def/Defs — a `def` defines a new scope, so the outer parameterless
    // block's implicit `it` param is not visible inside it.
    let mut block_ancestor: Option<NodeId> = None;
    for ancestor in cx.ancestors(node) {
        match *cx.kind(ancestor) {
            NodeKind::Def { .. } => return false, // new scope — outer `it` not visible
            NodeKind::Block { .. } => {
                block_ancestor = Some(ancestor);
                break;
            }
            _ => {}
        }
    }
    let Some(block_ancestor) = block_ancestor else {
        return false;
    };

    let NodeKind::Block {
        args: block_args_id,
        body,
        ..
    } = *cx.kind(block_ancestor)
    else {
        return false;
    };

    // Block must have an empty args node (no parameters)
    let NodeKind::Args(args_list) = *cx.kind(block_args_id) else {
        return false;
    };
    if !cx.list(args_list).is_empty() {
        return false;
    }

    // Now check for explicit pipe delimiters using the token stream.
    // In `{ || }` the `|` tokens appear right after the `{`/`do` opener;
    // in `{ }` there are none. We scan tokens in the range from the block
    // start up to (but not including) the body start.
    let block_start = cx.range(block_ancestor).start;
    let search_end = match body.get() {
        Some(b) => cx.range(b).start,
        None => cx.range(block_ancestor).end,
    };
    !block_has_pipe_delimiters(cx, block_start, search_end)
}

/// Returns `true` when any `|` token appears in the range `[start..end)`.
/// Used to detect explicit block parameter pipes (`{ ||` or `{ |x|`).
fn block_has_pipe_delimiters(cx: &Cx<'_>, start: u32, end: u32) -> bool {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < end)
        .any(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"|"
        })
}

#[cfg(test)]
mod tests {
    use super::RedundantSelf;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense + correction) -------------------

    #[test]
    fn flags_self_on_rvalue_with_different_name() {
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                a = self.b
                    ^^^^ Redundant `self` detected.
            "},
            "a = b\n",
        );
    }

    #[test]
    fn flags_self_on_plain_call() {
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                self.call
                ^^^^ Redundant `self` detected.
            "},
            "call\n",
        );
    }

    #[test]
    fn flags_second_self_after_or_asgn_with_same_lhs() {
        // RuboCop spec: when `self.x ||= 42` is followed by a bare
        // `self.x` (not the LHS of the or-asgn), the second use is
        // an offense. In Murphy v1 the `self.x ||= 42` parses to
        // `Unknown` so its `Send` is invisible; the standalone
        // `self.x` on the next line is still flagged exactly as
        // RuboCop would. Pinning this test guards against a future
        // translator change that exposes the or-asgn Send.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                self.x ||= 42
                self.x
                ^^^^ Redundant `self` detected.
            "},
            indoc! {"
                self.x ||= 42
                x
            "},
        );
    }

    #[test]
    fn flags_self_when_different_lvasgn_in_if() {
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                a = x if self.b
                         ^^^^ Redundant `self` detected.
            "},
            "a = x if b\n",
        );
    }

    #[test]
    fn flags_self_in_def_body_when_arg_name_differs() {
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                def foo(bar)
                  self.baz
                  ^^^^ Redundant `self` detected.
                end
            "},
            indoc! {"
                def foo(bar)
                  baz
                end
            "},
        );
    }

    // ----- Negative cases — name shadowing -------------------------

    #[test]
    fn accepts_self_when_method_matches_lvasgn() {
        test::<RedundantSelf>().expect_no_offenses("a = self.a\n");
    }

    #[test]
    fn accepts_self_when_method_matches_or_asgn_lvar() {
        test::<RedundantSelf>().expect_no_offenses("foo ||= self.foo\n");
    }

    #[test]
    fn accepts_self_as_op_assignment_target() {
        // `self.foo += bar` desugars to `self.foo = self.foo + bar`; without
        // `self.` the LHS introduces a local variable, so `self.` is required.
        test::<RedundantSelf>().expect_no_offenses("self.ignored_columns += %w(a b)\n");
        test::<RedundantSelf>().expect_no_offenses("self.count -= 1\n");
    }

    #[test]
    fn accepts_self_when_method_matches_and_asgn_lvar() {
        test::<RedundantSelf>().expect_no_offenses("foo &&= self.foo\n");
    }

    #[test]
    fn accepts_self_when_method_matches_method_arg() {
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            def foo(bar)
              self.bar
            end
        "});
    }

    #[test]
    fn accepts_self_when_method_matches_blockarg() {
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            def foo(&block)
              self.block
            end
        "});
    }

    #[test]
    fn accepts_self_when_method_matches_optional_arg() {
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            def foo(final = true)
              self.final
            end
        "});
    }

    #[test]
    fn accepts_self_when_method_matches_local_inside_def() {
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            def foo
              bar = 1
              self.bar
            end
        "});
    }

    #[test]
    fn accepts_self_when_method_matches_lvasgn_in_nested_rhs() {
        // RuboCop spec: `a = self.a || b || c` — the Send sits
        // inside an `Or` chain inside the lvasgn rhs. The enclosing
        // scope still introduces `:a` so the Send is skipped.
        test::<RedundantSelf>().expect_no_offenses("a = self.a || b || c\n");
    }

    #[test]
    fn accepts_self_when_method_matches_lvasgn_in_if_condition_and_body() {
        // RuboCop spec: `a = self.a if self.a` — both Sends see
        // `:a` via the enclosing-scope walk.
        test::<RedundantSelf>().expect_no_offenses("a = self.a if self.a\n");
    }

    #[test]
    fn accepts_self_when_method_matches_masgn_lvar() {
        // RuboCop spec: `a, b = self.a` — the Send is on the rhs;
        // the Masgn LHS introduces `:a` so the Send is skipped.
        test::<RedundantSelf>().expect_no_offenses("a, b = self.a\n");
    }

    #[test]
    fn accepts_self_in_masgn_with_matching_name() {
        // `a, b = self.a` — Masgn LHS introduces both names; method
        // matches one.
        test::<RedundantSelf>().expect_no_offenses("a, b = self.a\n");
    }

    // ----- Negative cases — syntactic exemptions -------------------

    #[test]
    fn accepts_self_setter() {
        test::<RedundantSelf>().expect_no_offenses("self.a = b\n");
    }

    #[test]
    fn accepts_self_on_mlhs_lvalue() {
        // `a, self.b = c, d` — Mlhs gate.
        test::<RedundantSelf>().expect_no_offenses("a, self.b = c, d\n");
    }

    #[test]
    fn accepts_self_bracket_operator() {
        test::<RedundantSelf>().expect_no_offenses("self[a]\n");
    }

    #[test]
    fn accepts_self_double_less_than_operator() {
        test::<RedundantSelf>().expect_no_offenses("self << a\n");
    }

    #[test]
    fn accepts_self_plus_operator() {
        test::<RedundantSelf>().expect_no_offenses("self.+(1)\n");
    }

    #[test]
    fn accepts_self_camel_case_method() {
        test::<RedundantSelf>().expect_no_offenses("self.Foo\n");
    }

    #[test]
    fn accepts_self_backtick_operator() {
        // Regression: the previous hand-rolled operator set omitted the
        // backtick method, so `self.\`` was wrongly flagged. Delegating
        // to the shared `operator_method?` set (which includes `` ` ``)
        // fixes it — `self.\`(cmd)` must not be reported.
        test::<RedundantSelf>().expect_no_offenses("self.`(\"ls\")\n");
    }

    #[test]
    fn accepts_self_keyword_method() {
        // Every name in the KEYWORDS list — pick `if` as a smoke
        // test; the constant pins the full list.
        test::<RedundantSelf>()
            .expect_no_offenses("self.if\n")
            .expect_no_offenses("self.class\n")
            .expect_no_offenses("self.return\n")
            .expect_no_offenses("self.yield\n")
            .expect_no_offenses("self.__FILE__\n");
    }

    #[test]
    fn flags_self_when_matching_name_is_in_nested_block_only() {
        // Nested-scope guard: the `bar` block-arg in `proc { |bar| }`
        // belongs to the inner block scope, not the outer def. The
        // outer `self.bar` does not see it and must be flagged.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                def outer
                  self.bar
                  ^^^^ Redundant `self` detected.
                  proc { |bar| }
                end
            "},
            indoc! {"
                def outer
                  bar
                  proc { |bar| }
                end
            "},
        );
    }

    #[test]
    fn flags_self_when_matching_name_is_in_nested_def_only() {
        // Same idea as the block case but with a nested `def`.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                def outer
                  self.bar
                  ^^^^ Redundant `self` detected.
                  def inner(bar); end
                end
            "},
            indoc! {"
                def outer
                  bar
                  def inner(bar); end
                end
            "},
        );
    }

    #[test]
    fn accepts_self_when_matching_block_arg_in_same_block() {
        // Mirror case of the nested-scope guard: when the Send is
        // *inside* the block whose arg matches, the cop must skip.
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            [1, 2].each do |bar|
              self.bar
            end
        "});
    }

    #[test]
    fn accepts_self_for_kernel_methods() {
        // RuboCop's `KERNEL_METHODS = Kernel.methods(false)` exemption
        // (`self.open`, `self.puts`, …) — bare calls may not resolve
        // identically, so explicit-self is intentionally preserved.
        test::<RedundantSelf>()
            .expect_no_offenses("self.open\n")
            .expect_no_offenses("self.puts\n")
            .expect_no_offenses("self.lambda { }\n")
            .expect_no_offenses("self.block_given?\n");
    }

    #[test]
    fn accepts_self_implicit_call() {
        // `self.()` — no selector range, nothing to remove.
        test::<RedundantSelf>().expect_no_offenses("self.()\n");
    }

    // ----- self.it in parameterless blocks (murphy-tpet) ------------------

    #[test]
    fn accepts_self_it_in_parameterless_brace_block() {
        // Ruby 3.3+ implicit `it` block param: `0.times { self.it }` must NOT
        // be flagged — removing `self.` would change the meaning from the
        // method call `it` to the implicit block parameter `it`.
        test::<RedundantSelf>().expect_no_offenses("0.times { self.it }\n");
    }

    #[test]
    fn accepts_self_it_in_parameterless_do_block() {
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            0.times do
              self.it
            end
        "});
    }

    #[test]
    fn flags_self_it_in_def_body() {
        // Inside a `def`, `it` is never an implicit block param.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                def foo
                  self.it
                  ^^^^ Redundant `self` detected.
                end
            "},
            indoc! {"
                def foo
                  it
                end
            "},
        );
    }

    #[test]
    fn flags_self_it_in_block_with_explicit_empty_pipes() {
        // `{ || self.it }` — block with explicit empty parameters: since the
        // explicit `||` means the block explicitly accepts no `it` param,
        // `self.it` is redundant.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                0.times { ||
                  self.it
                  ^^^^ Redundant `self` detected.
                }
            "},
            indoc! {"
                0.times { ||
                  it
                }
            "},
        );
    }

    #[test]
    fn flags_self_it_in_block_with_named_param() {
        // `{ |_n| self.it }` — block has a named param, so `it` is not
        // the implicit block parameter.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                0.times { |_n|
                  self.it
                  ^^^^ Redundant `self` detected.
                }
            "},
            indoc! {"
                0.times { |_n|
                  it
                }
            "},
        );
    }

    #[test]
    fn flags_self_it_in_def_inside_parameterless_block() {
        // `0.times { def foo; self.it; end }` — the `def` starts a fresh
        // scope. The outer parameterless block's implicit `it` param is NOT
        // visible inside the `def`, so `self.it` is genuinely redundant.
        test::<RedundantSelf>().expect_correction(
            indoc! {"
                0.times {
                  def foo
                    self.it
                    ^^^^ Redundant `self` detected.
                  end
                }
            "},
            indoc! {"
                0.times {
                  def foo
                    it
                  end
                }
            "},
        );
    }

    // ----- v1 known-limitation pins --------------------------------

    #[test]
    fn pattern_matching_in_clause_is_skipped_conservatively() {
        // `in Integer => bar` uses `MatchAs` (now fully lowered by the
        // translator). The cop finds the `MatchVar(:bar)` binding inside the
        // MatchAs pattern and correctly suppresses the offense — `self.bar`
        // is NOT redundant because `bar` is a match-var local.
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            case foo
            in Integer => bar
              self.bar
            end
        "});
    }

    #[test]
    fn case_match_subject_self_is_still_checked() {
        // The `case` subject runs before any match-var binding exists, so a
        // redundant `self.foo` there is still flagged (the InPattern skip
        // only covers pattern/guard/body, not the subject). No local named
        // `foo` is in scope, so `self.foo` is genuinely redundant.
        test::<RedundantSelf>().expect_offense(indoc! {"
            case self.foo
                 ^^^^ Redundant `self` detected.
            in Integer
              1
            end
        "});
    }

    #[test]
    fn self_op_asgn_lhs_is_skipped_via_parser_unknown() {
        // `self.x ||= 42` parses to a `NodeKind::Unknown` in Murphy
        // — the inner `Send` never reaches dispatch — so the cop
        // never sees it. The lack-of-offense matches RuboCop's
        // expected behaviour even though the path is not exercised.
        // Pinned so a future translator improvement that lowers
        // `self.x ||= 42` into a visible `OrAsgn`+`Send` flips this
        // test alongside the cop logic.
        test::<RedundantSelf>().expect_no_offenses("self.x ||= 42\n");
    }

    // ----- pattern-match scope precision (jw5t) ----------------------------

    #[test]
    fn array_pattern_match_var_shadows_self_send() {
        // `in [a, b]` binds `a` and `b` as locals — `self.a` inside the body
        // is not redundant and must NOT be flagged.
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            case foo
            in [a, b]
              self.a
            end
        "});
    }

    #[test]
    fn hash_pattern_shorthand_shadows_self_send() {
        // `in {a:}` binds `a` — `self.a` is not redundant.
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            case foo
            in {a:}
              self.a
            end
        "});
    }

    #[test]
    fn nested_case_in_outer_binding_is_respected() {
        // Nested `case/in`: the outer pattern binds `a`, the inner binds `b`.
        // `self.a` inside the inner body must NOT be flagged — the loop must
        // walk past the inner InPattern and check the outer one.
        // (The old `break`-based implementation regressed this.)
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            case x
            in [a]
              case y
              in [b]
                self.a
              end
            end
        "});
        // The inner binding `b` is visible from the inner body too.
        test::<RedundantSelf>().expect_no_offenses(indoc! {"
            case x
            in [a]
              case y
              in [b]
                self.b
              end
            end
        "});
    }

    #[test]
    fn unbound_name_in_pattern_is_still_flagged() {
        // `in [a]` binds `a` but not `b`, so `self.b` is genuinely redundant.
        test::<RedundantSelf>().expect_offense(indoc! {"
            case foo
            in [a]
              self.b
              ^^^^ Redundant `self` detected.
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(RedundantSelf);
