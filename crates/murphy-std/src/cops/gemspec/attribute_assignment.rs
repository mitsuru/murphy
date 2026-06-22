//! `Gemspec/AttributeAssignment` — use a consistent style for assigning a
//! gemspec attribute. Inside a `Gem::Specification.new do |spec|` block, when
//! the same attribute is set both *directly* (`spec.metadata = { ... }`) and via
//! *indexed* assignment (`spec.metadata['key'] = value`), the indexed form is
//! flagged. The cop runs only on `*.gemspec` files; the host applies the per-cop
//! `Include` from `config/default.yml`, so this cop never inspects the filename
//! itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/AttributeAssignment
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_new_investigation`: two whole-AST passes
//!   (`source_assignments` and `source_indexed_assignments`), then flag every
//!   indexed-assignment node whose attribute *also* appears as a direct
//!   assignment (`assignments.keys.intersection(indexed_assignments.keys)`).
//!
//!   Block-variable scoping: RuboCop's `assignment_method_declarations` /
//!   `indexed_assignment_method_declarations` patterns restrict the receiver to
//!   `(lvar {#match_block_variable_name? :_1 :it})` — the lvar must name the
//!   `Gem::Specification.new` block parameter, or be the implicit `_1` / `it`.
//!   We extract the block-variable name from the first `Gem::Specification.new`
//!   (or `::Gem::Specification`) block (`do |spec|` → "spec") and accept that
//!   name plus the always-allowed "_1" / "it" alternatives. The `_1` / `it`
//!   alternatives are accepted unconditionally — RuboCop's pattern union admits
//!   them even when no gemspec block is present (verified against standalone
//!   rubocop 1.87.0: `_1` inside a non-gemspec `foo do … end` still matches).
//!
//!   Direct (regular) pass: a `Send` whose receiver is *directly* the block
//!   lvar and whose selector `assignment_method?`s (ends with `=`, not a
//!   comparison — `cx.is_assignment_method`). RuboCop strips the trailing `=`
//!   and keys by the bare attribute name (`metadata= → :metadata`); we collect
//!   the same bare-name set. `[]=` strips to `[]`, which can never intersect an
//!   indexed attribute name, so it never produces a false offense.
//!
//!   Indexed pass: a `Send` with selector `[]=` whose receiver is itself an
//!   *argument-free* `Send` on a direct block lvar (`spec.metadata['k'] = v`),
//!   with a literal key. The literal check uses `cx.is_literal`, which matches
//!   NodePattern's shallow `literal?` predicate (NOT rubocop-ast's recursive
//!   `Node#literal?`): verified against rubocop 1.87.0, `spec.metadata[[FOO]]`
//!   and `spec.metadata[{ a: x }]` (shallow array/hash) ARE flagged while
//!   `spec.metadata[FOO]` (const) is NOT. The argument-free check mirrors
//!   RuboCop's exact-arity `(send (lvar X) _)`: `spec.foo(1)['k'] = …` does not
//!   match (verified). The index literal is taken raw — no parenthesis
//!   unwrapping — so `spec.metadata[('a')] = …` (a `Begin` index) is not a
//!   literal and is not matched, matching NodePattern.
//!
//!   Offense range is `cx.range(node)` (the whole `spec.attr[key] = value`
//!   statement). For a single-line assignment this equals RuboCop's
//!   `column...last_column` (verified byte-for-byte). The only accepted,
//!   near-impossible-in-a-gemspec divergence is a multi-line indexed assignment
//!   (value spanning lines): RuboCop trims to the first line's `last_column`,
//!   we keep the full node range — the same family as `Gemspec/DuplicatedAssignment`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct AttributeAssignment;

#[cop(
    name = "Gemspec/AttributeAssignment",
    description = "Use consistent style for Gemspec attributes assignment.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl AttributeAssignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // The set of lvar names whose `.foo = ` / `.attr[k] = ` assignments
        // RuboCop attributes to the gemspec block: the block parameter name (if
        // any) plus the implicit `_1` / `it`, which are always in the pattern's
        // alternation.
        let mut accepted: Vec<&str> = vec!["_1", "it"];
        if let Some(name) = gem_specification_block_var(cx)
            && !accepted.contains(&name)
        {
            accepted.push(name);
        }

        // Pass 1: bare attribute names of direct assignments (`spec.foo = …`),
        // RuboCop's `source_assignments` keyed by `method_name.delete_suffix("=")`.
        let mut direct_attrs: Vec<&str> = Vec::new();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            if let Some(attr) = direct_assignment_attr(node, &accepted, cx)
                && !direct_attrs.contains(&attr)
            {
                direct_attrs.push(attr);
            }
        }

        // Pass 2: indexed assignments (`spec.attr['k'] = v`), in source order.
        // Flag each whose attribute also appears in `direct_attrs` (RuboCop's
        // `keys.intersection`). The aggregator sorts by position; emitting in
        // walk order keeps test carets in source order.
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(attr) = indexed_assignment_attr(node, &accepted, cx) else {
                continue;
            };
            if direct_attrs.contains(&attr) {
                cx.emit_offense(
                    cx.range(node),
                    "Use consistent style for Gemspec attributes assignment.",
                    None,
                );
            }
        }
    }
}

/// The bare attribute name of `node` if it is a direct gemspec attribute
/// assignment — RuboCop's `(send (lvar X) _ ...)` filtered by
/// `assignment_method?`, with the trailing `=` stripped, where `X` is an
/// accepted block-variable name. `None` otherwise.
fn direct_assignment_attr<'a>(node: NodeId, accepted: &[&str], cx: &Cx<'a>) -> Option<&'a str> {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    let receiver = cx.call_receiver(node).get()?;
    if !is_accepted_lvar(receiver, accepted, cx) {
        return None;
    }
    if !cx.is_assignment_method(node) {
        return None;
    }
    // RuboCop: `method_name.to_s.delete_suffix('=')`.
    cx.method_name(node).map(|m| m.strip_suffix('=').unwrap_or(m))
}

/// The attribute name of `node` if it is an indexed gemspec assignment —
/// RuboCop's `(send (send (lvar X) attr) :[]= literal? _)`, where `X` is an
/// accepted block-variable name. `None` otherwise.
fn indexed_assignment_attr<'a>(node: NodeId, accepted: &[&str], cx: &Cx<'a>) -> Option<&'a str> {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(node)? != "[]=" {
        return None;
    }
    let receiver = cx.call_receiver(node).get()?;
    // The receiver must be a `(send (lvar X) attr)` — a single, argument-free
    // send on the block lvar. `spec.foo.bar['k'] = ` (two levels) and
    // `spec.foo(1)['k'] = ` (inner send has an arg) do not match RuboCop's
    // exact-arity `(send (lvar X) _)` pattern.
    if !matches!(cx.kind(receiver), NodeKind::Send { .. }) {
        return None;
    }
    if !cx.call_arguments(receiver).is_empty() {
        return None;
    }
    let inner_receiver = cx.call_receiver(receiver).get()?;
    if !is_accepted_lvar(inner_receiver, accepted, cx) {
        return None;
    }
    // `[]=` args are `[key, value]`; the key must be a literal (NodePattern's
    // shallow `literal?` — `cx.is_literal`). Taken raw, no paren unwrapping.
    let key = *cx.call_arguments(node).first()?;
    if !cx.is_literal(key) {
        return None;
    }
    cx.method_name(receiver)
}

/// True when `node` is `(lvar X)` for an accepted block-variable name `X`.
fn is_accepted_lvar(node: NodeId, accepted: &[&str], cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => accepted.contains(&cx.symbol_str(sym)),
        _ => false,
    }
}

/// The explicit single block-parameter name of the first
/// `Gem::Specification.new do |x| ... end` block. `None` if no such block
/// exists or its `(args ...)` is not exactly one plain argument. Mirrors
/// RuboCop's `(block ... (args (arg $_)) ...)`, which matches explicit-param
/// blocks only — numbered (`_1`) and `it` blocks are handled by the always-
/// allowed `_1` / `it` alternatives in the caller's accepted-name set, exactly
/// as RuboCop's `{#match_block_variable_name? :_1 :it}` alternation does.
fn gem_specification_block_var<'a>(cx: &Cx<'a>) -> Option<&'a str> {
    let root = cx.root();
    for node in std::iter::once(root).chain(cx.descendants(root)) {
        if !matches!(cx.kind(node), NodeKind::Block { .. }) {
            continue;
        }
        if !is_gem_specification_call(cx.block_call(node).get(), cx) {
            continue;
        }
        let args = cx.block_arguments(node).get()?;
        let NodeKind::Args(list) = *cx.kind(args) else {
            continue;
        };
        // RuboCop's `(args (arg $_))` requires exactly one plain arg.
        if let [only] = cx.list(list)
            && let NodeKind::Arg(sym) = *cx.kind(*only)
        {
            return Some(cx.symbol_str(sym));
        }
    }
    None
}

/// True when `call` is `Gem::Specification.new` (or `::Gem::Specification.new`).
fn is_gem_specification_call(call: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(call) = call else {
        return false;
    };
    if cx.method_name(call) != Some("new") {
        return false;
    }
    let Some(receiver) = cx.call_receiver(call).get() else {
        return false;
    };
    is_gem_specification_const(receiver, cx)
}

/// True when `node` is the const `Gem::Specification` or `::Gem::Specification`,
/// mirroring RuboCop's `(const (const {cbase nil?} :Gem) :Specification)`.
fn is_gem_specification_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "Specification" {
        return false;
    }
    let Some(scope) = scope.get() else {
        return false;
    };
    cx.is_global_const(scope, "Gem")
}

murphy_plugin_api::submit_cop!(AttributeAssignment);

#[cfg(test)]
mod tests {
    use super::AttributeAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_indexed_when_direct_assignment_also_present() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = { 'key' => 'value' }
              spec.metadata['rubygems_mfa_required'] = 'true'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn flags_each_indexed_assignment_after_direct() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata['a'] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
              spec.metadata['b'] = '2'
              ^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn allows_indexed_only_without_direct_assignment() {
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata['rubygems_mfa_required'] = 'true'
              spec.metadata['homepage'] = 'https://example.com'
            end
        "#});
    }

    #[test]
    fn allows_direct_only_without_indexed_assignment() {
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = { 'a' => 'b' }
              spec.name = 'foo'
            end
        "#});
    }

    #[test]
    fn allows_indexed_with_non_literal_const_key() {
        // `FOO` const key is not a NodePattern `literal?` → indexed pass skips it.
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata[FOO] = '1'
            end
        "#});
    }

    #[test]
    fn allows_indexed_with_non_literal_lvar_key() {
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata[key] = '1'
            end
        "#});
    }

    #[test]
    fn flags_symbol_and_integer_literal_keys() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata[:sym] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
              spec.metadata[0] = '2'
              ^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn flags_shallow_array_literal_key() {
        // NodePattern `literal?` is shallow: an `Array` node matches even if its
        // elements are non-literal. Verified against rubocop 1.87.0.
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata[[FOO]] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn allows_different_attributes() {
        // direct `metadata`, indexed `requirements` → no intersection.
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.requirements['a'] = '1'
            end
        "#});
    }

    #[test]
    fn allows_indexed_receiver_not_block_var() {
        // `other.metadata[...]` receiver is `(lvar other)`/send, not the block var.
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              other.metadata['a'] = '1'
            end
        "#});
    }

    #[test]
    fn allows_argful_inner_receiver() {
        // `spec.foo(x)['a'] = …` — inner send has an arg, breaking the
        // exact-arity `(send (lvar X) _)` pattern. Also `foo` != `metadata`.
        test::<AttributeAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata(x)['a'] = '1'
            end
        "#});
    }

    #[test]
    fn flags_with_alternate_block_var_name() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |s|
              s.metadata = {}
              s.metadata['a'] = '1'
              ^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn flags_with_numbered_param_underscore_one() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do
              _1.metadata = {}
              _1.metadata['a'] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn flags_with_it_param() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do
              it.metadata = {}
              it.metadata['a'] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }

    #[test]
    fn flags_with_cbase_gem_specification() {
        test::<AttributeAssignment>().expect_offense(indoc! {r#"
            ::Gem::Specification.new do |spec|
              spec.metadata = {}
              spec.metadata['a'] = '1'
              ^^^^^^^^^^^^^^^^^^^^^^^^ Use consistent style for Gemspec attributes assignment.
            end
        "#});
    }
}
