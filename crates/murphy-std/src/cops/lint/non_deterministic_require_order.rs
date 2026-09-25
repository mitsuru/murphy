//! `Lint/NonDeterministicRequireOrder` — sort `Dir[...]` / `Dir.glob(...)`
//! results before requiring files.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NonDeterministicRequireOrder
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop 1.87.0 `on_block` / `on_numblock` / `on_block_pass`,
//!   `unsorted_dir_block?` / `unsorted_dir_each?` /
//!   `unsorted_dir_glob_pass?` / `unsorted_dir_each_pass?`,
//!   `loop_variable` (`(args (arg $_))` — exactly one plain block argument),
//!   `method_require?`, and `var_is_required?` (`def_node_search` over the
//!   whole block body). `Dir[...]` / `Dir.glob(...)` chained with `.each`
//!   (block, numblock `_1`, and `&method(:require/:require_relative)`
//!   block-pass forms) and direct `Dir.glob(...) { }` blocks are flagged on
//!   the full send node, matching `add_offense(node.send_node)` /
//!   `add_offense(parent_node)`.
//!
//!   Autocorrect mirrors `correct_block` / `correct_block_pass`: an `.each`
//!   form gains `.sort` (`Dir[...].each` → `Dir[...].sort.each`, applied as
//!   a surgical selector widening `each` → `sort.each` with identical
//!   output), a direct `Dir.glob(...)` block becomes
//!   `Dir.glob(...).sort.each`, and a `Dir.glob(..., &method(:require))`
//!   pass drops the trailing block-pass argument and appends
//!   `.sort.each(&method(:require))`.
//!
//!   TargetRubyVersion gating mirrors `maximum_target_ruby_version 2.7` via
//!   the registry (`maximum_target_ruby_version = "2.7"`) plus a
//!   `Cx::target_ruby_version()` runtime guard, so direct invocations
//!   (unit tests) also stay silent on Ruby 3.0+. An unset target (`None`)
//!   still runs the cop so default test contexts exercise it. `Safe: false`
//!   mirrors the upstream config (`Safe: false`, no `SafeAutoCorrect`
//!   entry, so corrections ship).
//!
//!   Deliberate deviations: (1) `sort: false` is accepted on any target —
//!   upstream only documents it for Ruby 3.0+ (where the cop never runs),
//!   but Murphy honours the keyword as explicit sort intent. (2) A numblock
//!   body requiring several numbered params (e.g. `require _1; require _2`)
//!   emits a single offense; upstream would emit one per matching argument.
//!   (3) `itblock` (`{ it }`) is not handled — upstream notes it never
//!   occurs under the 2.7 gate (Ruby 3.4+ syntax).
//! ```
//!
//! ## Matched shapes
//! - `Dir[pattern].each { |f| require f }` — Dir glob with each block
//! - `Dir.glob(pattern).each { |f| require f }` — Dir.glob with each block
//! - `Dir.glob(pattern) { |f| require f }` — Dir.glob with direct block
//! - `Dir[pattern].each { require _1 }` — numblock form
//! - `Dir[pattern].each(&method(:require))` — each with block-pass
//! - `Dir.glob(pattern, &method(:require))` — glob with block-pass
//! - `Dir[pattern].each(&method(:require_relative))` — block-pass with require_relative

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RubyVersion, cop};

#[derive(Default)]
pub struct NonDeterministicRequireOrder;

const MSG: &str = "Sort files before requiring them.";

#[cop(
    name = "Lint/NonDeterministicRequireOrder",
    description = "Flags `Dir[...]` calls that may return files in non-deterministic order.",
    default_severity = "warning",
    default_enabled = true,
    maximum_target_ruby_version = "2.7",
    safe = false,
    options = NoOptions
)]
impl NonDeterministicRequireOrder {
    /// Handle block forms (`on_block`):
    /// - `Dir[pattern].each { |f| require f }`
    /// - `Dir.glob(pattern).each { |f| require f }`
    /// - `Dir.glob(pattern) { |f| require f }`
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if version_gated(cx) {
            return;
        }
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let Some((dir_call, is_each)) = identify_dir_call(call, cx) else {
            return;
        };
        // An `.each` carrying its own arguments (a `&method(:require)`
        // block-pass) is the `on_block_pass` shape, not `on_block` — skip
        // here so it reports exactly once via `check_send`.
        if is_each && !cx.call_arguments(call).is_empty() {
            return;
        }
        if is_each {
            if !unsorted_dir_glob(&dir_call, cx) {
                return;
            }
        } else if !unsorted_dir_block(&dir_call, cx) {
            return;
        }
        // Upstream `loop_variable`: `(args (arg $_))` — exactly one plain
        // argument. Multi-argument blocks (`|file, i|`) never match there.
        let Some(var_name) = single_block_arg_name(args, cx) else {
            return;
        };
        if !body_requires_var(body_id, &var_name, cx) {
            return;
        }
        let offense = if is_each {
            each_offense_range(call, cx)
        } else {
            send_expr_range(call, cx)
        };
        cx.emit_offense(offense, MSG, None);
        correct_send(call, is_each, cx);
    }

    /// Handle numblock forms (`on_numblock`):
    /// - `Dir[pattern].each { require _1 }`
    /// - `Dir.glob(pattern) { require _1 }`
    ///
    /// `_1` bodies are plain `lvar` nodes, so the same subtree search as
    /// `check_block` applies once the numbered names are known.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if version_gated(cx) {
            return;
        }
        let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let Some((dir_call, is_each)) = identify_dir_call(send, cx) else {
            return;
        };
        if is_each && !cx.call_arguments(send).is_empty() {
            return;
        }
        if is_each {
            if !unsorted_dir_glob(&dir_call, cx) {
                return;
            }
        } else if !unsorted_dir_block(&dir_call, cx) {
            return;
        }
        // Upstream filters `node.argument_list` (`_1`..`_max_n`) by
        // `var_is_required?`; one hit suffices for a single offense here.
        let required = (1..=max_n).any(|n| body_requires_var(body_id, &format!("_{n}"), cx));
        if !required {
            return;
        }
        let offense = if is_each {
            each_offense_range(send, cx)
        } else {
            send_expr_range(send, cx)
        };
        cx.emit_offense(offense, MSG, None);
        correct_send(send, is_each, cx);
    }

    /// Handle `&method(:require)` / `&method(:require_relative)` block-pass
    /// forms (`on_block_pass`):
    /// - `Dir[pattern].each(&method(:require))`
    /// - `Dir.glob(pattern, &method(:require))`
    #[on_node(kind = "send", methods = ["glob", "each"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if version_gated(cx) {
            return;
        }
        let method = cx.method_name(node).unwrap_or("");

        if method == "each" {
            // `unsorted_dir_each_pass?`: the `.each` takes exactly the
            // block-pass argument — no more, no less.
            let args = cx.call_arguments(node);
            if args.len() != 1 || !is_method_require_block_pass(args[0], cx) {
                return;
            }
            let Some(receiver) = cx.call_receiver(node).get() else {
                return;
            };
            if !unsorted_dir_glob(&receiver, cx) {
                return;
            }
            cx.emit_offense(send_expr_range(node, cx), MSG, None);
            // Upstream `corrector.replace(node.loc.selector, 'sort.each')`.
            cx.emit_edit(cx.selector(node), "sort.each");
        } else {
            // `unsorted_dir_glob_pass?`: `Dir.glob(..., &method(:require))`.
            if !is_dir_const_receiver(node, cx) {
                return;
            }
            if !unsorted_dir_self(node, cx) {
                return;
            }
            let args = cx.call_arguments(node);
            let Some(&last) = args.last() else {
                return;
            };
            if !is_method_require_block_pass(last, cx) {
                return;
            }
            cx.emit_offense(send_expr_range(node, cx), MSG, None);
            correct_glob_pass(node, args, last, cx);
        }
    }
}

/// `maximum_target_ruby_version 2.7`: silent on Ruby 3.0+. `None` (unset —
/// unit-test contexts) still runs the cop.
fn version_gated(cx: &Cx<'_>) -> bool {
    cx.target_ruby_version()
        .is_some_and(|v| v > RubyVersion::new(2, 7))
}

/// Identify whether `call` is a direct `Dir.glob` call or an `.each`
/// wrapping a Dir call. Returns `Some((dir_call_node, is_each))` where
/// `is_each` means "the dir_call is the receiver of an `.each`".
/// Upstream matchers are `send`-only (`csend` never matches).
fn identify_dir_call(call: NodeId, cx: &Cx<'_>) -> Option<(NodeId, bool)> {
    if !matches!(*cx.kind(call), NodeKind::Send { .. }) {
        return None;
    }
    match cx.method_name(call)? {
        "each" => Some((cx.call_receiver(call).get()?, true)),
        "glob" if is_dir_const_receiver(call, cx) => Some((call, false)),
        _ => None,
    }
}

/// Check if `node` is `Dir.glob(...)` or `Dir[...]` (the `[]` or `glob`
/// methods called on the `Dir` constant), suitable for `.each` chaining.
/// Mirrors `unsorted_dir_each?` plus the documented `sort: false` intent.
fn unsorted_dir_glob(node: &NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { .. } = *cx.kind(*node) else {
        return false;
    };
    if !is_dir_const_receiver(*node, cx) {
        return false;
    }
    let args = cx.call_arguments(*node);
    if args.is_empty() {
        return false;
    }
    // Respect explicit `sort: false` intent (see module docs).
    if args
        .iter()
        .any(|&arg| cx.raw_source(cx.range(arg)).contains("sort: false"))
    {
        return false;
    }
    matches!(cx.method_name(*node), Some("[]") | Some("glob"))
}

/// Check if `node` itself is an unsorted `Dir.glob(...)` call: at least one
/// argument and no `sort: false` keyword. Mirrors the argument side of
/// `unsorted_dir_glob_pass?` (the trailing block-pass is validated
/// separately by `is_method_require_block_pass`).
fn unsorted_dir_self(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("glob") {
        return false;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return false;
    }
    !args
        .iter()
        .any(|&arg| cx.raw_source(cx.range(arg)).contains("sort: false"))
}

/// Check if `node` is a direct `Dir.glob(...)` call used with a literal
/// block (`Dir.glob(...) { |f| ... }`). Mirrors `unsorted_dir_block?`.
fn unsorted_dir_block(node: &NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { .. } = *cx.kind(*node) else {
        return false;
    };
    if !is_dir_const_receiver(*node, cx) {
        return false;
    }
    let args = cx.call_arguments(*node);
    // `glob` requires at least a pattern.
    if args.is_empty() {
        return false;
    }
    // Respect explicit `sort: false` intent (see module docs).
    if args
        .iter()
        .any(|&arg| cx.raw_source(cx.range(arg)).contains("sort: false"))
    {
        return false;
    }
    cx.method_name(*node) == Some("glob")
}

/// Check if `node` is a receiver that is the `Dir` global constant or `::Dir`.
fn is_dir_const_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_receiver(node)
        .get()
        .is_some_and(|r| cx.is_global_const(r, "Dir"))
}

/// Upstream `loop_variable`: `(args (arg $_))` — the block takes exactly
/// one plain argument; that argument's name is the loop variable.
fn single_block_arg_name(args: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let children = cx.list(list);
    if children.len() != 1 {
        return None;
    }
    match *cx.kind(children[0]) {
        NodeKind::Arg(sym) => Some(cx.symbol_str(sym).to_string()),
        _ => None,
    }
}

/// Offense range for an `.each` send carrying a literal block: the send's
/// expression range extends over the attached block, so it is capped at the
/// `each` selector — or at the call's own closing paren for `each()` with
/// explicit parentheses.
fn each_offense_range(call: NodeId, cx: &Cx<'_>) -> Range {
    let expr = cx.range(call);
    if cx.is_parenthesized(call) {
        let close = cx.loc(call).end();
        if close != Range::ZERO {
            return Range {
                start: expr.start,
                end: close.end,
            };
        }
    }
    Range {
        start: expr.start,
        end: cx.selector(call).end,
    }
}

/// The expression range of a send, capped at its own closing paren when the
/// parser extends the range past the call end (e.g. a `Dir.glob(...)`
/// send carrying a literal block).
fn send_expr_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let expr = cx.range(node);
    let close = cx.loc(node).end();
    if close != Range::ZERO {
        Range {
            start: expr.start,
            end: close.end,
        }
    } else {
        expr
    }
}

/// Upstream `correct_block`: an `.each` form gains `.sort` before the
/// selector; a direct `Dir.glob(...)` block becomes
/// `Dir.glob(...).sort.each`.
fn correct_send(call: NodeId, is_each: bool, cx: &Cx<'_>) {
    if is_each {
        // Surgical `each` → `sort.each`; output is identical to replacing
        // the whole `Dir[...].each` send with `{receiver}.sort.each`.
        cx.emit_edit(cx.selector(call), "sort.each");
    } else {
        let range = send_expr_range(call, cx);
        let src = cx.raw_source(range).to_string();
        cx.emit_edit(range, &format!("{src}.sort.each"));
    }
}

/// Upstream `correct_block_pass` for the `Dir.glob(..., &method(:require))`
/// shape: drop the trailing block-pass argument (with its comma) and append
/// `.sort.each(&method(:require))` after the call.
fn correct_glob_pass(node: NodeId, args: &[NodeId], last: NodeId, cx: &Cx<'_>) {
    let block_src = cx.raw_source(cx.range(last)).to_string();
    if args.len() >= 2 {
        let prev = args[args.len() - 2];
        cx.emit_edit(
            Range {
                start: cx.range(prev).end,
                end: cx.range(last).end,
            },
            "",
        );
    }
    let end = send_expr_range(node, cx).end;
    cx.emit_edit(
        Range { start: end, end },
        &format!(".sort.each({block_src})"),
    );
}

/// Search the body subtree for `require var_name` or
/// `require_relative var_name` — Murphy's analog of RuboCop's
/// `def_node_search :var_is_required?`, which scans the whole body
/// (including nested conditionals).
fn body_requires_var(body: NodeId, var_name: &str, cx: &Cx<'_>) -> bool {
    // Check the body directly first.
    if send_requires_var(body, var_name, cx) {
        return true;
    }
    // Also check descendants for nested requires (e.g. inside if/else branches).
    for desc in cx.descendants(body) {
        if send_requires_var(desc, var_name, cx) {
            return true;
        }
    }
    false
}

/// Check if `node` is a `require` or `require_relative` send whose first
/// argument is a local variable matching `var_name`.
fn send_requires_var(node: NodeId, var_name: &str, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    if !matches!(
        cx.method_name(node),
        Some("require") | Some("require_relative")
    ) {
        return false;
    }
    if cx.call_receiver(node).get().is_some() {
        return false;
    }
    let args = cx.call_arguments(node);
    let Some(first_arg) = args.first() else {
        return false;
    };
    match *cx.kind(*first_arg) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == var_name,
        _ => false,
    }
}

/// Check if `arg` is a `&method(:require)` or `&method(:require_relative)`
/// block-pass — Murphy's analog of RuboCop's `method_require?`.
fn is_method_require_block_pass(arg: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::BlockPass(inner) = *cx.kind(arg) else {
        return false;
    };
    let Some(inner) = inner.get() else {
        return false;
    };
    let NodeKind::Send {
        receiver,
        method,
        args,
        ..
    } = *cx.kind(inner)
    else {
        return false;
    };
    // Must be a receiverless `method` call with a single symbol argument.
    if receiver.get().is_some() {
        return false;
    }
    if cx.symbol_str(method) != "method" {
        return false;
    }
    let method_args = cx.list(args);
    if method_args.len() != 1 {
        return false;
    }
    if let NodeKind::Sym(sym) = *cx.kind(method_args[0]) {
        let sym_str = cx.symbol_str(sym);
        if sym_str == "require" || sym_str == "require_relative" {
            return true;
        }
    }
    false
}

murphy_plugin_api::submit_cop!(NonDeterministicRequireOrder);

#[cfg(test)]
mod tests {
    use super::NonDeterministicRequireOrder;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_dir_index_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_dir_index_each_with_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require_relative file
            end
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each do |file|
              require_relative file
            end
            "#},
        );
    }

    #[test]
    fn flags_dir_glob_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), File::FNM_DOTMATCH).each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), File::FNM_DOTMATCH).sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_top_level_dir_index_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            ::Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            ::Dir["./lib/**/*.rb"].sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_top_level_dir_glob_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            ::Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), ::File::FNM_DOTMATCH).each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            ::Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), ::File::FNM_DOTMATCH).sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_dir_glob_block_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir.glob("./lib/**/*.rb") do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            Dir.glob("./lib/**/*.rb").sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_top_level_dir_glob_block_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            ::Dir.glob("./lib/**/*.rb") do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
            "#},
            indoc! {r#"
            ::Dir.glob("./lib/**/*.rb").sort.each do |file|
              require file
            end
            "#},
        );
    }

    #[test]
    fn flags_numblock_with_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each do
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require _1
            end
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each do
              require _1
            end
            "#},
        );
    }

    #[test]
    fn flags_numblock_with_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each do
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require_relative _1
            end
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each do
              require_relative _1
            end
            "#},
        );
    }

    #[test]
    fn flags_numblock_second_numbered_param() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each { require _2 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each { require _2 }
            "#},
        );
    }

    #[test]
    fn flags_dir_index_each_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each(&method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each(&method(:require))
            "#},
        );
    }

    #[test]
    fn flags_dir_index_each_block_pass_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each(&method(:require_relative))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each(&method(:require_relative))
            "#},
        );
    }

    #[test]
    fn flags_dir_glob_each_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir.glob(Rails.root.join('test', '*.rb')).each(&method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir.glob(Rails.root.join('test', '*.rb')).sort.each(&method(:require))
            "#},
        );
    }

    #[test]
    fn flags_dir_glob_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir.glob('./lib/**/*.rb', &method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir.glob('./lib/**/*.rb').sort.each(&method(:require))
            "#},
        );
    }

    #[test]
    fn flags_dir_glob_block_pass_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir.glob('./lib/**/*.rb', &method(:require_relative))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
            "#},
            indoc! {r#"
            Dir.glob('./lib/**/*.rb').sort.each(&method(:require_relative))
            "#},
        );
    }

    #[test]
    fn flags_require_inside_conditional() {
        test::<NonDeterministicRequireOrder>().expect_correction(
            indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              if file.start_with?('_')
                puts "Not required."
              else
                require file
              end
            end
            "#},
            indoc! {r#"
            Dir["./lib/**/*.rb"].sort.each do |file|
              if file.start_with?('_')
                puts "Not required."
              else
                require file
              end
            end
            "#},
        );
    }

    #[test]
    fn accepts_sorted_dir_index_each() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(
            "Dir[\"./lib/**/*.rb\"].sort.each do |file|\n  require file\nend\n",
        );
    }

    #[test]
    fn accepts_sorted_dir_glob_each() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), File::FNM_DOTMATCH).sort.each do |file|
              require file
            end
        "#});
    }

    #[test]
    fn accepts_dir_glob_with_sort_false_keyword() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir.glob(Rails.root.join('test', '*.rb'), sort: false).each(&method(:require))
        "#});
    }

    #[test]
    fn accepts_non_require_block_body() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
              puts file
            end
        "#});
    }

    #[test]
    fn accepts_dir_glob_block_not_require_body() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir.glob("./lib/**/*.rb") do |file|
              puts file
            end
        "#});
    }

    #[test]
    fn accepts_sorted_block_pass() {
        test::<NonDeterministicRequireOrder>()
            .expect_no_offenses("Dir[\"./lib/**/*.rb\"].sort.each(&method(:require))\n");
    }

    #[test]
    fn accepts_non_dir_glob_each() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Files.glob('*.rb').each do |file|
              require file
            end
        "#});
    }

    #[test]
    fn accepts_multi_argument_block() {
        // Upstream `loop_variable` (`(args (arg $_))`) only matches a
        // single plain block argument.
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file, i|
              require file
            end
        "#});
    }

    #[test]
    fn accepts_numblock_without_require() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Dir["./lib/**/*.rb"].each do
              puts _1
            end
        "#});
    }

    #[test]
    fn maximum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <NonDeterministicRequireOrder as Cop>::MAXIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(2, 7)),
        );
    }

    #[test]
    fn no_offense_on_ruby_3_0() {
        test::<NonDeterministicRequireOrder>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
              require file
            end
        "#});
        test::<NonDeterministicRequireOrder>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("Dir[\"./lib/**/*.rb\"].each(&method(:require))\n");
    }

    #[test]
    fn flags_on_ruby_2_7() {
        test::<NonDeterministicRequireOrder>()
            .with_target_ruby_version(2, 7)
            .expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }
}
