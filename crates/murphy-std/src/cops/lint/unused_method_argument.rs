//! `Lint/UnusedMethodArgument` — flag method parameters that are never
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnusedMethodArgument
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-qaio
//! notes: >
//!   Known gaps remain around option parity, message text, and autocorrect behavior.
//! ```
//!
//! read inside the method body.
//!
//! ## Defaults that mirror RuboCop
//!
//! - **`IgnoreNotImplementedMethods`** (default true): skip the cop when
//!   the body is a single statement (or a `Begin` whose last statement
//!   is) a `raise <NotImplementedException>` (or `.new(...)`), where the
//!   exception class name is in `NotImplementedExceptions`
//!   (default `["NotImplementedError"]`). `fail` is treated identically.
//!   Disable via `ignore_not_implemented_methods = false` to opt back
//!   into reporting on those methods.
//! - **`block_argument_with_yield`**: when the body uses `yield`, the
//!   `&blk` argument is implicitly used; do not flag it.
//!
//! ## Known v1 limitation: option overrides not wired through `Cx`
//!
//! Both `ignore_not_implemented_methods` and `not_implemented_exceptions`
//! are exported via `#[derive(CopOptions)]` so the host validates the
//! schema, but runtime reads still come from `Options::default()`.
//! `murphy-9cr.9` will route `[cops.rules."Lint/UnusedMethodArgument"]`
//! overrides through `Cx`; until then setting these keys in
//! `murphy.toml` has no effect at dispatch time. See
//! `references/options.md` in the port-rubocop-cop skill for the same
//! limitation in `RSpec/ExampleLength`.
//!
//! ## Autocorrect
//!
//! - Positional / optional / rest / kwrest args: prefix the name with
//!   `_` (idempotent because `_` names are skipped on re-run).
//! - **Kwarg / Kwoptarg**: no autocorrect — renaming a keyword argument
//!   breaks every caller using the keyword.
//! - **Blockarg**: remove the whole `&blk` (and the preceding comma if
//!   any), matching RuboCop's blockarg autocorrect.

use std::collections::HashSet;

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct UnusedMethodArgument;

/// Cop options for [`UnusedMethodArgument`]. v1: options are read from
/// `Default` at dispatch time (`murphy-9cr.9` will wire live overrides).
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
        let NodeKind::Def { args, body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body) = body.get() else {
            return;
        };

        let opts = Options::default();
        if opts.ignore_not_implemented_methods
            && is_not_implemented_body(cx, body, &opts.not_implemented_exceptions)
        {
            return;
        }

        let has_yield = body_contains_yield(cx, body);
        let reads = lvar_reads(cx, body);

        let NodeKind::Args(list) = *cx.kind(args) else {
            return;
        };
        let params: &[NodeId] = cx.list(list);
        for (i, &param) in params.iter().enumerate() {
            let param_kind = *cx.kind(param);
            let Some((name, range)) = param_name_and_range(cx, param) else {
                continue;
            };
            let name_str = cx.symbol_str(name);
            if name_str.is_empty() || name_str.starts_with('_') || reads.contains(name_str) {
                continue;
            }
            // `&blk` on a method that yields is implicitly used.
            if has_yield && matches!(param_kind, NodeKind::Blockarg(_)) {
                continue;
            }
            cx.emit_offense(range, "Unused method argument", None);
            emit_autocorrect(cx, param, &param_kind, range, params, i);
        }
    }
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

fn lvar_reads<'a>(cx: &Cx<'a>, body: NodeId) -> HashSet<&'a str> {
    std::iter::once(body)
        .chain(cx.descendants(body))
        .filter_map(|id| match *cx.kind(id) {
            NodeKind::Lvar(s) => Some(cx.symbol_str(s)),
            _ => None,
        })
        .collect()
}

/// Whether `body` (a method body) contains a `yield` that would
/// dispatch to *this* method's block. `yield` inside a nested method
/// definition (`def …` or `def self.…`, both encoded as `Def` with a
/// possible `receiver`) belongs to that inner method's block, not the
/// outer's, so the walk stops at those boundaries. Blocks / lambdas /
/// class / module bodies do *not* break `yield` scope, so the walk
/// descends into them.
fn body_contains_yield(cx: &Cx<'_>, body: NodeId) -> bool {
    let mut stack: Vec<NodeId> = vec![body];
    while let Some(id) = stack.pop() {
        match *cx.kind(id) {
            NodeKind::Yield(_) => return true,
            // Skip every nested method definition — that yield belongs
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

/// Whether `body` consists of nothing but a single `raise` / `fail`
/// call whose first argument is `Const(name)` or `Const(name).new(...)`
/// with `name` in `exceptions`. A multi-statement body like
/// `do_something; raise NotImplementedError` is *not* matched —
/// `do_something` could legitimately use the method's arguments, and a
/// trailing exception should not silence the cop on the whole method.
fn is_not_implemented_body(cx: &Cx<'_>, body: NodeId, exceptions: &[String]) -> bool {
    let target = match *cx.kind(body) {
        // `Begin` with a single child is the parser sometimes wrapping a
        // lone statement (`def foo(_); raise X; end` → `Begin([Send])`).
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
    let arg_ids = cx.list(args);
    let Some(&first_arg) = arg_ids.first() else {
        return false;
    };
    exception_const_matches(cx, first_arg, exceptions)
}

/// Match `<Const>` or `<Const>.new(...)` against the configured exception
/// class names. The lookup is by leaf name only — `::NotImplementedError`,
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
    use super::UnusedMethodArgument;
    use murphy_plugin_api::{
        Range,
        test_support::{indoc, run_cop_with_edits, test},
    };

    #[test]
    fn flags_unused_method_arguments() {
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
            def call(used, unused, _ignored)
                           ^^^^^^ Unused method argument
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
                        ^ Unused method argument
                  raise ArgumentError
                end
            "#});
    }

    #[test]
    fn multi_statement_body_ending_in_raise_not_implemented_still_flags() {
        // Only methods whose body is *just* `raise NotImplementedError`
        // bypass the cop. A real implementation that happens to end with
        // an unimplemented-class raise should still report unused args
        // — `do_something` could legitimately consume them.
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def foo(unused)
                        ^^^^^^ Unused method argument
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
                         ^^^ Unused method argument
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
        // The nested `def inner; yield; end` has its own block scope —
        // its `yield` refers to `inner`'s block, not `outer`'s. So the
        // outer `&blk` is still unused.
        test::<UnusedMethodArgument>().expect_offense(indoc! {r#"
                def outer(&blk)
                           ^^^ Unused method argument
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
}
