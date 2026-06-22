//! `Naming/ConstantName` — flag constant assignments whose name is not written
//! in SCREAMING_SNAKE_CASE.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/ConstantName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `on_casgn`:
//!
//!     value = node.parent&.or_asgn_type? ? node.parent.expression
//!                                        : node.expression
//!     return if allowed_assignment?(value)
//!     return if SNAKE_CASE.match?(node.name)   # /^[[:digit:][:upper:]_]+$/
//!     add_offense(node.loc.name)
//!
//!   The cop ONLY runs on `casgn` (constant assignment), never on constant
//!   reads or class/module declaration names — verified `Foo::Foo`,
//!   `class Foo`, and bare `FOO` reads against rubocop 1.87.0.
//!
//!   `allowed_assignment?` skips the offense when the assigned value is one of:
//!     * a `block`/`const`/`casgn` node (`X = OtherConst`, `X = Klass.new {}`,
//!       `X = (Y = 1)`);
//!     * a method call with no receiver OR a non-literal receiver
//!       (`X = compute`, `X = Foo.bar`) — but NOT a literal-receiver call
//!       (`X = "s".freeze`, `X = ("s").freeze`, `X = 5.to_s`, `X = [1].first`),
//!       which ARE flagged;
//!     * a `Class.new(...)` / `Struct.new(...)` call (any/no scope on the
//!       `Class`/`Struct` const, incl. `::Class.new`);
//!     * an `if`/ternary expression with a `const` directly in one of its
//!       branches.
//!
//!   Value-unwrap parity (verified, intentionally does NOT unwrap parens):
//!   RuboCop matches on the raw `node.expression` type, so a parenthesized
//!   value is a `begin`, not the node it wraps. Therefore `X = (compute)`
//!   (begin, not send) and `X = (a ? FOO : 2)` (begin, not if) ARE flagged,
//!   while their bare forms are not. Mirrors `.claude/rules/
//!   parenthesized-expressions.md`'s caveat that effective-kind unwrapping is
//!   wrong when the upstream rule keys off the literal node type.
//!
//!   Conditional-branch parity mirrors RuboCop's `IfNode#branches`: only
//!   immediate branches count and the walk recurses through `elsif` chains
//!   (the `else` of an `elsif` is a bare nested `if`) but NOT through a
//!   parenthesized nested `if` (a `begin`). A `const` only counts when it is
//!   the branch expression itself — a const in the condition, or buried in a
//!   branch's sub-expression (`else foo(FOO)`, `else [FOO]`), does NOT count.
//!   Verified: `if a then FOO else 2 end` ok; `if a then 1 elsif b then FOO
//!   else 2 end` ok; `if a then 1 else (if b then FOO else 2 end) end` flagged;
//!   `if FOO then 1 else 2 end` flagged.
//!
//!   `or_asgn` is the only parent special-case: for `Foo ||= 1` the value is
//!   read from the parent `or-asgn`'s RHS. `Foo &&= 1` (and_asgn) and
//!   `Foo += 1` (op_asgn) are NOT special-cased — their value falls through to
//!   the value-less casgn's own (absent) expression → not allowed → flagged.
//!   `Foo = bar&.baz` (csend RHS) is flagged: RuboCop's allowed-method check
//!   tests `send_type?`, and a safe-nav call is not a `send`.
//!
//!   Block-family parity: `block` RHS is allowed (`X = foo do end`,
//!   `X = foo { it }` — under Ruby 3.3.5 `it` lexes as a method call so the
//!   block is a plain `block`/`Itblock`); a `numblock` RHS (`X = foo { _1 }`)
//!   is NOT allowed and is flagged, matching rubocop 1.87.0 where numbered
//!   blocks are their own node type outside the `block/const/casgn` allow-set.
//!
//!   Offense range: RuboCop highlights `node.loc.name` (the leaf constant
//!   name). Murphy does not populate `loc.name` on `casgn`, so the range is
//!   computed by locating the leaf name after the scope (`Foo::Foo` → the
//!   second `Foo`). Verified columns: `Foo = 1` col 1..3; `MyMod::Foo = 1`
//!   col 8..10; `Foo::Foo = 1` col 6..8.
//!
//!   Known minor gaps (rare, documented; status stays `verified` for all
//!   real-world ASCII code):
//!     * `else if` (two words, NOT a single `elsif` keyword) nested-if on the
//!       RHS — e.g. `X = if a then 1 else if b then BAR else 2 end end`. Prism
//!       produces an identical nested-`If`-in-`else_` AST for both `elsif` and
//!       `else`+`if`, so they cannot be told apart by node kind. RuboCop
//!       distinguishes them token-wise (`IfNode#elsif?` checks the `elsif`
//!       keyword) and does NOT recurse into a plain `else if`, so it flags the
//!       example above. Murphy recurses for both shapes → rare false negative.
//!       Distinguishing would require token inspection; not worth the cost for
//!       how rare `else if` is. Verified against rubocop 1.87.0.
//!     * a constant name containing non-ASCII-cased letters (`ÉcoLe`) is not
//!       flagged by rubocop, but Murphy's SNAKE_CASE check (ASCII
//!       `A-Z`/`0-9`/`_` only) would flag it. The exact mechanism of rubocop's
//!       POSIX-class handling here is not characterized; non-ASCII constant
//!       names are vanishingly rare.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use SCREAMING_SNAKE_CASE for constants.";

#[derive(Default)]
pub struct ConstantName;

#[cop(
    name = "Naming/ConstantName",
    description = "Constants should use SCREAMING_SNAKE_CASE.",
    default_severity = "warning",
    default_enabled = true
)]
impl ConstantName {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { scope, name, value } = *cx.kind(node) else {
            return;
        };

        // RuboCop reads the value from the parent `or-asgn` for `Foo ||= 1`
        // (the casgn target itself is value-less there); otherwise from the
        // casgn's own `value`. `and_asgn`/op-asgn are intentionally NOT
        // special-cased.
        let value = match cx.parent(node).get() {
            Some(parent) => match *cx.kind(parent) {
                NodeKind::OrAsgn { value, .. } => Some(value),
                _ => value.get(),
            },
            None => value.get(),
        };

        if allowed_assignment(value, cx) {
            return;
        }

        let name_str = cx.symbol_str(name);
        if is_screaming_snake_case(name_str) {
            return;
        }

        cx.emit_offense(name_range(node, scope.get(), name_str, cx), MSG, None);
    }
}

/// SCREAMING_SNAKE_CASE — RuboCop's `/^[[:digit:][:upper:]_]+$/`. Every
/// character must be an ASCII uppercase letter, an ASCII digit, or an
/// underscore. (Non-ASCII uppercase is a documented minor gap.)
fn is_screaming_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// Port of RuboCop's `allowed_assignment?`. `value` is `None` when the
/// assignment has no resolvable RHS (e.g. `Foo &&= 1`, whose casgn target is
/// value-less and whose parent is not an `or-asgn`); RuboCop's matchers all
/// short-circuit on `nil`, so `None` is never allowed.
fn allowed_assignment(value: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(value) = value else {
        return false;
    };

    // `%i[block const casgn].include?(value.type)`. `Itblock` is treated like
    // `block` (see parity note); `Numblock` is deliberately excluded.
    if matches!(
        *cx.kind(value),
        NodeKind::Block { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Const { .. }
            | NodeKind::Casgn { .. }
    ) {
        return true;
    }

    allowed_method_call_on_rhs(value, cx)
        || class_or_struct_return_method(value, cx)
        || allowed_conditional_expression_on_rhs(value, cx)
}

/// `allowed_method_call_on_rhs?` — `node&.send_type? && (node.receiver.nil? ||
/// !literal_receiver?(node))`. A safe-navigation call (`csend`) is NOT a
/// `send`, so it is not allowed.
fn allowed_method_call_on_rhs(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return false;
    };
    match receiver.get() {
        None => true,
        Some(recv) => !is_literal_receiver(recv, cx),
    }
}

/// `literal_receiver?` — `{(send literal? ...) (send (begin literal?) ...)}`.
/// The receiver is literal either directly or wrapped in a single-level
/// parenthesized `begin`. This is the ONE place a `begin` is peeked into, and
/// only one level, only on the receiver.
fn is_literal_receiver(receiver: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_literal(receiver) {
        return true;
    }
    if let NodeKind::Begin(list) = *cx.kind(receiver)
        && let [inner] = cx.list(list)
    {
        return cx.is_literal(*inner);
    }
    false
}

/// `class_or_struct_return_method?` — `(send (const _ {:Class :Struct}) :new
/// ...)`. The receiver const may carry any/no scope (`::Class`, `Foo::Class`).
fn class_or_struct_return_method(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return false;
    };
    if cx.method_name(node) != Some("new") {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Const { name, .. } = *cx.kind(recv) else {
        return false;
    };
    matches!(cx.symbol_str(name), "Class" | "Struct")
}

/// `allowed_conditional_expression_on_rhs?` — `node&.if_type? &&
/// contains_constant?(node)`.
fn allowed_conditional_expression_on_rhs(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::If { .. }) && contains_constant(node, cx)
}

/// `contains_constant?` — `node.branches.compact.any?(&:const_type?)`.
/// Mirrors `IfNode#branches`: the immediate `then`/`else` branches, recursing
/// through `elsif` chains (where the `else` is itself a bare `if`) but not
/// through a parenthesized nested `if` (a `begin`). A branch counts only when
/// it is *directly* a `const` node.
fn contains_constant(if_node: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = if_node;
    loop {
        let NodeKind::If {
            then_, else_, ..
        } = *cx.kind(current)
        else {
            return false;
        };

        if then_.get().is_some_and(|b| is_const(b, cx)) {
            return true;
        }

        match else_.get() {
            None => return false,
            Some(else_branch) => {
                // Recurse only through a bare `elsif` (a nested `if`); a
                // parenthesized `if` is a `begin` and stops the walk.
                if matches!(*cx.kind(else_branch), NodeKind::If { .. }) {
                    current = else_branch;
                } else {
                    return is_const(else_branch, cx);
                }
            }
        }
    }
}

fn is_const(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Const { .. })
}

/// Byte range of the leaf constant name, mirroring RuboCop's `node.loc.name`.
/// Murphy leaves `loc.name` as `Range::ZERO` for `casgn`, so the leaf is
/// located by searching for `name` starting after the scope segment (so
/// `Foo::Foo` anchors the second `Foo`). Falls back to the node start if the
/// name is not found (should not happen).
fn name_range(node: NodeId, scope: Option<NodeId>, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(node);
    let search_start = scope.map_or(expr.start, |s| cx.range(s).end);
    let haystack = cx.raw_source(Range {
        start: search_start,
        end: expr.end,
    });
    match haystack.find(name) {
        Some(off) => {
            let start = search_start + off as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start + name.len() as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::ConstantName;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses (carets derived from rubocop 1.87.0 column..last_column;
    //     leading spaces = column-1, carets = last_column-column+1). ---

    #[test]
    fn flags_camel_case_constant() {
        // rubocop: line 1, col 1..3 (`Foo`)
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = 1
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_mixed_case_constant() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            InchInCm = 2.54
            ^^^^^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_literal_string_assignment() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Baz = "literal"
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_scoped_constant_leaf() {
        // `MyMod::Foo = 1`: rubocop flags the leaf `Foo` at col 8..10.
        test::<ConstantName>().expect_offense(indoc! {r#"
            MyMod::Foo = 1
                   ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_repeated_scope_leaf() {
        // `Foo::Foo = 1`: rubocop flags the leaf (second `Foo`) at col 6..8,
        // NOT the scope segment.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo::Foo = 1
                 ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    // --- or_asgn / and_asgn / op_asgn parent handling ---

    #[test]
    fn flags_or_asgn_literal() {
        // `Foo ||= 1`: value (from parent or-asgn) is a literal → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo ||= 1
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_and_asgn_literal() {
        // `Foo &&= 1`: and_asgn is NOT special-cased → value-less → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo &&= 1
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_op_asgn_literal() {
        // `Foo += 1`: op_asgn is NOT special-cased → value-less → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo += 1
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn allows_or_asgn_method_call() {
        // `Foo ||= compute`: value is a no-receiver send → allowed.
        test::<ConstantName>().expect_no_offenses("Foo ||= compute\n");
    }

    #[test]
    fn allows_or_asgn_const() {
        test::<ConstantName>().expect_no_offenses("Foo ||= OtherConst\n");
    }

    // --- method call on RHS ---

    #[test]
    fn allows_no_receiver_method_call() {
        test::<ConstantName>().expect_no_offenses("Computed = compute_value\n");
    }

    #[test]
    fn allows_non_literal_receiver_method_call() {
        test::<ConstantName>().expect_no_offenses("Aliased = obj.call\n");
    }

    #[test]
    fn allows_const_receiver_new() {
        test::<ConstantName>().expect_no_offenses("OtherNew = Foo.new\n");
    }

    #[test]
    fn flags_safe_navigation_method_call() {
        // `Foo = bar&.baz`: csend RHS is NOT a send → not allowed → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = bar&.baz
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_literal_receiver_method_call() {
        // `Foo = "str".freeze`: literal receiver → not allowed → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = "str".freeze
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_parenthesized_literal_receiver() {
        // `Foo = ("str").freeze`: `(send (begin literal?) ...)` → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = ("str").freeze
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_numeric_receiver_method_call() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = 5.to_s
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_array_receiver_method_call() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = [1, 2].first
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn allows_method_chain() {
        // `Foo = a.b.c`: receiver `a.b` is a send (not literal) → allowed.
        test::<ConstantName>().expect_no_offenses("Foo = a.b.c\n");
    }

    // --- parenthesized value (begin) is NOT unwrapped ---

    #[test]
    fn flags_parenthesized_method_call() {
        // `Foo = (compute)`: value is `begin`, not send → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = (compute)
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_parenthesized_literal() {
        // `Foo = (5)`: value is `begin`, not the int → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = (5)
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    // --- Class.new / Struct.new ---

    #[test]
    fn allows_class_new() {
        test::<ConstantName>().expect_no_offenses("MyClass = Class.new\n");
    }

    #[test]
    fn allows_struct_new() {
        test::<ConstantName>().expect_no_offenses("MyStruct = Struct.new(:a)\n");
    }

    #[test]
    fn allows_cbase_class_new() {
        test::<ConstantName>().expect_no_offenses("MyClass = ::Class.new\n");
    }

    #[test]
    fn allows_class_new_with_block() {
        // Value type is `block` → allowed regardless of the call.
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            MyClass = Class.new do
            end
        "#});
    }

    // --- block-family RHS ---

    #[test]
    fn allows_block_value() {
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            Klass = whatever do
            end
        "#});
    }

    #[test]
    fn allows_itblock_value() {
        // `Foo = bar { it }`: under Ruby 3.3.5 this is a plain block → allowed.
        test::<ConstantName>().expect_no_offenses("Foo = bar { it }\n");
    }

    #[test]
    fn allows_braced_block_value() {
        test::<ConstantName>().expect_no_offenses("Foo = bar { |x| x }\n");
    }

    #[test]
    fn flags_numblock_value() {
        // `Foo = bar { _1 }`: numblock is NOT in the block/const/casgn allow
        // set → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Foo = bar { _1 }
            ^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    // --- const / casgn RHS ---

    #[test]
    fn allows_const_value() {
        test::<ConstantName>().expect_no_offenses("Aliased = SomeClass\n");
    }

    #[test]
    fn allows_scoped_const_value() {
        test::<ConstantName>().expect_no_offenses("Aliased = ::Some::Const\n");
    }

    #[test]
    fn allows_casgn_value() {
        // `Outer = Inner = 1`: value is a `casgn` → allowed (the inner `Inner`
        // is a separate casgn that is itself checked).
        test::<ConstantName>().expect_no_offenses("Outer = INNER = 1\n");
    }

    // --- conditional expression on RHS ---

    #[test]
    fn allows_ternary_with_const() {
        test::<ConstantName>().expect_no_offenses("Cond = x ? FOO : 2\n");
    }

    #[test]
    fn allows_if_with_const_in_then() {
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            Cond = if x then FOO else 2 end
        "#});
    }

    #[test]
    fn allows_if_with_const_in_else() {
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            Cond = if x then 1 else FOO end
        "#});
    }

    #[test]
    fn allows_elsif_with_const() {
        // The const is in an elsif branch — `branches` recurses through elsif.
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            Cond = if x then 1 elsif y then FOO else 2 end
        "#});
    }

    #[test]
    fn flags_ternary_without_const() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = x ? 1 : 2
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_if_without_const() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = if x then 1 else 2 end
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_elsif_without_const() {
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = if x then 1 elsif y then 2 else 3 end
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_const_only_in_condition() {
        // The const is in the CONDITION, not a branch → not allowed → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = if FOO then 1 else 2 end
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_const_buried_in_branch() {
        // `else foo(FOO)`: the branch is a send, not a const; FOO buried in an
        // argument does NOT count → flagged.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = if x then 1 else foo(FOO) end
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn flags_parenthesized_nested_if() {
        // `else (if y then FOO else 2 end)`: the else is a `begin`, not a bare
        // elsif, so the walk does not recurse → no const branch found.
        test::<ConstantName>().expect_offense(indoc! {r#"
            Cond = if x then 1 else (if y then FOO else 2 end) end
            ^^^^ Use SCREAMING_SNAKE_CASE for constants.
        "#});
    }

    #[test]
    fn known_gap_else_if_not_distinguished_from_elsif() {
        // DIVERGENCE (documented): `else if` (two words) is NOT an `elsif`, so
        // RuboCop does not recurse and DOES flag this (the inner `if` is not a
        // const branch). Prism produces an identical nested-`If`-in-`else_` AST
        // for `elsif` and `else if`, so Murphy cannot distinguish them and
        // treats this as an elsif chain → finds `BAR` → no offense. This pins
        // Murphy's actual (slightly divergent) behavior; see the parity note.
        test::<ConstantName>()
            .expect_no_offenses("Cond = if x then 1 else if y then BAR else 2 end end\n");
    }

    // --- no offenses (conforming names / non-constant targets) ---

    #[test]
    fn accepts_screaming_snake_case() {
        test::<ConstantName>().expect_no_offenses("INCH_IN_CM = 2.54\n");
    }

    #[test]
    fn accepts_single_letter_with_digit() {
        test::<ConstantName>().expect_no_offenses("T1 = if x then 1 else 2 end\n");
    }

    #[test]
    fn accepts_all_caps_no_underscore() {
        test::<ConstantName>().expect_no_offenses("ABC = 1\n");
    }

    #[test]
    fn ignores_local_variable() {
        // `fooBar = 1` is a local variable (lvasgn), not a constant.
        test::<ConstantName>().expect_no_offenses("fooBar = 1\n");
    }

    #[test]
    fn ignores_constant_read() {
        // RuboCop is `on_casgn` only — bare reads of a non-conforming const
        // are not flagged.
        test::<ConstantName>().expect_no_offenses("SomeClass\nFooBar\n");
    }

    #[test]
    fn ignores_class_declaration_name() {
        // `class FooBar` is checked by ClassAndModuleCamelCase, not here.
        test::<ConstantName>().expect_no_offenses(indoc! {r#"
            class FooBar
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(ConstantName);
