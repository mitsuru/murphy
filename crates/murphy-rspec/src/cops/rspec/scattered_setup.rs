//! `RSpec/ScatteredSetup` — do not scatter setup across multiple same-scope hooks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ScatteredSetup
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group?`: a bare or `RSpec`
//!   `ExampleGroups.all` block) with `repeated_hooks`
//!   (`ExampleGroup#hooks` in-scope search halting at scope changes and
//!   examples, keeping only `knowable_scope?` non-`around` hooks grouped
//!   by `(name, scope, metadata)`). Scope maps `each` / `example` /
//!   bare / hash to `each`, `all` / `context` to `context`, `suite` to
//!   `suite`; metadata merges post-scope syms (`:k` ≡ `k: true`) with
//!   `true`-normalised values. Each repeat flags the whole hook block
//!   per `add_offense(occurrence)` with `Do not define multiple
//!   `%<hook>s` hooks in the same example group (also defined on
//!   line(s) ...).`. Detection is at parity (verified vs 3.7.0,
//!   including scope-symbol, metadata, and `around`-clean cases);
//!   autocorrect (splice bodies into the first hook) is not ported in
//!   this batch — same convention as `RSpec/HookArgument` (status:
//!   partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` example groups:
//!
//! - Two `before {}` — both flagged.
//! - `after {}`, `after(:each) {}`, `after(:example) {}` — all flagged.
//! - Two `before(:all) {}` — both flagged.
//! - Two `around {}` — clean (`around` excluded).
//! - `before {}` + `after {}` — different names, clean.
//! - `before {}` + `before(:all) {}` — different scopes, clean.
//!
//! ## No autocorrect
//!
//! Upstream splices later hook bodies into the first hook and removes
//! the later blocks. This batch reports only.

use std::collections::{BTreeMap, HashMap};

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_call, line_index_of_offset};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ScatteredSetup;

#[cop(
    name = "RSpec/ScatteredSetup",
    description = "Checks for setup scattered across multiple hooks in an example group.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ScatteredSetup {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        let hooks = hooks_in_scope(cx, body_id);
        let mut by_key: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        let mut meta: HashMap<NodeId, String> = HashMap::new();
        // Key by (name, scope, metadata); keep insertion order via the
        // hooks vec for line reporting.
        for hook in hooks {
            let Some((name, scope, md)) = hook_key(cx, hook) else {
                continue;
            };
            let key = format!("{name}\x1f{scope}\x1f{md}");
            by_key.entry(key).or_default().push(hook);
            meta.entry(hook).or_insert(name);
        }
        let src = cx.source();
        for group in by_key.values() {
            if group.len() < 2 {
                continue;
            }
            let lines: Vec<usize> = group
                .iter()
                .map(|&h| line_index_of_offset(src, cx.range(h).start) + 1)
                .collect();
            for (idx, &hook) in group.iter().enumerate() {
                let others: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, l)| *l)
                    .collect();
                let name = meta.get(&hook).cloned().unwrap_or_default();
                let msg = format!(
                    "Do not define multiple `{name}` hooks in the same example group (also defined on {}).",
                    lines_msg(&others)
                );
                cx.emit_offense(cx.range(hook), &msg, None);
            }
        }
    }
}

fn lines_msg(numbers: &[usize]) -> String {
    if numbers.len() == 1 {
        format!("line {}", numbers[0])
    } else {
        format!(
            "lines {}",
            numbers
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Every in-scope hook block under `body`, in document order.
///
/// Mirrors `ExampleGroup#hooks` (`find_all_in_scope(node, :hook?)`):
/// a hook block is collected without descending inside it; scope-change
/// and example blocks halt the search without descending.
fn hooks_in_scope(cx: &Cx<'_>, body: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    // A lone body node is its own single child (mirrors `ScatteredLet`).
    match *cx.kind(body) {
        NodeKind::Begin(members) => {
            for child in cx.list(members).to_vec() {
                collect_hooks(cx, child, &mut out);
            }
        }
        _ => collect_hooks(cx, body, &mut out),
    }
    out
}

fn collect_hooks(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
    if hook_call(cx, id).is_some() {
        out.push(id);
        return;
    }
    if is_scope_change(cx, id) || is_example_block(cx, id) {
        return;
    }
    for child in cx.children(id) {
        collect_hooks(cx, child, out);
    }
}

/// The hook call when `id` is a bare hook block, else `None`.
///
/// Mirrors upstream `hook?` (`(any_block (send nil? #Hooks.all ...)
/// ...)`): bare receiver, any block wrapper.
fn hook_call(cx: &Cx<'_>, id: NodeId) -> Option<NodeId> {
    let call = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        NodeKind::Itblock { send, .. } => send,
        _ => return None,
    };
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return None;
    };
    if receiver != OptNodeId::NONE {
        return None;
    }
    if !is_hook_name(cx.symbol_str(method)) {
        return None;
    }
    Some(call)
}

fn is_hook_name(name: &str) -> bool {
    matches!(
        name,
        "before"
            | "after"
            | "around"
            | "prepend_before"
            | "append_before"
            | "prepend_after"
            | "append_after"
    )
}

/// `true` when `id` is a scope-change block (nested group or bare
/// include), mirroring `ExampleGroup#scope_change?`.
fn is_scope_change(cx: &Cx<'_>, id: NodeId) -> bool {
    let call = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        NodeKind::Itblock { send, .. } => send,
        _ => return false,
    };
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    let name = cx.symbol_str(method);
    if is_include_name(name) {
        return receiver == OptNodeId::NONE;
    }
    if !is_rspec_or_bare(cx, receiver) {
        return false;
    }
    is_example_group_name(name) || is_shared_group_name(name)
}

/// `true` when `id` is a bare example block, mirroring `example?`
/// (`(block (send nil? #Examples.all ...) ...)`).
fn is_example_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let call = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        NodeKind::Itblock { send, .. } => send,
        _ => return false,
    };
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    is_example_name(cx.symbol_str(method))
}

fn is_rspec_or_bare(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true;
    };
    matches!(
        *cx.kind(rid),
        NodeKind::Const { scope, name }
            if scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
    )
}

fn is_example_group_name(name: &str) -> bool {
    matches!(
        name,
        "describe"
            | "context"
            | "feature"
            | "example_group"
            | "xdescribe"
            | "xcontext"
            | "xfeature"
            | "fdescribe"
            | "fcontext"
            | "ffeature"
    )
}

fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

fn is_include_name(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

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

/// `(name, scope_key, metadata_key)` for a hook block, or `None` when
/// the scope is unknowable or the hook is `around`.
///
/// Mirrors `repeated_hooks` (`knowable_scope?`, `name != :around`,
/// group by `[name, scope, metadata]`).
fn hook_key(cx: &Cx<'_>, hook: NodeId) -> Option<(String, String, String)> {
    let call = hook_call(cx, hook)?;
    let NodeKind::Send { method, args, .. } = *cx.kind(call) else {
        return None;
    };
    let name = cx.symbol_str(method).to_owned();
    if name == "around" {
        return None;
    }
    let arg_ids = cx.list(args).to_vec();
    let scope_arg = arg_ids.first().copied();
    // `knowable_scope?`: no arg, a `Sym`, or a `Hash`.
    if let Some(sa) = scope_arg {
        match *cx.kind(sa) {
            NodeKind::Sym(_) | NodeKind::Hash(_) => {}
            _ => return None,
        }
    }
    let scope_key = scope_string(cx, scope_arg);
    let md_nodes: Vec<NodeId> = match scope_arg {
        None => Vec::new(),
        Some(sa) => {
            if matches!(*cx.kind(sa), NodeKind::Sym(s) if is_valid_scope(cx.symbol_str(s))) {
                arg_ids[1..].to_vec()
            } else {
                arg_ids
            }
        }
    };
    let md_key = metadata_string(cx, &md_nodes);
    Some((name, scope_key, md_key))
}

fn is_valid_scope(name: &str) -> bool {
    matches!(name, "each" | "example" | "context" | "all" | "suite")
}

fn scope_string(cx: &Cx<'_>, scope_arg: Option<NodeId>) -> String {
    let Some(sa) = scope_arg else {
        return "each".to_owned();
    };
    match *cx.kind(sa) {
        NodeKind::Hash(_) => "each".to_owned(),
        NodeKind::Sym(s) => match cx.symbol_str(s) {
            "each" | "example" => "each".to_owned(),
            "context" | "all" => "context".to_owned(),
            "suite" => "suite".to_owned(),
            other => format!("unknown:{other}"),
        },
        _ => "unknown".to_owned(),
    }
}

/// Canonical metadata string: sorted `key=value` pairs with `true`
/// normalised so `:k` and `k: true` compare equal.
///
/// Mirrors `Hook#metadata` (`transform_metadata` + `inject(&:merge)`).
fn metadata_string(cx: &Cx<'_>, nodes: &[NodeId]) -> String {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for &md in nodes {
        match *cx.kind(md) {
            NodeKind::Sym(s) => {
                map.insert(cx.symbol_str(s).to_owned(), "true".to_owned());
            }
            NodeKind::Str(sid) => {
                map.insert(cx.string_str(sid).to_owned(), "true".to_owned());
            }
            NodeKind::Hash(pairs) => {
                for &pair in cx.list(pairs) {
                    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                        continue;
                    };
                    let k = metadata_key_string(cx, key);
                    let v = if matches!(*cx.kind(value), NodeKind::True_) {
                        "true".to_owned()
                    } else {
                        cx.raw_source(cx.range(value)).to_owned()
                    };
                    map.insert(k, v);
                }
            }
            _ => {
                map.insert(cx.raw_source(cx.range(md)).to_owned(), "true".to_owned());
            }
        }
    }
    map.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn metadata_key_string(cx: &Cx<'_>, key: NodeId) -> String {
    match *cx.kind(key) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::ScatteredSetup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_before_hooks() {
        test::<ScatteredSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  before { bar }
                  ^^^^^^^^^^^^^^ Do not define multiple `before` hooks in the same example group (also defined on line 3).
                  before { baz }
                  ^^^^^^^^^^^^^^ Do not define multiple `before` hooks in the same example group (also defined on line 2).
                end
            "#});
    }

    #[test]
    fn flags_same_scope_symbol_variants() {
        test::<ScatteredSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  after { bar }
                  ^^^^^^^^^^^^^ Do not define multiple `after` hooks in the same example group (also defined on lines 3, 4).
                  after(:each) { baz }
                  ^^^^^^^^^^^^^^^^^^^^ Do not define multiple `after` hooks in the same example group (also defined on lines 2, 4).
                  after(:example) { baz }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Do not define multiple `after` hooks in the same example group (also defined on lines 2, 3).
                end
            "#});
    }

    #[test]
    fn ignores_around_hooks() {
        test::<ScatteredSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  around { bar }
                  around { baz }
                end
            "#});
    }

    #[test]
    fn ignores_different_hooks() {
        test::<ScatteredSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  before { bar }
                  after { baz }
                  around { qux }
                end
            "#});
    }

    #[test]
    fn ignores_different_scopes() {
        test::<ScatteredSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  before { bar }
                  before(:all) { baz }
                  before(:suite) { baz }
                end
            "#});
    }

    #[test]
    fn ignores_nested_group_hooks() {
        test::<ScatteredSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  before { bar }

                  describe '.baz' do
                    before { baz }
                  end
                end
            "#});
    }

    #[test]
    fn ignores_different_metadata() {
        test::<ScatteredSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  before(:example) { foo }
                  before(:example, :special_case) { bar }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ScatteredSetup);
