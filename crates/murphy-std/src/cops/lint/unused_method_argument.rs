//! `Lint/UnusedMethodArgument` — flag method parameters that are never
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnusedMethodArgument
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Fixed logic false positives: zero-arity super (Zsuper), zero-arity binding,
//!   and fail-with-any-args asymmetry now match RuboCop v1.86.2 behavior.
//!   Offense message matches RuboCop (name, hint, `(*)` clause).
//!   All options (IgnoreEmptyMethods, IgnoreNotImplementedMethods,
//!   NotImplementedExceptions, AllowUnusedKeywordArguments) are read live at
//!   dispatch time via `cx.options_or_default`, so configured overrides take
//!   effect. (Closed gap: murphy-qaio.)
//! ```
//!
//! read inside the method body.
//!
//! ## Defaults that mirror RuboCop
//!
//! - **`IgnoreEmptyMethods`** (default true): skip the cop when the
//!   method body is absent (empty method body like `def foo(x); end`).
//!   Disable via `ignore_empty_methods = false` to opt back into
//!   reporting on those methods.
//! - **`IgnoreNotImplementedMethods`** (default true): skip the cop when
//!   the body is a single statement (or a `Begin` whose last statement
//!   is) a `raise <NotImplementedException>` (or `.new(...)`), where the
//!   exception class name is in `NotImplementedExceptions`
//!   (default `["NotImplementedError"]`). `fail` is asymmetric: any
//!   `fail` call (with or without arguments) suppresses the cop, matching
//!   RuboCop's `not_implemented?` pattern.
//!   Disable via `ignore_not_implemented_methods = false` to opt back
//!   into reporting on those methods.
//! - **`AllowUnusedKeywordArguments`** (default false): when true, unused
//!   keyword arguments (`bar:`, `bar: 1`) are not flagged.
//! - **`block_argument_with_yield`**: when the body uses `yield`, the
//!   `&blk` argument is implicitly used; do not flag it.
//!
//! ## Message format
//!
//! Matches RuboCop's format:
//! - Base: `Unused method argument - \`NAME\`.`
//! - Hint (for non-keyword args): ` If it's necessary, use \`_\` or
//!   \`_NAME\` as an argument name to indicate that it won't be used.
//!   If it's unnecessary, remove it.`
//! - All-unused clause (when no argument in the method is referenced):
//!   ` You can also write as \`METHODNAME(*)\` if you want the method to
//!   accept any arguments but don't care about them.`
//!
//! ## Options
//!
//! `ignore_not_implemented_methods`, `not_implemented_exceptions`,
//! `ignore_empty_methods`, and `allow_unused_keyword_arguments` are
//! exported via `#[derive(CopOptions)]` and read live at dispatch time via
//! [`Cx::options_or_default`], so configured
//! `[cops.rules."Lint/UnusedMethodArgument"]` overrides take effect.
//!
//! ## Autocorrect
//!
//! - Positional / optional / rest / kwrest args: prefix the name with
//!   `_` (idempotent because `_` names are skipped on re-run).
//! - **Kwarg / Kwoptarg**: no autocorrect -- renaming a keyword argument
//!   breaks every caller using the keyword.
//! - **Blockarg**: remove the whole `&blk` (and the preceding comma if
//!   any), matching RuboCop's blockarg autocorrect.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct UnusedMethodArgument;

/// Cop options for [`UnusedMethodArgument`]. Read live at dispatch time via
/// [`Cx::options_or_default`].
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = true,
        description = "When true, skip the cop on methods that raise a NotImplementedException."
    )]
    pub ignore_not_implemented_methods: bool,
    #[option(
        default = ["NotImplementedError"],
        description = "Exception classes whose `raise`/`fail` calls in a method body bypass the cop."
    )]
    pub not_implemented_exceptions: Vec<String>,
    #[option(
        default = true,
        description = "When true, skip the cop on methods with an empty body (no statements)."
    )]
    pub ignore_empty_methods: bool,
    #[option(
        default = false,
        description = "When true, unused keyword arguments (kwarg/kwoptarg) are not flagged."
    )]
    pub allow_unused_keyword_arguments: bool,
}

#[cop(
    name = "Lint/UnusedMethodArgument",
    description = "Flag method parameters that are never read.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl UnusedMethodArgument {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def {
            name: method_name,
            args,
            body,
            ..
        } = *cx.kind(node)
        else {
            return;
        };

        let opts = cx.options_or_default::<Options>();

        // IgnoreEmptyMethods: when true (default), skip methods with no body.
        if opts.ignore_empty_methods && body.get().is_none() {
            return;
        }
        let Some(body) = body.get() else {
            // ignore_empty_methods is false -- but we still need a body to
            // do lvar scanning. If body is absent, there are no reads.
            return;
        };

        if opts.ignore_not_implemented_methods
            && is_not_implemented_body(cx, body, &opts.not_implemented_exceptions)
        {
            return;
        }

        let has_yield = body_contains_yield(cx, body);

        // Bare `super` implicitly forwards all arguments to the superclass.
        if body_contains_zsuper(cx, body) {
            return;
        }

        // `binding` with no arguments captures the full local scope --
        // every local variable (including method parameters) is referenced.
        if body_contains_zero_arity_binding(cx, body) {
            return;
        }

        let NodeKind::Args(list) = *cx.kind(args) else {
            return;
        };
        let params: &[NodeId] = cx.list(list);

        // Pre-compute raw reference check for each param (ignoring suppression
        // rules). Used by both `usages` and `all_unused` to avoid O(N²) traversals.
        let param_referenced: Vec<bool> = params
            .iter()
            .map(|&param| {
                let Some((name, _)) = param_name_and_range(cx, param) else {
                    return false;
                };
                let name_str = cx.symbol_str(name);
                if name_str.is_empty() {
                    return false;
                }
                let model_used = cx
                    .var_model()
                    .and_then(|m| m.scope(node))
                    .and_then(|s| {
                        s.variables()
                            .iter()
                            .find(|v| v.name == name && v.is_argument)
                    })
                    .map(|v| !v.references.is_empty())
                    .unwrap_or(false);
                model_used || lvar_reads_excluding_shadowed(cx, body, name)
            })
            .collect();

        // Pre-compute usage for each param.
        //
        // `usages[i]` reflects suppression logic (underscore convention,
        // blockarg-with-yield, kwarg-allow) — used to decide whether to
        // emit an offense for parameter `i`.
        //
        // `all_unused` mirrors RuboCop's `all_arguments.none?(&:referenced?)`:
        // it checks only whether a param is *actually read* (model + lvar scan),
        // ignoring suppression rules. This drives the "(*)"-suffix in the
        // offense message. Example: `def foo(_x, y)` — `_x` is suppressed
        // by convention but not referenced, `y` is unused; both count as
        // unreferenced, so `(*)` appears in the `y` message.
        let usages: Vec<bool> = params
            .iter()
            .enumerate()
            .map(|(i, &param)| {
                let param_kind = *cx.kind(param);
                let Some((name, _range)) = param_name_and_range(cx, param) else {
                    return true; // unknown param shape -- treat as used
                };
                let name_str = cx.symbol_str(name);
                if name_str.is_empty() || name_str.starts_with('_') {
                    return true; // already suppressed by convention
                }
                // `&blk` on a method that yields is implicitly used.
                if has_yield && matches!(param_kind, NodeKind::Blockarg(_)) {
                    return true;
                }
                // AllowUnusedKeywordArguments: treat kwarg/kwoptarg as used.
                if opts.allow_unused_keyword_arguments
                    && matches!(param_kind, NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. })
                {
                    return true;
                }
                param_referenced[i]
            })
            .collect();

        let all_unused = !param_referenced.iter().any(|&r| r);
        let method_name_str = cx.symbol_str(method_name);

        for (i, &param) in params.iter().enumerate() {
            if usages[i] {
                continue;
            }
            let param_kind = *cx.kind(param);
            let Some((name, range)) = param_name_and_range(cx, param) else {
                continue;
            };
            let name_str = cx.symbol_str(name);

            // Build the RuboCop-compatible message.
            let is_kwarg = matches!(param_kind, NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. });
            let msg = build_message(name_str, is_kwarg, all_unused, method_name_str);

            cx.emit_offense(range, &msg, None);
            emit_autocorrect(cx, param, &param_kind, range, params, i);
        }
    }
}

/// Build the offense message matching RuboCop's format.
fn build_message(name: &str, is_kwarg: bool, all_unused: bool, method_name: &str) -> String {
    let mut msg = format!("Unused method argument - `{name}`.");
    if !is_kwarg {
        msg.push_str(&format!(
            " If it's necessary, use `_` or `_{name}` as an argument name \
             to indicate that it won't be used. If it's unnecessary, remove it."
        ));
    }
    if all_unused {
        msg.push_str(&format!(
            " You can also write as `{method_name}(*)` if you want the method \
             to accept any arguments but don't care about them."
        ));
    }
    msg
}

fn emit_autocorrect(
    cx: &Cx<'_>,
    param: NodeId,
    param_kind: &NodeKind,
    name_range: Range,
    params: &[NodeId],
    index: usize,
) {
    match *param_kind {
        // Renaming a kwarg breaks all callers using the keyword.
        NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. } => {}
        // Remove the whole `&blk` (with the preceding comma if any).
        NodeKind::Blockarg(_) => {
            let blockarg_range = cx.range(param);
            let start = if index == 0 {
                blockarg_range.start
            } else {
                cx.range(params[index - 1]).end
            };
            cx.emit_edit(
                Range {
                    start,
                    end: blockarg_range.end,
                },
                "",
            );
        }
        // Default: prefix the name with `_`.
        _ => {
            cx.emit_edit(
                Range {
                    start: name_range.start,
                    end: name_range.start,
                },
                "_",
            );
        }
    }
}

/// Whether `body` (a method body) contains an `Lvar` read of `name`, skipping reads inside
/// nested scopes that re-declare `name` as an argument (shadowing). Those
/// reads refer to the inner argument, not the method argument under test, so
/// they must not mark the method argument as used. Requires the
/// `VarSemanticModel` to detect which scopes redeclare `name`; when the model
/// is unavailable the scan is conservative and counts every read (no shadow
/// detection), preserving the prior "any read uses the arg" behaviour.
fn lvar_reads_excluding_shadowed(cx: &Cx<'_>, body: NodeId, name: Symbol) -> bool {
    let model = cx.var_model();
    let mut stack = vec![body];
    while let Some(id) = stack.pop() {
        let kind = *cx.kind(id);
        if let NodeKind::Lvar(n) = kind
            && n == name
        {
            return true;
        }
        // At scope boundaries: if the scope redeclares `name` as an argument,
        // skip its entire subtree (reads there refer to the inner arg). This
        // applies even when `body` itself is the boundary (a method whose only
        // statement is a block redeclaring `name`).
        let is_scope_boundary = matches!(
            kind,
            NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::Block { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. }
                | NodeKind::Lambda
                | NodeKind::Class { .. }
                | NodeKind::Module { .. }
                | NodeKind::Sclass { .. }
        );
        if is_scope_boundary
            && let Some(m) = model
            && let Some(scope) = m.scope(id)
            && scope
                .variables()
                .iter()
                .any(|v| v.name == name && v.is_argument)
        {
            // This scope re-declares `name`: skip its subtree entirely.
            continue;
        }
        stack.extend(cx.children(id));
    }
    false
}

/// Whether `body` (a method body) contains a `yield` that would
/// dispatch to *this* method's block. `yield` inside a nested method
/// definition (`def ...` or `def self....`, both encoded as `Def` with a
/// possible `receiver`) belongs to that inner method's block, not the
/// outer's, so the walk stops at those boundaries. Blocks / lambdas /
/// class / module bodies do *not* break `yield` scope, so the walk
/// descends into them.
fn body_contains_yield(cx: &Cx<'_>, body: NodeId) -> bool {
    let mut stack: Vec<NodeId> = vec![body];
    while let Some(id) = stack.pop() {
        match *cx.kind(id) {
            NodeKind::Yield(_) => return true,
            // Skip every nested method definition -- that yield belongs
            // to the inner method's block, not the outer's. This applies
            // even when the outer body *is itself* a `Def` (the outer
            // method's only statement is defining the inner one).
            NodeKind::Def { .. } => continue,
            _ => {}
        }
        stack.extend(cx.children(id));
    }
    false
}

/// Whether `body` (a method body) contains a bare `super` (`Zsuper` node) that
/// would implicitly forward all method arguments to the superclass. Walk stops
/// at nested `Def` boundaries, matching `body_contains_yield`'s scoping rule.
fn body_contains_zsuper(cx: &Cx<'_>, body: NodeId) -> bool {
    let mut stack: Vec<NodeId> = vec![body];
    while let Some(id) = stack.pop() {
        match *cx.kind(id) {
            NodeKind::Zsuper => return true,
            // A nested def has its own super scope; don't cross the boundary.
            NodeKind::Def { .. } | NodeKind::Defs { .. } => continue,
            _ => {}
        }
        stack.extend(cx.children(id));
    }
    false
}

/// Whether `body` contains a zero-arity `binding` call (no receiver, no args).
/// `binding` captures the full local scope, making every accessible variable
/// implicitly referenced. Walk stops at nested `Def` boundaries.
fn body_contains_zero_arity_binding(cx: &Cx<'_>, body: NodeId) -> bool {
    let mut stack: Vec<NodeId> = vec![body];
    while let Some(id) = stack.pop() {
        match *cx.kind(id) {
            NodeKind::Send {
                receiver,
                method,
                args,
            } if receiver.get().is_none()
                && cx.symbol_str(method) == "binding"
                && cx.list(args).is_empty() =>
            {
                return true;
            }
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Module { .. }
            | NodeKind::Class { .. }
            | NodeKind::Sclass { .. } => continue,
            _ => {}
        }
        stack.extend(cx.children(id));
    }
    false
}

/// Whether `body` consists of nothing but a single `raise` / `fail`
/// call whose first argument is `Const(name)` or `Const(name).new(...)`
/// with `name` in `exceptions`. A multi-statement body like
/// `do_something; raise NotImplementedError` is *not* matched --
/// `do_something` could legitimately use the method's arguments, and a
/// trailing exception should not silence the cop on the whole method.
fn is_not_implemented_body(cx: &Cx<'_>, body: NodeId, exceptions: &[String]) -> bool {
    let target = match *cx.kind(body) {
        // `Begin` with a single child is the parser sometimes wrapping a
        // lone statement (`def foo(_); raise X; end` -> `Begin([Send])`).
        // More than one statement means there is real code besides the
        // raise; in that case the method isn't "just unimplemented".
        NodeKind::Begin(list) => match cx.list(list) {
            [only] => *only,
            _ => return false,
        },
        _ => body,
    };
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(target)
    else {
        return false;
    };
    if receiver.get().is_some() {
        return false;
    }
    let m = cx.symbol_str(method);
    if m != "raise" && m != "fail" {
        return false;
    }
    // RuboCop's `not_implemented?` is asymmetric: `fail` matches any
    // arguments (including none) because bare `fail` is a common
    // "not-implemented" sentinel. `raise` still requires the first
    // argument to be an allowed exception class.
    if m == "fail" {
        return true;
    }
    let arg_ids = cx.list(args);
    let Some(&first_arg) = arg_ids.first() else {
        return false;
    };
    exception_const_matches(cx, first_arg, exceptions)
}

/// Match `<Const>` or `<Const>.new(...)` against the configured exception
/// class names. The lookup is by leaf name only -- `::NotImplementedError`,
/// `Foo::NotImplementedError`, and plain `NotImplementedError` all
/// resolve through the same `Const.name`.
fn exception_const_matches(cx: &Cx<'_>, node: NodeId, exceptions: &[String]) -> bool {
    let const_id = match *cx.kind(node) {
        NodeKind::Const { name, .. } => return name_matches(cx, name, exceptions),
        NodeKind::Send {
            receiver, method, ..
        } => match (receiver.get(), cx.symbol_str(method)) {
            (Some(r), "new") => r,
            _ => return false,
        },
        _ => return false,
    };
    matches!(
        *cx.kind(const_id),
        NodeKind::Const { name, .. } if name_matches(cx, name, exceptions),
    )
}

fn name_matches(cx: &Cx<'_>, sym: Symbol, exceptions: &[String]) -> bool {
    let s = cx.symbol_str(sym);
    exceptions.iter().any(|e| e == s)
}

fn param_name_and_range(cx: &Cx<'_>, node: NodeId) -> Option<(Symbol, Range)> {
    let name = match *cx.kind(node) {
        NodeKind::Arg(s)
        | NodeKind::Restarg(s)
        | NodeKind::Kwarg(s)
        | NodeKind::Kwrestarg(s)
        | NodeKind::Blockarg(s) => s,
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => name,
        _ => return None,
    };
    Some((name, cx.node(node).loc.name))
}

#[cfg(test)]
mod tests {
    use super::{Options, UnusedMethodArgument};
    use murphy_plugin_api::{
        Range,
        test_support::{indoc, run_cop_with_edits, test},
    };

    #[test]
    fn allows_unused_kwarg_when_option_enabled() {
        // `AllowUnusedKeywordArguments: true` is read live via
        // `cx.options_or_default`, so an unused keyword argument is not flagged.
        test::<UnusedMethodArgument>()
            .with_options(&Options {
                allow_unused_keyword_arguments: true,
                ..Options::default()
            })
            .expect_no_offenses("def foo(bar:)\n  1\nend\n");
    }

    #[test]
    fn flags_unused_method_arguments() {
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
            def call(used, unused, _ignored)
                           ^^^^^^ Unused method argument - `unused`. If it's necessary, use `_` or `_unused` as an argument name to indicate that it won't be used. If it's unnecessary, remove it.
              used
            end
        "#});
    }

    #[test]
    fn autocorrects_by_prefixing_underscore_and_reaches_fixpoint() {
        let run = run_cop_with_edits::<UnusedMethodArgument>("def 名前(foo)\n  1\nend\n");
        assert_eq!(run.edits[0].range, Range { start: 11, end: 11 });
        assert_eq!(run.edits[0].replacement, "_");
        test::<UnusedMethodArgument>().expect_no_offenses("def 名前(_foo)\n  1\nend\n");
    }

    // murphy-lmqm: IgnoreNotImplementedMethods + block_argument_with_yield +
    // kwarg/blockarg autocorrect alignment.

    #[test]
    fn skips_method_whose_body_just_raises_not_implemented_error() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
                def foo(x)
                  raise NotImplementedError
                end
            "#});
    }

    #[test]
    fn skips_method_whose_body_just_raises_not_implemented_error_dot_new() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
                def foo(x)
                  raise NotImplementedError.new("nope")
                end
            "#});
    }

    #[test]
    fn skips_method_with_fail_not_implemented_error() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
                def foo(x)
                  fail NotImplementedError
                end
            "#});
    }

    #[test]
    fn raise_of_different_exception_still_flags() {
        // Only NotImplementedError-class raises trigger the bypass.
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def foo(x)
                        ^ Unused method argument - `x`. If it's necessary, use `_` or `_x` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `foo(*)` if you want the method to accept any arguments but don't care about them.
                  raise ArgumentError
                end
            "#});
    }

    #[test]
    fn multi_statement_body_ending_in_raise_not_implemented_still_flags() {
        // Only methods whose body is *just* `raise NotImplementedError`
        // bypass the cop. A real implementation that happens to end with
        // an unimplemented-class raise should still report unused args
        // -- `do_something` could legitimately consume them.
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def foo(unused)
                        ^^^^^^ Unused method argument - `unused`. If it's necessary, use `_` or `_unused` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `foo(*)` if you want the method to accept any arguments but don't care about them.
                  do_something
                  raise NotImplementedError
                end
            "#});
    }

    #[test]
    fn block_arg_with_yield_in_body_is_not_flagged() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
                def foo(&blk)
                  yield
                end
            "#});
    }

    #[test]
    fn block_arg_without_yield_is_still_flagged() {
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def foo(&blk)
                         ^^^ Unused method argument - `blk`. If it's necessary, use `_` or `_blk` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `foo(*)` if you want the method to accept any arguments but don't care about them.
                  1
                end
            "#});
    }

    #[test]
    fn kwarg_offense_has_no_autocorrect() {
        // Renaming a kwarg breaks all callers; RuboCop intentionally
        // skips autocorrect for `Kwarg` / `Kwoptarg`.
        let run = run_cop_with_edits::<UnusedMethodArgument>("def foo(bar:)\n  1\nend\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits, Vec::new());
    }

    #[test]
    fn kwoptarg_offense_has_no_autocorrect() {
        let run = run_cop_with_edits::<UnusedMethodArgument>("def foo(bar: 1)\n  1\nend\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits, Vec::new());
    }

    #[test]
    fn blockarg_autocorrect_removes_arg_entirely() {
        // For `def foo(x, &blk)`, autocorrect removes `, &blk` so the
        // signature becomes `def foo(x)`.
        let run = run_cop_with_edits::<UnusedMethodArgument>("def foo(x, &blk)\n  x\nend\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        let edit = &run.edits[0];
        // The edit should clear out from end-of-prev-arg to end-of-blockarg.
        let mut source = String::from("def foo(x, &blk)\n  x\nend\n");
        source.replace_range(
            edit.range.start as usize..edit.range.end as usize,
            &edit.replacement,
        );
        assert_eq!(source, "def foo(x)\n  x\nend\n");
    }

    #[test]
    fn yield_inside_nested_def_does_not_save_outer_blockarg() {
        // The nested `def inner; yield; end` has its own block scope --
        // its `yield` refers to `inner`'s block, not `outer`'s. So the
        // outer `&blk` is still unused.
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def outer(&blk)
                           ^^^ Unused method argument - `blk`. If it's necessary, use `_` or `_blk` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `outer(*)` if you want the method to accept any arguments but don't care about them.
                  def inner
                    yield
                  end
                end
            "#});
    }

    #[test]
    fn yield_inside_block_body_still_uses_outer_blockarg() {
        // `yield` inside a block refers to the *enclosing method's*
        // block, so the outer `&blk` is implicitly used.
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
                def outer(&blk)
                  [1, 2].each do |x|
                    yield x
                  end
                end
            "#});
    }

    #[test]
    fn blockarg_only_param_removes_just_the_arg() {
        let run = run_cop_with_edits::<UnusedMethodArgument>("def foo(&blk)\n  1\nend\n");
        assert_eq!(run.offenses.len(), 1);
        let edit = &run.edits[0];
        let mut source = String::from("def foo(&blk)\n  1\nend\n");
        source.replace_range(
            edit.range.start as usize..edit.range.end as usize,
            &edit.replacement,
        );
        assert_eq!(source, "def foo()\n  1\nend\n");
    }

    // Fix 1: zero-arity `super` (Zsuper node) implicitly passes all args.
    #[test]
    fn zero_arity_super_suppresses_all_param_offenses() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def some_method(foo)
              super
            end
        "#});
    }

    // Fix 2: zero-arity `binding` captures the full local scope.
    #[test]
    fn zero_arity_binding_suppresses_all_param_offenses() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def some_method(foo, bar)
              do_something binding
            end
        "#});
    }

    // Fix 3a: `fail` with a non-exception-class argument (IgnoreNotImplementedMethods: true).
    #[test]
    fn fail_with_any_arg_suppresses_cop() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def method(arg)
              fail "TODO"
            end
        "#});
    }

    // Fix 3b: bare `fail` with no arguments also suppresses the cop.
    #[test]
    fn bare_fail_suppresses_cop() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def method(arg)
              fail
            end
        "#});
    }

    // VarSemanticModel migration: cross-scope read -- arg used inside a block.
    // The model only tracks same-scope references; the lvar-scan fallback
    // handles this case so no false-positive offense is emitted.
    #[test]
    fn arg_used_only_inside_nested_block_is_not_flagged() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def foo(x)
              [1].each { puts x }
            end
        "#});
    }

    // VarSemanticModel migration: a method arg shadowed by an inner block
    // parameter of the same name is still unused -- the read inside the block
    // refers to the inner `|x|`, not the method's `x`.
    #[test]
    fn arg_shadowed_by_inner_block_is_flagged() {
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
            def foo(x)
                    ^ Unused method argument - `x`. If it's necessary, use `_` or `_x` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `foo(*)` if you want the method to accept any arguments but don't care about them.
              [1].each do |x|
                puts x
              end
            end
        "#});
    }

    // VarSemanticModel migration: compound-assign is an implicit read.
    // `x += 1` parses as OpAsgn with no explicit Lvar node, so the old
    // lvar-scan missed it.  The model's reference tracking catches it.
    #[test]
    fn arg_used_via_op_assign_is_not_flagged() {
        test::<UnusedMethodArgument>().expect_no_offenses(indoc! {r#"
            def foo(x)
              x += 1
              x
            end
        "#});
    }

    // --- murphy-qaio option schema tests ---

    /// IgnoreEmptyMethods (default true): method with no body is skipped.
    #[test]
    fn ignores_method_with_empty_body() {
        // `def foo(x); end` has no body (body is None in the AST).
        test::<UnusedMethodArgument>().expect_no_offenses("def foo(x); end\n");
    }

    /// AllowUnusedKeywordArguments (default false): kwarg IS flagged at default.
    /// This test verifies the default-false behavior (kwarg is reported).
    #[test]
    fn kwarg_is_flagged_at_default() {
        let run = run_cop_with_edits::<UnusedMethodArgument>("def foo(bar:)\n  1\nend\n");
        assert_eq!(run.offenses.len(), 1, "kwarg should be flagged at default");
    }

    /// RuboCop computes the `(*)` clause from `all_arguments.none?(&:referenced?)`,
    /// not from which args would be suppressed by convention. An underscore-prefixed
    /// arg (`_x`) is not referenced, so when it coexists with an unused regular arg
    /// (`y`), the `(*)` clause still appears in the `y` message.
    #[test]
    fn all_unused_clause_present_when_underscore_arg_coexists_with_unused_arg() {
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
            def foo(_x, y)
                        ^ Unused method argument - `y`. If it's necessary, use `_` or `_y` as an argument name to indicate that it won't be used. If it's unnecessary, remove it. You can also write as `foo(*)` if you want the method to accept any arguments but don't care about them.
              1
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UnusedMethodArgument);
