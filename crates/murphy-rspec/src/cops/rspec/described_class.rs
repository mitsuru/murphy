//! `RSpec/DescribedClass` — prefer `described_class` over explicit constants.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DescribedClass
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`described_constant`: `(block (send _
//!   :describe $(const ...) ...) (args) $_)` — any receiver, `describe`
//!   only, first arg a `Const`, body present) with `EnforcedStyle`
//!   (`described_class` default, `explicit` alternate), `SkipBlocks`
//!   (default false) and `OnlyStaticConstants` (default true). The body is
//!   walked with `find_usage`: yield when `offensive?`, prune when
//!   `scope_change?` (`def` / `class` / `module`,
//!   `Class/Module/Struct.new` + `Data.define` instance-exec closures, or
//!   — with `SkipBlocks` — any non-RSpec block) or `allowed?` (`Const`
//!   with `OnlyStaticConstants`). `offensive?` for `described_class`
//!   style skips `described_class::CONST` (contains a bare
//!   `described_class` send), skips the describe's own const arg, and
//!   compares `full_const_name` (`collapse_namespace(namespace,
//!   const_name)`, where `namespace` is enclosing `class` / `module`
//!   names). For `explicit` style any bare `described_class` send is
//!   offensive. Messages mirror `MSG` (`Use `%<replacement>s` instead of
//!   `%<src>s`). Detection is at parity for the common shapes;
//!   autocorrect (replace with `described_class` / the explicit name) is
//!   not ported in this batch — same convention as `RSpec/HookArgument`
//!   (status: partial, autocorrect as gap). Murphy collapses `::Foo` to
//!   `Const { scope: None }` (no `cbase`), so leading-`::` absoluteness
//!   is not distinguished.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is `describe` with a `Const` first
//! arg (any receiver, mirroring `(send _ :describe ...)`):
//!
//! - `describe MyClass do; subject { MyClass.do_something }; end` —
//!   flagged (`Use `described_class` instead of `MyClass``).
//! - `describe MyClass do; subject { described_class.do_something };
//!   end` — preferred spelling, not flagged.
//! - `describe MyClass do; subject { MyClass::CONSTANT }; end` — static
//!   constant, not flagged by default (`OnlyStaticConstants: true`);
//!   flagged (the inner `MyClass`) when `OnlyStaticConstants: false`.
//! - `describe MyClass do; subject { described_class.x }; end` with
//!   `EnforcedStyle: explicit` — flagged (`Use `MyClass` instead of
//!   `described_class``).
//! - `def foo; MyClass; end` inside a group — scope change, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces with `described_class` (or the explicit name;
//! unsafe when `SkipBlocks: false`). This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribedClass;

#[derive(CopOptions)]
pub struct DescribedClassOptions {
    #[option(
        name = "EnforcedStyle",
        default = "described_class",
        description = "Whether to prefer described_class or explicit class references."
    )]
    pub enforced_style: DescribedClassStyle,
    #[option(
        name = "SkipBlocks",
        default = false,
        description = "Whether to skip non-RSpec blocks (e.g. controller helpers)."
    )]
    pub skip_blocks: bool,
    #[option(
        name = "OnlyStaticConstants",
        default = true,
        description = "Whether static constants like MyClass::CONSTANT are allowed."
    )]
    pub only_static_constants: bool,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum DescribedClassStyle {
    #[option(value = "described_class")]
    DescribedClass,
    #[option(value = "explicit")]
    Explicit,
}

#[cop(
    name = "RSpec/DescribedClass",
    description = "Checks that tests use `described_class`.",
    default_severity = "warning",
    default_enabled = true,
    options = DescribedClassOptions,
)]
impl DescribedClass {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            method, args, ..
        } = *cx.kind(call)
        else {
            return;
        };
        if cx.symbol_str(method) != "describe" {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !matches!(*cx.kind(first), NodeKind::Const { .. }) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        let opts = cx.options_or_default::<DescribedClassOptions>();
        let described_name = cx.const_name(first).unwrap_or_default();
        walk_body(cx, body_id, first, &described_name, &opts);
    }
}

/// Depth-first walk mirroring `find_usage`: check `offensive?`, prune on
/// `scope_change?` / `allowed?`, else recurse into children.
fn walk_body(
    cx: &Cx<'_>,
    id: NodeId,
    described_const: NodeId,
    described_name: &str,
    opts: &DescribedClassOptions,
) {
    if is_offensive(cx, id, described_const, described_name, opts) {
        emit(cx, id, described_const, described_name, opts);
    }
    if is_scope_change(cx, id, opts.skip_blocks) || is_allowed(cx, id, opts) {
        return;
    }
    for child in cx.children(id) {
        walk_body(cx, child, described_const, described_name, opts);
    }
}

fn is_offensive(
    cx: &Cx<'_>,
    id: NodeId,
    described_const: NodeId,
    _described_name: &str,
    opts: &DescribedClassOptions,
) -> bool {
    match opts.enforced_style {
        DescribedClassStyle::Explicit => {
            matches!(*cx.kind(id), NodeKind::Send { receiver, method, .. }
                if receiver == OptNodeId::NONE && cx.symbol_str(method) == "described_class")
        }
        DescribedClassStyle::DescribedClass => {
            if !matches!(*cx.kind(id), NodeKind::Const { .. }) {
                return false;
            }
            if contains_described_class(cx, id) {
                return false;
            }
            if id == described_const {
                return false;
            }
            // Nearest enclosing describe's const: walk block ancestors for
            // a `describe` with a const first arg.
            let Some(nearest) = nearest_described_const(cx, id) else {
                return false;
            };
            if nearest == id {
                return false;
            }
            full_const_name(cx, nearest) == full_const_name(cx, id)
        }
    }
}

fn emit(
    cx: &Cx<'_>,
    id: NodeId,
    described_const: NodeId,
    described_name: &str,
    opts: &DescribedClassOptions,
) {
    match opts.enforced_style {
        DescribedClassStyle::Explicit => {
            cx.emit_offense(
                cx.range(id),
                &format!("Use `{described_name}` instead of `described_class`."),
                None,
            );
        }
        DescribedClassStyle::DescribedClass => {
            let src = cx.const_name(id).unwrap_or_else(|| {
                // Fallback to source slice for exotic const shapes.
                cx.raw_source(cx.range(id)).to_owned()
            });
            // Skip the describe's own const arg (should not happen since
            // we only walk the body, but guard anyway).
            if id == described_const {
                return;
            }
            cx.emit_offense(
                cx.range(id),
                &format!("Use `described_class` instead of `{src}`."),
                None,
            );
        }
    }
}

/// `true` when `id`'s subtree contains a bare `described_class` send
/// (covers `described_class::CONSTANT`).
fn contains_described_class(cx: &Cx<'_>, id: NodeId) -> bool {
    for desc in core::iter::once(id).chain(cx.descendants(id)) {
        if let NodeKind::Send { receiver, method, .. } = *cx.kind(desc)
            && receiver == OptNodeId::NONE
            && cx.symbol_str(method) == "described_class"
        {
            return true;
        }
    }
    false
}

/// Nearest enclosing `describe` const (via block ancestors), or `None`.
fn nearest_described_const(cx: &Cx<'_>, id: NodeId) -> Option<NodeId> {
    for anc in cx.ancestors(id) {
        let NodeKind::Block { call, .. } = *cx.kind(anc) else {
            continue;
        };
        let NodeKind::Send { method, args, .. } = *cx.kind(call) else {
            continue;
        };
        if cx.symbol_str(method) != "describe" {
            continue;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            continue;
        };
        if matches!(*cx.kind(first), NodeKind::Const { .. }) {
            return Some(first);
        }
    }
    None
}

/// Full constant name with lexical `class` / `module` namespace collapsed,
/// mirroring `full_const_name` + `collapse_namespace`.
fn full_const_name(cx: &Cx<'_>, id: NodeId) -> Vec<String> {
    let ns = lexical_namespace(cx, id);
    let parts = const_parts(cx, id);
    collapse_namespace(&ns, &parts)
}

/// Lexical namespace: enclosing `class` / `module` defined names split on
/// `::`, innermost last.
fn lexical_namespace(cx: &Cx<'_>, id: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    // Collect outermost-first by reversing ancestors.
    let mut stack: Vec<NodeId> = cx.ancestors(id).collect();
    stack.reverse();
    for anc in stack {
        let name_id = match *cx.kind(anc) {
            NodeKind::Class { name, .. } => Some(name),
            NodeKind::Module { name, .. } => Some(name),
            _ => None,
        };
        if let Some(nid) = name_id
            && let Some(full) = cx.const_name(nid)
        {
            for part in full.split("::") {
                if !part.is_empty() {
                    out.push(part.to_owned());
                }
            }
        }
    }
    out
}

/// Constant parts: `None` head marks an absolute / unresolvable scope
/// (`cbase`, `lvar`, `send` like `described_class::X`), mirroring
/// upstream `const_name` returning `[nil, name]`.
fn const_parts(cx: &Cx<'_>, id: NodeId) -> Vec<Option<String>> {
    let NodeKind::Const { scope, name } = *cx.kind(id) else {
        return Vec::new();
    };
    let short = cx.symbol_str(name).to_owned();
    let Some(scope_id) = scope.get() else {
        return vec![Some(short)];
    };
    match *cx.kind(scope_id) {
        NodeKind::Const { .. } => {
            let mut parent = const_parts(cx, scope_id);
            parent.push(Some(short));
            parent
        }
        NodeKind::Cbase => vec![None, Some(short)],
        // `described_class::X`, `var::X`, etc. — unresolvable relatively.
        _ => vec![None, Some(short)],
    }
}

/// Collapse `namespace` + `const` per upstream `collapse_namespace`.
fn collapse_namespace(namespace: &[String], konst: &[Option<String>]) -> Vec<String> {
    // Absolute or empty namespace → const as-is (drop leading `None`).
    if namespace.is_empty() || konst.first() == Some(&None) {
        return konst
            .iter()
            .filter_map(|p| p.clone())
            .collect();
    }
    let c: Vec<String> = konst.iter().filter_map(|p| p.clone()).collect();
    let start = namespace.len().saturating_sub(c.len());
    let max = namespace.len();
    let mut intersection = max;
    for shift in start..=max {
        let tail_len = max - shift;
        if tail_len <= c.len() && namespace[shift..] == c[..tail_len] {
            intersection = shift;
            break;
        }
    }
    let mut out = namespace[..intersection].to_vec();
    out.extend(c);
    out
}

/// `true` when `id` changes scope: `def` / `defs` / `class` / `module`,
/// `Class/Module/Struct.new` + `Data.define` blocks, or (with
/// `skip_blocks`) any non-RSpec block.
fn is_scope_change(cx: &Cx<'_>, id: NodeId, skip_blocks: bool) -> bool {
    match *cx.kind(id) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => return true,
        NodeKind::Class { .. } | NodeKind::Module { .. } => return true,
        _ => {}
    }
    if is_instance_exec_closure(cx, id) {
        return true;
    }
    if skip_blocks && is_skippable_block(cx, id) {
        return true;
    }
    false
}

/// `Class.new` / `Module.new` / `Struct.new` / `Data.define` blocks.
fn is_instance_exec_closure(cx: &Cx<'_>, id: NodeId) -> bool {
    let (call, ) = match *cx.kind(id) {
        NodeKind::Block { call, .. } => (call,),
        _ => return false,
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Const { scope, name } = *cx.kind(recv) else {
        return false;
    };
    if scope != OptNodeId::NONE {
        return false;
    }
    let recv_name = cx.symbol_str(name);
    let method_name = cx.symbol_str(method);
    matches!(
        (recv_name, method_name),
        ("Class" | "Module" | "Struct", "new") | ("Data", "define")
    )
}

/// Non-RSpec blocks (when `SkipBlocks`): any block type whose call is not
/// an RSpec DSL method with an RSpec-or-bare receiver.
fn is_skippable_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let call = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => return false,
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    // RSpec blocks are never skippable.
    if is_rspec_or_bare_receiver(cx, receiver) && is_rspec_method(cx.symbol_str(method)) {
        return false;
    }
    true
}

/// `true` when `id` is a `Const` and static constants are allowed
/// (prunes recursion into `MyClass::CONSTANT`).
fn is_allowed(cx: &Cx<'_>, id: NodeId, opts: &DescribedClassOptions) -> bool {
    opts.only_static_constants && matches!(*cx.kind(id), NodeKind::Const { .. })
}

/// RSpec DSL methods (`Language::ALL` subset used for `SkipBlocks`).
fn is_rspec_method(name: &str) -> bool {
    matches!(
        name,
        "describe" | "context" | "feature" | "example_group"
            | "xdescribe" | "xcontext" | "xfeature"
            | "fdescribe" | "fcontext" | "ffeature"
            | "it" | "specify" | "example" | "scenario" | "its"
            | "fit" | "fspecify" | "fexample" | "fscenario" | "focus"
            | "xit" | "xspecify" | "xexample" | "xscenario" | "skip"
            | "pending"
            | "are_expected" | "expect" | "expect_any_instance_of" | "is_expected"
            | "should" | "should_not" | "should_not_receive" | "should_receive"
            | "let" | "let!"
            | "prepend_before" | "before" | "append_before" | "around"
            | "prepend_after" | "after" | "append_after"
            | "it_behaves_like" | "it_should_behave_like" | "include_examples"
            | "include_context"
            | "to" | "to_not" | "not_to"
            | "shared_examples" | "shared_examples_for" | "shared_context"
            | "subject" | "subject!"
            | "raise_error" | "raise_exception"
    )
}

#[cfg(test)]
mod tests {
    use super::{DescribedClass, DescribedClassOptions, DescribedClassStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn explicit() -> DescribedClassOptions {
        DescribedClassOptions {
            enforced_style: DescribedClassStyle::Explicit,
            skip_blocks: false,
            only_static_constants: true,
        }
    }

    fn no_static() -> DescribedClassOptions {
        DescribedClassOptions {
            enforced_style: DescribedClassStyle::DescribedClass,
            skip_blocks: false,
            only_static_constants: false,
        }
    }

    fn skip_blocks() -> DescribedClassOptions {
        DescribedClassOptions {
            enforced_style: DescribedClassStyle::DescribedClass,
            skip_blocks: true,
            only_static_constants: true,
        }
    }

    #[test]
    fn flags_explicit_constant() {
        test::<DescribedClass>().expect_offense(indoc! {r#"
                describe MyClass do
                  subject { MyClass.do_something }
                            ^^^^^^^ Use `described_class` instead of `MyClass`.
                end
            "#});
    }

    #[test]
    fn does_not_flag_described_class() {
        test::<DescribedClass>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                  subject { described_class.do_something }
                end
            "#});
    }

    #[test]
    fn allows_static_constant_by_default() {
        test::<DescribedClass>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                  subject { MyClass::CONSTANT }
                end
            "#});
    }

    #[test]
    fn flags_static_constant_when_not_allowed() {
        test::<DescribedClass>()
            .with_options(&no_static())
            .expect_offense(indoc! {r#"
                describe MyClass do
                  subject { MyClass::CONSTANT }
                            ^^^^^^^ Use `described_class` instead of `MyClass`.
                end
            "#});
    }

    #[test]
    fn flags_described_class_in_explicit_style() {
        test::<DescribedClass>()
            .with_options(&explicit())
            .expect_offense(indoc! {r#"
                describe MyClass do
                  subject { described_class.do_something }
                            ^^^^^^^^^^^^^^^ Use `MyClass` instead of `described_class`.
                end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_constant_in_explicit_style() {
        test::<DescribedClass>()
            .with_options(&explicit())
            .expect_no_offenses(indoc! {r#"
                describe MyClass do
                  subject { MyClass.do_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_inside_def() {
        test::<DescribedClass>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                  def foo
                    MyClass.do_something
                  end
                end
            "#});
    }

    #[test]
    fn skips_non_rspec_blocks_when_configured() {
        test::<DescribedClass>()
            .with_options(&skip_blocks())
            .expect_no_offenses(indoc! {r#"
                describe MyConcern do
                  controller(ApplicationController) do
                    include MyConcern
                  end
                end
            "#});
    }

    #[test]
    fn does_not_skip_non_rspec_blocks_by_default() {
        // Without `SkipBlocks`, the inner constant still flags.
        test::<DescribedClass>().expect_offense(indoc! {r#"
                describe MyConcern do
                  controller(ApplicationController) do
                    include MyConcern
                            ^^^^^^^^^ Use `described_class` instead of `MyConcern`.
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(DescribedClass);
