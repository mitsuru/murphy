//! `Lint/NonDeterministicRequireOrder` — flags `Dir[...]` calls that may
//! return files in non-deterministic order when used with `require`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NonDeterministicRequireOrder
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers the most common patterns:
//!   `Dir[...].each { |f| require f }`,
//!   `Dir.glob(...).each { |f| require f }`,
//!   `Dir.glob(...) { |f| require f }`,
//!   `Dir[...].each(&method(:require))`,
//!   and `Dir.glob(..., &method(:require))`.
//!
//!   Known v1 gaps: (1) Numblock forms (`Dir[...].each { require _1 }`)
//!   are not dispatched — Murphy v1 has no `on_numblock` dispatch for this
//!   cop. (2) `require`/`require_relative` inside nested conditionals
//!   within the block body (e.g. `if cond; require f; end`) is not detected
//!   — the search is limited to send nodes directly reachable from the body.
//!   (3) This cop is intended for Ruby < 3.0 (Ruby 3.0+ sorts Dir.glob by
//!   default) — Murphy does not gate by TargetRubyVersion, so the cop may
//!   fire alongside `Lint/RedundantDirGlobSort` in Ruby 3.0+ projects.
//! ```
//!
//! ## Matched shapes
//! - `Dir[pattern].each { |f| require f }` — Dir glob with each block
//! - `Dir.glob(pattern).each { |f| require f }` — Dir.glob with each block
//! - `Dir.glob(pattern) { |f| require f }` — Dir.glob with direct block
//! - `Dir[pattern].each(&method(:require))` — each with block-pass
//! - `Dir.glob(pattern, &method(:require))` — glob with block-pass
//! - `Dir[pattern].each(&method(:require_relative))` — block-pass with require_relative

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct NonDeterministicRequireOrder;

const MSG: &str = "Sort files before requiring them.";

#[cop(
    name = "Lint/NonDeterministicRequireOrder",
    description = "Flags `Dir[...]` calls that may return files in non-deterministic order.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NonDeterministicRequireOrder {
    /// Handle block forms:
    /// - `Dir[pattern].each { |f| require f }`
    /// - `Dir.glob(pattern).each { |f| require f }`
    /// - `Dir.glob(pattern) { |f| require f }`
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };

        // Determine what the call is — could be `Dir.glob` (direct block)
        // or `Dir[...].each` / `Dir.glob(...).each` (each block).
        let (dir_call, is_each) = match identify_dir_call(call, cx) {
            Some(result) => result,
            None => return,
        };

        // If it's a `.each` call, the receiver is the Dir call.
        // If it's a direct `.glob` call, the call itself is the Dir call.
        if is_each && !unsorted_dir_glob(&dir_call, cx) {
            return;
        }
        if !is_each && !unsorted_dir_block(&dir_call, cx) {
            return;
        }

        // Get the block variable name (first argument).
        let var_name = first_block_arg_name(args, cx);
        if var_name.is_none() {
            return;
        }

        // Search the body for `require var_name` or `require_relative var_name`.
        if !body_requires_var(body_id, var_name.unwrap(), cx) {
            return;
        }

        // Emit offense on the Dir glob call. Use the closing paren to cap
        // the range when the parser extends it past the method call end.
        let range = {
            let expr = cx.range(dir_call);
            let close = cx.loc(dir_call).end();
            if close != Range::ZERO {
                Range { start: expr.start, end: close.end }
            } else {
                expr
            }
        };
        cx.emit_offense(range, MSG, None);
    }

    /// Handle `&method(:require)` or `&method(:require_relative)` block-pass
    /// forms:
    /// - `Dir[pattern].each(&method(:require))`
    /// - `Dir.glob(pattern, &method(:require))`
    #[on_node(kind = "send", methods = ["[]", "glob", "each"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let method = cx.method_name(node).unwrap_or("");

        if method == "each" {
            // `Dir[...].each(&method(:require))` or `Dir.glob(...).each(&method(:require))`
            let Some(receiver) = cx.call_receiver(node).get() else {
                return;
            };
            if !unsorted_dir_glob(&receiver, cx) {
                return;
            }
            let args = cx.call_arguments(node);
            if !has_method_require_block_pass(args, cx) {
                return;
            }
            cx.emit_offense(cx.range(node), MSG, None);
        } else {
            // `Dir.glob(..., &method(:require))` or `Dir[...](...)` (not applicable
            // since `[]` takes only one string arg, but handled for completeness).
            if !is_dir_const_receiver(node, cx) {
                return;
            }
            let args = cx.call_arguments(node);
            if method == "[]" {
                // `Dir[...]` without each is unusual for require, but check.
                if !has_method_require_block_pass(args, cx) {
                    return;
                }
                cx.emit_offense(cx.range(node), MSG, None);
            } else if method == "glob" {
                if !unsorted_dir_block(&node, cx) {
                    return;
                }
                if !has_method_require_block_pass(args, cx) {
                    return;
                }
                cx.emit_offense(cx.range(node), MSG, None);
            }
        }
    }
}

/// Identify whether `call` is a direct Dir call or a `.each` wrapping a Dir call.
/// Returns `Some((dir_call_node, is_each))` where `is_each` means "the dir_call
/// is the receiver of an `.each`".
fn identify_dir_call(call: NodeId, cx: &Cx<'_>) -> Option<(NodeId, bool)> {
    if !matches!(*cx.kind(call), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return None;
    }

    let method = cx.method_name(call)?;

    if method == "each" {
        let receiver = cx.call_receiver(call).get()?;
        if unsorted_dir_glob(&receiver, cx) {
            return Some((receiver, true));
        }
    } else if (method == "glob" || method == "[]")
        && is_dir_const_receiver(call, cx) {
            return Some((call, false));
        }

    None
}

/// Check if `node` is `Dir.glob(...)` or `Dir[...]` (the `[]` or `glob` methods
/// called on `Dir` constant), suitable for `.each` chaining.
fn unsorted_dir_glob(node: &NodeId, cx: &Cx<'_>) -> bool {
    let mut id = *node;
    while let NodeKind::Begin(list) = *cx.kind(id) {
        let children = cx.list(list);
        if children.len() == 1 {
            id = children[0];
        } else {
            break;
        }
    }
    let NodeKind::Send { .. } = *cx.kind(id) else {
        return false;
    };
    if !is_dir_const_receiver(id, cx) {
        return false;
    }
    let args = cx.call_arguments(id);
    if args.is_empty() {
        return false;
    }
    // Exclude if any argument contains `sort: false`.
    if args
        .iter()
        .any(|&arg| cx.raw_source(cx.range(arg)).contains("sort: false"))
    {
        return false;
    }
    matches!(cx.method_name(id), Some("[]") | Some("glob"))
}

/// Check if `node` is a direct `Dir.glob(...)` call (the method itself, used
/// without `.each` — i.e., in block form `Dir.glob(...) { |f| ... }`).
fn unsorted_dir_block(node: &NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { args, .. } = *cx.kind(*node) else {
        return false;
    };
    if !is_dir_const_receiver(*node, cx) {
        return false;
    }
    let args = cx.list(args);
    // `glob` requires at least a pattern; `[]` requires a pattern.
    if args.is_empty() {
        return false;
    }
    // Exclude if any argument contains `sort: false`.
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

/// Get the name of the first block argument (e.g. `|file|` → `"file"`).
fn first_block_arg_name(args: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let children = cx.list(list);
    let first = children.first()?;
    match *cx.kind(*first) {
        NodeKind::Arg(sym) => Some(cx.symbol_str(sym).to_string()),
        NodeKind::Optarg { name, .. } => Some(cx.symbol_str(name).to_string()),
        _ => None,
    }
}

/// Search the body subtree for `require var_name` or `require_relative var_name`.
fn body_requires_var(body: NodeId, var_name: String, cx: &Cx<'_>) -> bool {
    // Check the body directly first.
    if send_requires_var(body, &var_name, cx) {
        return true;
    }
    // Also check descendants for nested requires (e.g. inside if/else branches).
    for desc in cx.descendants(body) {
        if send_requires_var(desc, &var_name, cx) {
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
    if !matches!(cx.method_name(node), Some("require") | Some("require_relative")) {
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

/// Check if args contain a `&method(:require)` or `&method(:require_relative)`
/// block-pass.
fn has_method_require_block_pass(args: &[NodeId], cx: &Cx<'_>) -> bool {
    for &arg in args {
        let NodeKind::BlockPass(inner) = *cx.kind(arg) else {
            continue;
        };
        let Some(inner) = inner.get() else {
            continue;
        };
        let NodeKind::Send {
            receiver,
            method,
            args,
            ..
        } = *cx.kind(inner)
        else {
            continue;
        };
        // Must be a receiverless `method` call with a single symbol argument.
        if receiver.get().is_some() {
            continue;
        }
        if cx.symbol_str(method) != "method" {
            continue;
        }
        let method_args = cx.list(args);
        let Some(first) = method_args.first() else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(*first) {
            let sym_str = cx.symbol_str(sym);
            if sym_str == "require" || sym_str == "require_relative" {
                return true;
            }
        }
    }
    false
}

murphy_plugin_api::submit_cop!(NonDeterministicRequireOrder);

#[cfg(test)]
mod tests {
    use super::NonDeterministicRequireOrder;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── block with each ─────────────────────────────────────────────────

    #[test]
    fn flags_dir_index_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    #[test]
    fn flags_dir_index_each_with_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require_relative file
            end
        "#});
    }

    #[test]
    fn flags_dir_glob_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), File::FNM_DOTMATCH).each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    #[test]
    fn flags_top_level_dir_index_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            ::Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    #[test]
    fn flags_top_level_dir_glob_each_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            ::Dir.glob(Rails.root.join(__dir__, 'test', '*.rb'), ::File::FNM_DOTMATCH).each do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    // ── direct block on glob ────────────────────────────────────────────

    #[test]
    fn flags_dir_glob_block_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir.glob("./lib/**/*.rb") do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    #[test]
    fn flags_top_level_dir_glob_block_with_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            ::Dir.glob("./lib/**/*.rb") do |file|
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              require file
            end
        "#});
    }

    // ── block-pass forms ────────────────────────────────────────────────

    #[test]
    fn flags_dir_index_each_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each(&method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
        "#});
    }

    #[test]
    fn flags_dir_index_each_block_pass_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each(&method(:require_relative))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
        "#});
    }

    #[test]
    fn flags_dir_glob_each_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir.glob(Rails.root.join('test', '*.rb')).each(&method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
        "#});
    }

    #[test]
    fn flags_dir_glob_block_pass_require() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir.glob('./lib/**/*.rb', &method(:require))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
        "#});
    }

    #[test]
    fn flags_dir_glob_block_pass_require_relative() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir.glob('./lib/**/*.rb', &method(:require_relative))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
        "#});
    }

    // ── require inside conditional (descendant search) ──────────────────

    #[test]
    fn flags_require_inside_conditional() {
        test::<NonDeterministicRequireOrder>().expect_offense(indoc! {r#"
            Dir["./lib/**/*.rb"].each do |file|
            ^^^^^^^^^^^^^^^^^^^^ Sort files before requiring them.
              if file.start_with?('_')
                puts "Not required."
              else
                require file
              end
            end
        "#});
    }

    // ── accepted patterns ───────────────────────────────────────────────

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
        test::<NonDeterministicRequireOrder>().expect_no_offenses(
            "Dir[\"./lib/**/*.rb\"].sort.each(&method(:require))\n",
        );
    }

    #[test]
    fn accepts_non_dir_glob_each() {
        test::<NonDeterministicRequireOrder>().expect_no_offenses(indoc! {r#"
            Files.glob('*.rb').each do |file|
              require file
            end
        "#});
    }
}
