//! `RSpec/ExpectOutput` — use `expect { ... }.to output` instead of mutating `$stdout` / `$stderr`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExpectOutput
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_gvasgn` (assignment to `$stdout` / `$stderr`
//!   gated by `inside_example_scope?`). The offense range is the variable
//!   name (`node.loc.name`: the expression start through the end of the
//!   `$stdout` / `$stderr` token) with `Use `expect { ... }.to
//!   output(...).to_<name>` instead of mutating $<name>.` as the message.
//!   `inside_example_scope?` walks `Block` ancestors: `false` at an
//!   example group (`ExampleGroups.all` with a bare or `RSpec`
//!   receiver), `true` at an example (`Examples.all`: regular + focused
//!   + skipped + pending, bare receiver only), `Hook#example?` at a hook
//!   (`Hooks.all`, bare receiver only; each-scope means no args, a `Hash`
//!   first arg, or `:each` / `:example`), else the parent. Numbered-param
//!   (`Numblock`) ancestors never count as examples or groups, matching
//!   upstream's block-only patterns. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Gvasgn`:
//!
//! - `it 'x' do $stdout = StringIO.new end` — flagged (range `$stdout`).
//! - `it 'x' do $stderr = StringIO.new end` — flagged (range `$stderr`).
//! - `describe 'x' do before { $stdout = io } end` — each-scope hook,
//!   flagged.
//! - `describe 'x' do before(:all) { $stdout = io } end` —
//!   context-scope hook, not flagged.
//! - `describe 'x' do $stdout = io end` — group body, not flagged.
//! - `$stdout = io` at file top level — not flagged.
//! - `$other = io` in an example — other global, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; restructuring around
//! `expect { ... }.to output` needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_hook_name};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExpectOutput;

#[cop(
    name = "RSpec/ExpectOutput",
    description = "Checks for opportunities to use `expect { ... }.to output`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ExpectOutput {
    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvasgn { name, .. } = *cx.kind(node) else {
            return;
        };
        let var = cx.symbol_str(name);
        // Upstream `node.name[1..]`: strip the `$` sigil, then gate on
        // `stdout` / `stderr`.
        let short = match var {
            "$stdout" => "stdout",
            "$stderr" => "stderr",
            _ => return,
        };
        if !inside_example_scope(cx, node) {
            return;
        }
        // Upstream `node.loc.name` covers the `$stdout` token. Murphy
        // records no name loc for `Gvasgn`, so project it from the
        // expression start plus the variable spelling length.
        let expr = cx.range(node);
        let name_range = Range {
            start: expr.start,
            end: expr.start + (var.len() as u32),
        };
        cx.emit_offense(
            name_range,
            &format!(
                "Use `expect {{ ... }}.to output(...).to_{short}` \
                 instead of mutating ${short}."
            ),
            None,
        );
    }
}

/// Example selectors (`Examples` in rubocop-rspec's default config):
/// regular (`it`, `specify`, `example`, `scenario`, `its`), focused
/// (`fit`, `fspecify`, `fexample`, `fscenario`, `focus`), skipped
/// (`xit`, `xspecify`, `xexample`, `xscenario`, `skip`) and pending
/// (`pending`).
fn is_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

/// `true` when the assignment sits where an `expect { ... }.to output`
/// replacement applies: the nearest enclosing example/group/hook `Block`
/// is an example, or an each-scope hook.
///
/// Mirrors upstream `inside_example_scope?` (`false` at `nil` — no
/// ancestor matches here — or at an example group, `true` at an
/// example, `Hook#example?` at a hook, else the parent).
fn inside_example_scope(cx: &Cx<'_>, node: NodeId) -> bool {
    for ancestor in cx.ancestors(node) {
        let NodeKind::Block { call, .. } = *cx.kind(ancestor) else {
            continue;
        };
        if is_example_group_call(cx, call) {
            return false;
        }
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        let name = cx.symbol_str(method);
        if is_example_name(name) {
            return true;
        }
        if is_hook_name(name) {
            return is_each_scope_hook(cx, call);
        }
    }
    false
}

/// `true` when a hook call runs per-example: no scope argument, a `Hash`
/// first argument (metadata), or an explicit `:each` / `:example` scope.
///
/// Mirrors upstream `Hook#example?` (`scope.equal?(:each)`), where the
/// scope defaults to `:each` for a missing argument and a `Hash` first
/// argument counts as `:each`.
fn is_each_scope_hook(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return false;
    };
    let arg_ids = cx.list(args);
    let Some(first) = arg_ids.first() else {
        return true;
    };
    match *cx.kind(*first) {
        NodeKind::Hash(_) => true,
        NodeKind::Sym(sym) => matches!(cx.symbol_str(sym), "each" | "example"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ExpectOutput;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_stdout_assignment_in_example() {
        test::<ExpectOutput>().expect_offense(indoc! {r#"
                it 'x' do
                  $stdout = StringIO.new
                  ^^^^^^^ Use `expect { ... }.to output(...).to_stdout` instead of mutating $stdout.
                end
            "#});
    }

    #[test]
    fn flags_stderr_assignment_in_example() {
        test::<ExpectOutput>().expect_offense(indoc! {r#"
                it 'x' do
                  $stderr = StringIO.new
                  ^^^^^^^ Use `expect { ... }.to output(...).to_stderr` instead of mutating $stderr.
                end
            "#});
    }

    #[test]
    fn flags_assignment_in_each_scope_hook() {
        // The hook is each-scope (no args), so the assignment is in
        // example scope per `Hook#example?`.
        test::<ExpectOutput>().expect_offense(indoc! {r#"
                describe 'x' do
                  before do
                    $stdout = StringIO.new
                    ^^^^^^^ Use `expect { ... }.to output(...).to_stdout` instead of mutating $stdout.
                  end
                end
            "#});
    }

    #[test]
    fn flags_assignment_in_nested_block_in_example() {
        // Non-RSpec blocks between the assignment and the example are
        // skipped, mirroring the parent walk.
        test::<ExpectOutput>().expect_offense(indoc! {r#"
                it 'x' do
                  foo do
                    $stdout = StringIO.new
                    ^^^^^^^ Use `expect { ... }.to output(...).to_stdout` instead of mutating $stdout.
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_assignment_in_all_scope_hook() {
        test::<ExpectOutput>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  before(:all) do
                    $stdout = StringIO.new
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_assignment_in_group_body() {
        test::<ExpectOutput>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  $stdout = StringIO.new
                end
            "#});
    }

    #[test]
    fn does_not_flag_assignment_at_top_level() {
        test::<ExpectOutput>().expect_no_offenses(indoc! {r#"
                $stdout = StringIO.new
            "#});
    }

    #[test]
    fn does_not_flag_other_global_in_example() {
        test::<ExpectOutput>().expect_no_offenses(indoc! {r#"
                it 'x' do
                  $other = StringIO.new
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExpectOutput);
