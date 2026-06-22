//! `Naming/MemoizedInstanceVariableName` — a memoized method's instance
//! variable name must match the method name.
//!
//! In `def foo; @foo ||= …; end` the memoization ivar `@foo` must derive from
//! the method name `foo`. Two memoization shapes are recognised, mirroring
//! RuboCop: the `@ivar ||= …` form (`on_or_asgn`) and the
//! `return @ivar if defined?(@ivar); @ivar = …` form (`on_defined?`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/MemoizedInstanceVariableName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `on_or_asgn` + `on_defined?`. Detection parity
//!   is complete and column-for-column verified against rubocop 1.87.0. The
//!   cop is unsafe (`Safe: false`) and RuboCop's autocorrect is intentionally
//!   NOT ported — it rewrites the ivar name, which may collide with ivars
//!   already in use; Murphy emits detection-only offenses (`emit_offense` with
//!   no edit), matching the codebase convention for unsafe rewrites.
//!
//!   Definition discovery mirrors `find_definition`: the enclosing definition
//!   is a `def`, a `def self.x` (singleton; Murphy models both as `Def` with a
//!   receiver, plus `Defs`), or a `define_method`/`define_singleton_method`
//!   block whose name argument is a `sym`/`str` literal. The method name for
//!   the block form is read off the block's call first argument (NOT
//!   `cx.method_name`, which would return `define_method`).
//!
//!   `on_or_asgn` eligibility (`body == node || body.children.last == node`) is
//!   ported as a strict ONE-level check, exactly as parser-gem evaluates it:
//!     * the def body IS the `||=`, OR
//!     * the def body is a `begin` whose LAST child is the `||=`, OR
//!     * the def body is a block whose body IS the `||=`.
//!   Anything deeper (a `begin` whose last child is a block, an `if`/`case`
//!   body, a block whose body is itself a `begin`) does NOT fire — verified
//!   against rubocop, which over-fires nowhere here because the check is
//!   literally `node.children.last`, not a recursive last-leaf walk.
//!
//!   `on_defined?` matches the `defined_memoized?` begin-shape
//!   (`(begin (if (defined (ivar %1)) (return (ivar %1)) nil?) ... (ivasgn %1 _))`)
//!   with the ivar UNIFIED across all three positions: the `defined?` operand,
//!   the `return`ed ivar, and the final `@ivar = …` write all share one name,
//!   else the shape does not match and nothing fires. When it fires, three
//!   offenses are emitted (the `defined?` ivar, the `return` ivar, and the
//!   final ivasgn's name range), all carrying the same message — verified
//!   columns 10-13 / 27-30 / 3-6 for `return @baz if defined?(@baz)` /
//!   `@baz = …`.
//!
//!   Matching (`matches?`): always matches (no offense) when the method is one
//!   of `INITIALIZE_METHODS` (`initialize`, `initialize_clone`,
//!   `initialize_copy`, `initialize_dup`). Otherwise the method name is
//!   normalised by deleting EVERY `!`, `?`, `=` (RuboCop's `delete('!?=')`,
//!   not just suffix-strip) and the ivar's leading `@` is stripped, then the
//!   stripped ivar name must appear in `variable_name_candidates`:
//!     * disallowed (default): [method, method.delete_prefix('_')]
//!     * required:            [_method, method (only if it already starts '_')]
//!     * optional:            [method, _method, method.delete_prefix('_')]
//!
//!   Message: `required` style with an ivar NOT starting `_` uses the
//!   UNDERSCORE_REQUIRED wording ("does not start with `_`"); otherwise the
//!   MSG wording ("does not match method name"). `suggested_var` deletes
//!   `!?=` and prefixes `_` under `required`.
//!
//!   The `var` printed in the message keeps its `@` sigil (RuboCop prints
//!   `@foo`), as do the candidate-suggestion `@…` forms.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

/// Methods exempt from the check (RuboCop `INITIALIZE_METHODS`).
const INITIALIZE_METHODS: [&str; 4] = [
    "initialize",
    "initialize_clone",
    "initialize_copy",
    "initialize_dup",
];

/// Dynamic method-definition selectors (RuboCop `DYNAMIC_DEFINE_METHODS`).
const DYNAMIC_DEFINE_METHODS: [&str; 2] = ["define_method", "define_singleton_method"];

#[derive(Default)]
pub struct MemoizedInstanceVariableName;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum LeadingUnderscoresStyle {
    /// The ivar must match the method name with no leading underscore.
    #[option(value = "disallowed")]
    Disallowed,
    /// The ivar must be prefixed with an underscore.
    #[option(value = "required")]
    Required,
    /// Either an underscore-prefixed or bare ivar is accepted.
    #[option(value = "optional")]
    Optional,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyleForLeadingUnderscores",
        default = "disallowed",
        description = "Whether memoized ivars must/may be prefixed with an underscore."
    )]
    pub style: LeadingUnderscoresStyle,
}

#[cop(
    name = "Naming/MemoizedInstanceVariableName",
    description = "Memoized method name should match memo instance variable name.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MemoizedInstanceVariableName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            let Some((method_name, body)) = definition(id, cx) else {
                continue;
            };
            let Some(body) = body.get() else {
                continue;
            };

            check_or_asgn(method_name, body, &opts, cx);
            check_defined(method_name, body, &opts, cx);
        }
    }
}

/// If `id` is a method definition (`def`, `def self.x`, or a
/// `define_method`/`define_singleton_method` block with a literal name),
/// return `(method_name, body)`. Mirrors RuboCop's `method_definition?`.
fn definition<'a>(id: NodeId, cx: &Cx<'a>) -> Option<(&'a str, OptNodeId)> {
    match *cx.kind(id) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => {
            let name = cx.method_name(id)?;
            Some((name, cx.def_body(id)))
        }
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            let call = cx.block_call(id).get()?;
            let selector = cx.method_name(call)?;
            if !DYNAMIC_DEFINE_METHODS.contains(&selector) {
                return None;
            }
            // The defined method name is the first argument, which must be a
            // `sym` or `str` literal.
            let arg = cx.call_arguments(call).first().copied()?;
            let name = literal_name(arg, cx)?;
            Some((name, cx.block_body(id)))
        }
        _ => None,
    }
}

/// Extract the string value of a `sym`/`str` literal node, or `None`.
fn literal_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
        NodeKind::Str(sid) => Some(cx.string_str(sid)),
        _ => None,
    }
}

/// `on_or_asgn`: an `@ivar ||= …` that is the eligible (one-level-last)
/// statement of the method body.
fn check_or_asgn(method_name: &str, body: NodeId, opts: &Options, cx: &Cx<'_>) {
    let Some(or_asgn) = eligible_or_asgn(body, cx) else {
        return;
    };
    let NodeKind::OrAsgn { target, .. } = *cx.kind(or_asgn) else {
        return;
    };
    let NodeKind::Ivasgn { name, .. } = *cx.kind(target) else {
        return;
    };
    let ivar_name = cx.symbol_str(name);
    if matches(method_name, ivar_name, opts.style) {
        return;
    }
    emit(cx.range(target), method_name, ivar_name, opts.style, cx);
}

/// Find the `||=` node that is eligible per RuboCop's
/// `body == node || body.children.last == node` — a STRICT one-level check.
fn eligible_or_asgn(body: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    // body == node
    if is_ivar_or_asgn(body, cx) {
        return Some(body);
    }
    // body is a `begin` whose last child is the `||=`
    if let NodeKind::Begin(list) = *cx.kind(body) {
        let last = *cx.list(list).last()?;
        if is_ivar_or_asgn(last, cx) {
            return Some(last);
        }
        return None;
    }
    // body is a block whose body IS the `||=`
    if matches!(
        *cx.kind(body),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    ) {
        // A `define_method`/`define_singleton_method` block is itself a
        // definition: the `||=` belongs to the INNER method, not this outer
        // one, and is checked when we visit the block directly. Treating it as
        // transparent would attribute the memoization to the outer method too,
        // double-firing where RuboCop's `find_definition` (nearest enclosing
        // definition) fires once.
        if let Some(call) = cx.block_call(body).get()
            && cx
                .method_name(call)
                .is_some_and(|m| DYNAMIC_DEFINE_METHODS.contains(&m))
        {
            return None;
        }
        let inner = cx.block_body(body).get()?;
        if is_ivar_or_asgn(inner, cx) {
            return Some(inner);
        }
    }
    None
}

/// True iff `id` is an `||=` whose target is an instance-variable write.
fn is_ivar_or_asgn(id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::OrAsgn { target, .. } = *cx.kind(id) else {
        return false;
    };
    matches!(*cx.kind(target), NodeKind::Ivasgn { .. })
}

/// `on_defined?`: the `return @ivar if defined?(@ivar); …; @ivar = …` memo
/// shape (RuboCop `defined_memoized?`), emitting three offenses on a mismatch.
fn check_defined(method_name: &str, body: NodeId, opts: &Options, cx: &Cx<'_>) {
    let NodeKind::Begin(list) = *cx.kind(body) else {
        return;
    };
    let stmts = cx.list(list);
    // Need at least the guard `if` and the final `ivasgn`.
    let [first, .., last] = stmts else {
        return;
    };

    // First statement: `if (defined (ivar %1)) (return (ivar %1)) nil`.
    let NodeKind::If {
        cond, then_, else_, ..
    } = *cx.kind(*first)
    else {
        return;
    };
    if else_.get().is_some() {
        return;
    }
    let NodeKind::Defined(defined_arg) = *cx.kind(cond) else {
        return;
    };
    let NodeKind::Ivar(defined_sym) = *cx.kind(defined_arg) else {
        return;
    };
    let then_ = then_.get().filter(|t| is_return_ivar(*t, cx));
    let Some(then_node) = then_ else {
        return;
    };
    let NodeKind::Return(ret_val) = *cx.kind(then_node) else {
        return;
    };
    let return_ivar = ret_val.get().filter(|r| matches!(*cx.kind(*r), NodeKind::Ivar(_)));
    let Some(return_ivar) = return_ivar else {
        return;
    };

    // Last statement: `(ivasgn %1 _)` with a value.
    let NodeKind::Ivasgn {
        name: assign_name,
        value,
    } = *cx.kind(*last)
    else {
        return;
    };
    if value.get().is_none() {
        return;
    }

    // Unify the ivar name across all three positions (RuboCop's `%1`).
    let defined_name = cx.symbol_str(defined_sym);
    let NodeKind::Ivar(return_sym) = *cx.kind(return_ivar) else {
        return;
    };
    if cx.symbol_str(return_sym) != defined_name {
        return;
    }
    if cx.symbol_str(assign_name) != defined_name {
        return;
    }

    if matches(method_name, defined_name, opts.style) {
        return;
    }

    // Three offenses: defined? ivar, return ivar, and the ivasgn name range.
    emit(cx.range(defined_arg), method_name, defined_name, opts.style, cx);
    emit(cx.range(return_ivar), method_name, defined_name, opts.style, cx);
    emit(
        ivasgn_name_range(*last, defined_name, cx),
        method_name,
        defined_name,
        opts.style,
        cx,
    );
}

/// `(return (ivar …))` shape check.
fn is_return_ivar(id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Return(val) = *cx.kind(id) else {
        return false;
    };
    val.get()
        .is_some_and(|v| matches!(*cx.kind(v), NodeKind::Ivar(_)))
}

/// The name range of an `@ivar = …` write (the `@ivar` part), located by
/// searching the node's source. Murphy leaves `loc.name == ZERO` on ivasgn.
fn ivasgn_name_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    match src.find(name) {
        Some(off) => Range {
            start: expr.start + off as u32,
            end: expr.start + off as u32 + name.len() as u32,
        },
        None => Range {
            start: expr.start,
            end: expr.start + name.len() as u32,
        },
    }
}

/// RuboCop `matches?`: true (no offense) when the method is an initialize
/// method or the ivar name is a valid candidate for the method name.
fn matches(method_name: &str, ivar_name: &str, style: LeadingUnderscoresStyle) -> bool {
    if INITIALIZE_METHODS.contains(&method_name) {
        return true;
    }
    let normalised = normalise_method(method_name);
    let variable_name = ivar_name.strip_prefix('@').unwrap_or(ivar_name);
    candidate_match(&normalised, variable_name, style)
}

/// RuboCop `method_name.to_s.delete('!?=')` — remove every `!`, `?`, `=`.
fn normalise_method(method_name: &str) -> String {
    method_name
        .chars()
        .filter(|c| !matches!(c, '!' | '?' | '='))
        .collect()
}

/// True iff `variable_name` is one of `variable_name_candidates(method_name)`.
fn candidate_match(method_name: &str, variable_name: &str, style: LeadingUnderscoresStyle) -> bool {
    let no_underscore = method_name.strip_prefix('_').unwrap_or(method_name);
    match style {
        LeadingUnderscoresStyle::Required => {
            variable_name == format!("_{method_name}")
                || (method_name.starts_with('_') && variable_name == method_name)
        }
        LeadingUnderscoresStyle::Disallowed => {
            variable_name == method_name || variable_name == no_underscore
        }
        LeadingUnderscoresStyle::Optional => {
            variable_name == method_name
                || variable_name == format!("_{method_name}")
                || variable_name == no_underscore
        }
    }
}

/// RuboCop `suggested_var`: normalised method name, `_`-prefixed under
/// `required`.
fn suggested_var(method_name: &str, style: LeadingUnderscoresStyle) -> String {
    let suggestion = normalise_method(method_name);
    if style == LeadingUnderscoresStyle::Required {
        format!("_{suggestion}")
    } else {
        suggestion
    }
}

/// Emit one offense, picking the message wording per RuboCop `message`.
fn emit(
    range: Range,
    method_name: &str,
    ivar_name: &str,
    style: LeadingUnderscoresStyle,
    cx: &Cx<'_>,
) {
    let variable_name = ivar_name.strip_prefix('@').unwrap_or(ivar_name);
    let suggested = suggested_var(method_name, style);
    let message = if style == LeadingUnderscoresStyle::Required && !variable_name.starts_with('_') {
        format!(
            "Memoized variable `{ivar_name}` does not start with `_`. Use `@{suggested}` instead."
        )
    } else {
        format!(
            "Memoized variable `{ivar_name}` does not match method name `{method_name}`. Use `@{suggested}` instead."
        )
    };
    cx.emit_offense(range, &message, None);
}

#[cfg(test)]
mod tests {
    use super::{LeadingUnderscoresStyle, MemoizedInstanceVariableName, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- or-asgn path (carets from rubocop 1.87.0) ---

    #[test]
    fn flags_mismatched_ivar_in_def() {
        // rubocop: L2 c3-12 (`@something`)
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def foo
              @something ||= calculate_expensive_thing
              ^^^^^^^^^^ Memoized variable `@something` does not match method name `foo`. Use `@foo` instead.
            end
        "#});
    }

    #[test]
    fn accepts_matching_ivar() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              @foo ||= compute
            end
        "#});
    }

    #[test]
    fn flags_when_or_asgn_is_last_statement() {
        // rubocop: L3 c3-8 (`@other`)
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def baz
              do_thing
              @other ||= compute
              ^^^^^^ Memoized variable `@other` does not match method name `baz`. Use `@baz` instead.
            end
        "#});
    }

    #[test]
    fn ignores_or_asgn_that_is_not_last_statement() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def qux
              @first ||= compute
              do_thing
            end
        "#});
    }

    #[test]
    fn flags_or_asgn_as_sole_block_body() {
        // `[1].each do; @x ||= y; end` where the block is the whole def body.
        // rubocop: L3 c5-6 (`@x`)
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def foo
              [1].each do
                @x ||= y
                ^^ Memoized variable `@x` does not match method name `foo`. Use `@foo` instead.
              end
            end
        "#});
    }

    #[test]
    fn ignores_or_asgn_in_block_when_block_is_not_last() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              bar
              [1].each do
                @x ||= y
              end
            end
        "#});
    }

    #[test]
    fn ignores_or_asgn_inside_if_body() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              if c
                @x ||= y
              end
            end
        "#});
    }

    #[test]
    fn ignores_or_asgn_not_last_inside_block() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              [1].each do
                bar
                @x ||= y
              end
            end
        "#});
    }

    #[test]
    fn accepts_begin_body() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              @foo ||= begin
                calculate
              end
            end
        "#});
    }

    // --- method-name normalization ---

    #[test]
    fn accepts_predicate_method() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo?
              @foo ||= compute
            end
        "#});
    }

    #[test]
    fn accepts_bang_method() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo!
              @foo ||= compute
            end
        "#});
    }

    #[test]
    fn accepts_setter_method() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo=(x)
              @foo ||= x
            end
        "#});
    }

    // --- leading-underscore (disallowed default) ---

    #[test]
    fn accepts_underscore_method_with_bare_ivar() {
        // disallowed: candidates [_foo, foo]; @foo -> foo matches.
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def _foo
              @foo ||= compute
            end
        "#});
    }

    #[test]
    fn flags_underscore_ivar_under_disallowed() {
        // disallowed: candidates [foo, foo]; @_foo -> _foo not a candidate.
        // rubocop: L2 c3-7 (`@_foo`)
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def foo
              @_foo ||= compute
              ^^^^^ Memoized variable `@_foo` does not match method name `foo`. Use `@foo` instead.
            end
        "#});
    }

    // --- initialize exemption ---

    #[test]
    fn ignores_initialize() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def initialize
              @anything ||= compute
            end
        "#});
    }

    // --- singleton def ---

    #[test]
    fn flags_singleton_def() {
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def self.foo
              @bar ||= compute
              ^^^^ Memoized variable `@bar` does not match method name `foo`. Use `@foo` instead.
            end
        "#});
    }

    // --- define_method / define_singleton_method ---

    #[test]
    fn flags_define_method() {
        // rubocop: L2 c3-12 (`@something`)
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            define_method(:foo) do
              @something ||= compute
              ^^^^^^^^^^ Memoized variable `@something` does not match method name `foo`. Use `@foo` instead.
            end
        "#});
    }

    #[test]
    fn flags_define_singleton_method() {
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            define_singleton_method(:bar) do
              @other ||= compute
              ^^^^^^ Memoized variable `@other` does not match method name `bar`. Use `@bar` instead.
            end
        "#});
    }

    #[test]
    fn flags_define_method_with_string_name() {
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            define_method("baz") do
              @wrong ||= compute
              ^^^^^^ Memoized variable `@wrong` does not match method name `baz`. Use `@baz` instead.
            end
        "#});
    }

    #[test]
    fn fires_once_for_define_method_nested_in_def() {
        // The `||=` belongs to the inner `foo`, not the outer `outer`. RuboCop
        // (`find_definition` → nearest enclosing definition) fires exactly one
        // offense, attributed to `foo`. Verified against rubocop 1.87.0.
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def outer
              define_method(:foo) do
                @something ||= compute
                ^^^^^^^^^^ Memoized variable `@something` does not match method name `foo`. Use `@foo` instead.
              end
            end
        "#});
    }

    // --- defined? path (three offenses) ---

    #[test]
    fn flags_defined_memoization() {
        // rubocop emits three offenses: defined? ivar (L1 c25-28),
        // return ivar (L1 c10-13), and the ivasgn name (L2 c3-6).
        test::<MemoizedInstanceVariableName>().expect_offense(indoc! {r#"
            def bar
              return @baz if defined?(@baz)
                     ^^^^ Memoized variable `@baz` does not match method name `bar`. Use `@bar` instead.
                                      ^^^^ Memoized variable `@baz` does not match method name `bar`. Use `@bar` instead.
              @baz = compute
              ^^^^ Memoized variable `@baz` does not match method name `bar`. Use `@bar` instead.
            end
        "#});
    }

    #[test]
    fn accepts_defined_memoization_with_matching_ivar() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def bar
              return @bar if defined?(@bar)
              @bar = compute
            end
        "#});
    }

    #[test]
    fn ignores_defined_memoization_with_mismatched_ivars() {
        // defined?(@a)/return @a but assign @b — the %1 unification fails,
        // so the shape does not match and nothing fires.
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              return @a if defined?(@a)
              @b = compute
            end
        "#});
    }

    // --- required style ---

    #[test]
    fn required_style_flags_bare_ivar() {
        // rubocop: L2 c3-6 with UNDERSCORE_REQUIRED wording.
        test::<MemoizedInstanceVariableName>()
            .with_options(&Options {
                style: LeadingUnderscoresStyle::Required,
            })
            .expect_offense(indoc! {r#"
                def foo
                  @foo ||= compute
                  ^^^^ Memoized variable `@foo` does not start with `_`. Use `@_foo` instead.
                end
            "#});
    }

    #[test]
    fn required_style_accepts_underscore_ivar() {
        test::<MemoizedInstanceVariableName>()
            .with_options(&Options {
                style: LeadingUnderscoresStyle::Required,
            })
            .expect_no_offenses(indoc! {r#"
                def bar
                  @_bar ||= compute
                end
            "#});
    }

    // --- optional style ---

    #[test]
    fn optional_style_accepts_both() {
        test::<MemoizedInstanceVariableName>()
            .with_options(&Options {
                style: LeadingUnderscoresStyle::Optional,
            })
            .expect_no_offenses(indoc! {r#"
                def foo
                  @foo ||= compute
                end
            "#});
    }

    #[test]
    fn optional_style_accepts_underscore() {
        test::<MemoizedInstanceVariableName>()
            .with_options(&Options {
                style: LeadingUnderscoresStyle::Optional,
            })
            .expect_no_offenses(indoc! {r#"
                def bar
                  @_bar ||= compute
                end
            "#});
    }

    // --- non-memoization ivasgn ---

    #[test]
    fn ignores_plain_ivar_assignment() {
        test::<MemoizedInstanceVariableName>().expect_no_offenses(indoc! {r#"
            def foo
              @bar = compute
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(MemoizedInstanceVariableName);
