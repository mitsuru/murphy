//! `Style/AccessModifierDeclarations` — checks style of how access modifiers
//! are used.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AccessModifierDeclarations
//! upstream_version_checked: 1.86.2
//! version_added: "0.57"
//! safe: false
//! supports_autocorrect: false
//! status: partial
//! gap_issues: []
//! notes: >
//!   Report-only (no autocorrect). RuboCop's autocorrect is marked
//!   SafeAutoCorrect: false and requires multi-node rearrangement plus
//!   comment-attachment facilities that are not yet part of the v1 Cx ABI.
//!   All three Allow* config options are supported.
//!   AllowModifiersOnSymbols: true allows `private :foo, :bar` and
//!   `private *%i[...]` and `private *CONST` / `private *method_call` forms.
//!   AllowModifiersOnAttrs: true allows `private attr_reader :x` etc.
//!   AllowModifiersOnAliasMethod: true allows `private alias_method :a, :b`.
//!   The `right_siblings_same_inline_method?` guard (group style) is
//!   implemented: only the last in a consecutive `private def…` run is flagged,
//!   not earlier ones. Parent carve-out for Pair/Block/Numblock is implemented.
//! ```
//!
//! ## Matched shapes
//!
//! - `group` style (default): flags access modifiers that are inlined with a
//!   method definition (e.g. `private def bar; end`) unless they are NOT the
//!   last in a consecutive run of same-name inline modifiers.
//! - `inline` style: flags bare access modifiers (e.g. standalone `private`)
//!   when at least one `def` follows them before the next bare modifier.
//!
//! ## Autocorrect
//!
//! Not implemented. RuboCop's autocorrect is unsafe (`SafeAutoCorrect: false`)
//! and depends on comment-attachment facilities not yet in the v1 Cx ABI.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

// ── message helpers ───────────────────────────────────────────────────────────

fn group_msg(name: &str) -> String {
    format!("`{name}` should not be inlined in method definitions.")
}

fn inline_msg(name: &str) -> String {
    format!("`{name}` should be inlined in method definitions.")
}

// ── option types ──────────────────────────────────────────────────────────────

/// Enforced access-modifier declaration style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AccessModifierStyle {
    #[default]
    #[option(value = "group")]
    Group,
    #[option(value = "inline")]
    Inline,
}

/// Options for `Style/AccessModifierDeclarations`.

#[derive(CopOptions)]
pub struct AccessModifierDeclarationsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "group",
        description = "Enforces whether access modifiers should be used inline or as a group."
    )]
    pub enforced_style: AccessModifierStyle,

    #[option(
        name = "AllowModifiersOnSymbols",
        default = true,
        description = "Allow modifiers applied directly to symbols (`private :foo, :bar`)."
    )]
    pub allow_modifiers_on_symbols: bool,

    #[option(
        name = "AllowModifiersOnAttrs",
        default = true,
        description = "Allow modifiers applied to attr_* methods (`private attr_reader :x`)."
    )]
    pub allow_modifiers_on_attrs: bool,

    #[option(
        name = "AllowModifiersOnAliasMethod",
        default = true,
        description = "Allow modifiers applied to alias_method (`private alias_method :a, :b`)."
    )]
    pub allow_modifiers_on_alias_method: bool,
}

// ── cop struct ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AccessModifierDeclarations;

#[cop(
    name = "Style/AccessModifierDeclarations",
    description = "Checks style of how access modifiers are used.",
    default_severity = "warning",
    default_enabled = true,
    options = AccessModifierDeclarationsOptions,
)]
impl AccessModifierDeclarations {
    #[on_node(kind = "send", methods = ["private", "protected", "public", "module_function"])]
    fn check_send(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        opts: &AccessModifierDeclarationsOptions,
    ) {
        // Must be an access modifier (macro-scope: no receiver, known name).
        if !cx.is_access_modifier(node) {
            return;
        }

        // Parent carve-out: inside a Pair (hash value) or any block, skip.
        if is_inside_pair_or_block(node, cx) {
            return;
        }

        // Allow* option checks.
        if opts.allow_modifiers_on_symbols && access_modifier_with_symbol(node, cx) {
            return;
        }
        if opts.allow_modifiers_on_attrs && access_modifier_with_attr(node, cx) {
            return;
        }
        if opts.allow_modifiers_on_alias_method && access_modifier_with_alias_method(node, cx) {
            return;
        }

        match opts.enforced_style {
            AccessModifierStyle::Group => check_group_style(node, cx),
            AccessModifierStyle::Inline => check_inline_style(node, cx),
        }
    }
}

// ── allowed? helpers ──────────────────────────────────────────────────────────

/// Whether this node is nested inside a `Pair` (hash value) or a block
/// (`Block`, `Numblock`). Mirrors RuboCop's `node.parent&.type?(:pair, :any_block)`.
fn is_inside_pair_or_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        *cx.kind(parent),
        NodeKind::Pair { .. } | NodeKind::Block { .. } | NodeKind::Numblock { .. }
    )
}

/// Whether the node is `private :foo, :bar` / `private *%i[...]` / `private *CONST`
/// / `private *method_call` — i.e., arguments are symbols, or first arg is a
/// splat wrapping an array/const/send.
///
/// Mirrors RuboCop's `access_modifier_with_symbol?` pattern.
fn access_modifier_with_symbol(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return false;
    }
    // All args are symbols.
    if args.iter().all(|&a| matches!(*cx.kind(a), NodeKind::Sym(..))) {
        return true;
    }
    // First arg is a splat wrapping array/const/send.
    args.first().is_some_and(|&first| {
        if let NodeKind::Splat(inner) = *cx.kind(first) {
            inner.get().is_some_and(|inner_id| {
                matches!(
                    *cx.kind(inner_id),
                    NodeKind::Array(..) | NodeKind::Const { .. } | NodeKind::Send { .. }
                )
            })
        } else {
            false
        }
    })
}

/// Whether the node is `private attr_reader :x` / `private attr_writer :x` /
/// `private attr_accessor :x` / `private attr :x`.
///
/// Mirrors RuboCop's `access_modifier_with_attr?` pattern.
fn access_modifier_with_attr(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return false;
    };
    if let NodeKind::Send { receiver, .. } = *cx.kind(first) {
        if receiver.get().is_some() {
            return false;
        }
        return matches!(
            cx.method_name(first),
            Some("attr" | "attr_reader" | "attr_writer" | "attr_accessor")
        );
    }
    false
}

/// Whether the node is `private alias_method :new_name, :old_name`.
///
/// Mirrors RuboCop's `access_modifier_with_alias_method?` pattern.
fn access_modifier_with_alias_method(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return false;
    };
    if let NodeKind::Send { receiver, .. } = *cx.kind(first) {
        if receiver.get().is_some() {
            return false;
        }
        return cx.method_name(first) == Some("alias_method");
    }
    false
}

// ── offense detection ─────────────────────────────────────────────────────────

/// Group style: flag any non-bare (inlined) access modifier, unless:
/// - The parent is an `if` node (RuboCop skips these).
/// - No parent exists and the modifier uses symbols (no parent =
///   top-level context where bare symbol forms are tolerated).
/// - A right sibling has the same method as an inline def modifier (meaning
///   this is not the last in a consecutive run).
///
/// Mirrors RuboCop's `offense?` for group style:
/// `access_modifier_is_inlined?(node) && !right_siblings_same_inline_method?(node)`
fn check_group_style(node: NodeId, cx: &Cx<'_>) {
    // Only flag non-bare (inlined) access modifiers (any arguments).
    if !cx.is_non_bare_access_modifier(node) {
        return;
    }
    // Parent carve-out that mirrors RuboCop's ternary:
    //   `node.parent ? node.parent.if_type? : access_modifier_with_symbol?(node)`
    match cx.parent(node).get() {
        Some(parent) => {
            // Skip if inside an `if` expression.
            if matches!(*cx.kind(parent), NodeKind::If { .. }) {
                return;
            }
        }
        None => {
            // No parent (root-level) and a symbol modifier — skip.
            if access_modifier_with_symbol(node, cx) {
                return;
            }
        }
    }
    // Skip if a right sibling has the same inline method (not the last in run).
    if right_siblings_have_same_inline(node, cx) {
        return;
    }
    let name = cx.method_name(node).unwrap_or("private");
    cx.emit_offense(cx.node(node).loc.name, &group_msg(name), None);
}

/// Inline style: flag a bare modifier only when at least one `def` follows
/// before the next bare modifier.
fn check_inline_style(node: NodeId, cx: &Cx<'_>) {
    // Only flag bare modifiers.
    if !cx.is_bare_access_modifier(node) {
        return;
    }
    // Flag only if grouped def nodes follow.
    if select_grouped_def_nodes(node, cx).is_empty() {
        return;
    }
    let name = cx.method_name(node).unwrap_or("private");
    cx.emit_offense(cx.node(node).loc.name, &inline_msg(name), None);
}

// ── sibling helpers ───────────────────────────────────────────────────────────

/// Collects the right siblings of `node` within a `Begin` body.
/// Returns an empty vec if there's no parent or the parent is not a `Begin`.
fn right_siblings(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(parent) = cx.parent(node).get() else {
        return vec![];
    };
    let NodeKind::Begin(list) = *cx.kind(parent) else {
        return vec![];
    };
    let all = cx.list(list);
    match all.iter().position(|&s| s == node) {
        Some(i) => all[i + 1..].to_vec(),
        None => vec![],
    }
}

/// `select_grouped_def_nodes` — `def` nodes that follow this bare modifier
/// up to (but not including) the next bare access modifier.
fn select_grouped_def_nodes(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    right_siblings(node, cx)
        .into_iter()
        .take_while(|&sib| !cx.is_bare_access_modifier(sib))
        .filter(|&sib| matches!(*cx.kind(sib), NodeKind::Def { .. }))
        .collect()
}

/// `right_siblings_same_inline_method?` — true when any right sibling has
/// the same method name, is a non-bare access modifier, and wraps a def.
/// Used to suppress the offense on non-last occurrences in a consecutive run.
fn right_siblings_have_same_inline(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(my_name) = cx.method_name(node) else {
        return false;
    };
    right_siblings(node, cx).into_iter().any(|sib| {
        cx.method_name(sib) == Some(my_name)
            && cx.is_non_bare_access_modifier(sib)
            && cx.is_def_modifier(sib)
    })
}

// ── registration ──────────────────────────────────────────────────────────────

murphy_plugin_api::submit_cop!(AccessModifierDeclarations);

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        AccessModifierDeclarations, AccessModifierDeclarationsOptions, AccessModifierStyle,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn inline_opts() -> AccessModifierDeclarationsOptions {
        AccessModifierDeclarationsOptions {
            enforced_style: AccessModifierStyle::Inline,
            ..Default::default()
        }
    }

    fn no_symbols_opts() -> AccessModifierDeclarationsOptions {
        AccessModifierDeclarationsOptions {
            allow_modifiers_on_symbols: false,
            ..Default::default()
        }
    }

    fn no_attrs_opts() -> AccessModifierDeclarationsOptions {
        AccessModifierDeclarationsOptions {
            allow_modifiers_on_attrs: false,
            ..Default::default()
        }
    }

    fn no_alias_opts() -> AccessModifierDeclarationsOptions {
        AccessModifierDeclarationsOptions {
            allow_modifiers_on_alias_method: false,
            ..Default::default()
        }
    }

    // ── group style (default) ─────────────────────────────────────────────────

    #[test]
    fn group_flags_private_def() {
        test::<AccessModifierDeclarations>().expect_offense(indoc! {"
            class Foo
              private def bar; end
              ^^^^^^^ `private` should not be inlined in method definitions.
            end
        "});
    }

    #[test]
    fn group_flags_protected_def() {
        test::<AccessModifierDeclarations>().expect_offense(indoc! {"
            class Foo
              protected def bar; end
              ^^^^^^^^^ `protected` should not be inlined in method definitions.
            end
        "});
    }

    #[test]
    fn group_flags_public_def() {
        test::<AccessModifierDeclarations>().expect_offense(indoc! {"
            class Foo
              public def bar; end
              ^^^^^^ `public` should not be inlined in method definitions.
            end
        "});
    }

    #[test]
    fn group_flags_module_function_def() {
        test::<AccessModifierDeclarations>().expect_offense(indoc! {"
            module Foo
              module_function def bar; end
              ^^^^^^^^^^^^^^^ `module_function` should not be inlined in method definitions.
            end
        "});
    }

    #[test]
    fn group_accepts_bare_private_with_def() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private
              def bar; end
              def baz; end
            end
        "});
    }

    #[test]
    fn group_accepts_bare_private_alone() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private
            end
        "});
    }

    #[test]
    fn group_flags_only_last_in_consecutive_private_def_run() {
        // Two consecutive `private def …` — only the last is flagged.
        test::<AccessModifierDeclarations>().expect_offense(indoc! {"
            class Foo
              private def bar; end
              private def baz; end
              ^^^^^^^ `private` should not be inlined in method definitions.
            end
        "});
    }

    // ── AllowModifiersOnSymbols ───────────────────────────────────────────────

    #[test]
    fn group_allows_private_symbol_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private :bar, :baz
            end
        "});
    }

    #[test]
    fn group_allows_private_splat_array_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private *%i[qux quux]
            end
        "});
    }

    #[test]
    fn group_allows_private_splat_const_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private *METHOD_NAMES
            end
        "});
    }

    #[test]
    fn group_flags_private_symbol_when_not_allowed() {
        test::<AccessModifierDeclarations>()
            .with_options(&no_symbols_opts())
            .expect_offense(indoc! {"
                class Foo
                  private :bar, :baz
                  ^^^^^^^ `private` should not be inlined in method definitions.
                end
            "});
    }

    // ── AllowModifiersOnAttrs ─────────────────────────────────────────────────

    #[test]
    fn group_allows_private_attr_reader_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private attr_reader :bar
            end
        "});
    }

    #[test]
    fn group_allows_private_attr_writer_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private attr_writer :bar
            end
        "});
    }

    #[test]
    fn group_allows_private_attr_accessor_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private attr_accessor :bar
            end
        "});
    }

    #[test]
    fn group_allows_private_attr_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private attr :bar
            end
        "});
    }

    #[test]
    fn group_flags_private_attr_reader_when_not_allowed() {
        test::<AccessModifierDeclarations>()
            .with_options(&no_attrs_opts())
            .expect_offense(indoc! {"
                class Foo
                  private attr_reader :bar
                  ^^^^^^^ `private` should not be inlined in method definitions.
                end
            "});
    }

    // ── AllowModifiersOnAliasMethod ───────────────────────────────────────────

    #[test]
    fn group_allows_private_alias_method_by_default() {
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            class Foo
              private alias_method :bar, :foo
            end
        "});
    }

    #[test]
    fn group_flags_private_alias_method_when_not_allowed() {
        test::<AccessModifierDeclarations>()
            .with_options(&no_alias_opts())
            .expect_offense(indoc! {"
                class Foo
                  private alias_method :bar, :foo
                  ^^^^^^^ `private` should not be inlined in method definitions.
                end
            "});
    }

    // ── inline style ──────────────────────────────────────────────────────────

    #[test]
    fn inline_flags_bare_private_with_following_def() {
        test::<AccessModifierDeclarations>()
            .with_options(&inline_opts())
            .expect_offense(indoc! {"
                class Foo
                  private
                  ^^^^^^^ `private` should be inlined in method definitions.
                  def bar; end
                  def baz; end
                end
            "});
    }

    #[test]
    fn inline_accepts_private_def() {
        test::<AccessModifierDeclarations>()
            .with_options(&inline_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  private def bar; end
                  private def baz; end
                end
            "});
    }

    #[test]
    fn inline_accepts_bare_private_without_following_def() {
        // A trailing bare `private` with no def after it is not flagged.
        test::<AccessModifierDeclarations>()
            .with_options(&inline_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  private
                end
            "});
    }

    #[test]
    fn inline_accepts_bare_private_followed_only_by_modifier() {
        // `private` followed by another bare `protected` but no `def` — not flagged.
        test::<AccessModifierDeclarations>()
            .with_options(&inline_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  private
                  protected
                end
            "});
    }

    // ── false-positive guards ─────────────────────────────────────────────────

    #[test]
    fn group_skips_private_inside_block() {
        // Access modifier inside a block is skipped (parent is Block).
        test::<AccessModifierDeclarations>().expect_no_offenses(indoc! {"
            foo { private def bar; end }
        "});
    }
}
