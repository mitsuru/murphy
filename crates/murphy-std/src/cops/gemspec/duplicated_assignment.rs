//! `Gemspec/DuplicatedAssignment` — an attribute-assignment method call should
//! be listed only once in a gemspec. Inside a `Gem::Specification.new do |spec|`
//! block, a `spec.foo = ...` setter or a `spec.attr[key] = ...` indexed
//! assignment repeated with the same name/key is an unintended overwrite. The
//! cop runs only on `*.gemspec` files; the host applies the per-cop `Include`
//! from `config/default.yml`, so this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/DuplicatedAssignment
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Two disjoint passes mirror RuboCop's `process_assignment_method_nodes` and
//!   `process_indexed_assignment_method_nodes`.
//!
//!   Block-variable scoping: RuboCop's `assignment_method_declarations` /
//!   `indexed_assignment_method_declarations` patterns restrict the receiver to
//!   `(lvar {#match_block_variable_name? :_1 :it})` — the lvar must name the
//!   `Gem::Specification.new` block parameter, or be the implicit `_1`/`it`.
//!   We extract the block-variable name from the first `Gem::Specification.new`
//!   (or `::Gem::Specification`) block (`do |spec|` → "spec"; numblock → "_1";
//!   itblock → "it") and accept that name plus the always-allowed "_1"/"it"
//!   alternatives. This is parity-critical: `config.foo = 1; config.foo = 2`
//!   inside the block (config != spec) is NOT flagged — verified against
//!   standalone rubocop 1.87.0.
//!
//!   Regular pass: a `Send` whose receiver is *directly* the block lvar and
//!   whose selector `assignment_method?`s (ends with `=`, not a comparison —
//!   `cx.is_assignment_method`). Grouped by `method_name`; each duplicate after
//!   the first is flagged citing the first occurrence's 1-based line. RuboCop's
//!   `(send (lvar X) _ ...)` matches a direct-lvar receiver only, so
//!   `spec["a"] = 1; spec["b"] = 2` (direct index on the var) lands here under
//!   selector `[]=` and groups together regardless of key — RuboCop flags this
//!   too (verified), so we preserve the quirk rather than special-case it out.
//!
//!   Indexed pass: a `Send` with selector `[]=` whose receiver is itself an
//!   *argument-free* `Send` on a direct block lvar (`spec.metadata["k"] = v`),
//!   with a literal key (`cx.is_literal`, mirroring the node-pattern
//!   `literal?`). The argument-free check mirrors RuboCop's exact-arity
//!   `(send (lvar X) _)`: `spec.foo(1)["k"] = ...` does not match (verified
//!   against rubocop 1.87.0). Grouped by `(inner_method_name, key)`. String
//!   keys are grouped by *value* (`cx.string_str`), so `"k"` and `'k'` collide
//!   and are flagged — matching RuboCop, which groups by structural `Node#==`.
//!   Non-string literal keys are grouped by `raw_source`. The offense message
//!   embeds the duplicate node's key via `raw_source` (so the cited form is the
//!   literal as written, e.g. `metadata['k']=`), even though grouping is by
//!   value — matches RuboCop.
//!
//!   Offense range is `cx.range(node)` (the whole statement). For a single-line
//!   assignment this equals RuboCop's `column...last_column`. Two negligible,
//!   near-impossible gemspec shapes are the only accepted divergences:
//!   (1) a multi-line assignment (a value spanning lines) — RuboCop trims the
//!   range to the first line's `last_column`, we keep the full node range;
//!   (2) non-string literal index keys are grouped by `raw_source`, so exotic
//!   equivalent forms (e.g. `:k` vs `:'k'`) under-group where RuboCop's
//!   structural `Node#==` would collide them — string keys (the only realistic
//!   gemspec index key) are unaffected because they group by value.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DuplicatedAssignment;

#[cop(
    name = "Gemspec/DuplicatedAssignment",
    description = "An attribute assignment method calls should be listed only once in a gemspec.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicatedAssignment {
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

        // Regular assignment pass — group `(send (lvar X) sel= ...)` by selector.
        let mut groups: Vec<(&str, Vec<NodeId>)> = Vec::new();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(method) = regular_assignment_method(node, &accepted, cx) else {
                continue;
            };
            push_group(&mut groups, method, node);
        }
        for (method, nodes) in &groups {
            emit_dupes(nodes, cx, |_node| (*method).to_owned());
        }

        // Indexed assignment pass — group `(send (send (lvar X) attr) :[]= key v)`
        // by `(attr, key-value)`.
        let mut idx_groups: Vec<(String, Vec<NodeId>)> = Vec::new();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(key) = indexed_assignment_group_key(node, &accepted, cx) else {
                continue;
            };
            push_group(&mut idx_groups, key, node);
        }
        for (_key, nodes) in &idx_groups {
            emit_dupes(nodes, cx, |node| indexed_assignment_label(node, cx));
        }
    }
}

/// Flag every node after the first in a duplicate group, citing the first
/// occurrence's 1-based line. `label` builds the back-ticked assignment name
/// embedded in the message for the offending node.
fn emit_dupes(nodes: &[NodeId], cx: &Cx<'_>, label: impl Fn(NodeId) -> String) {
    if nodes.len() < 2 {
        return;
    }
    let first_line = line_of(cx, cx.range(nodes[0]).start);
    for &node in &nodes[1..] {
        let assignment = label(node);
        let message = format!(
            "`{assignment}` method calls already given on line {first_line} of the gemspec."
        );
        cx.emit_offense(cx.range(node), &message, None);
    }
}

/// Insert `node` into the group keyed by `key`, preserving source/insertion
/// order so `nodes.first()` is the first occurrence.
fn push_group<K: PartialEq>(groups: &mut Vec<(K, Vec<NodeId>)>, key: K, node: NodeId) {
    if let Some(entry) = groups.iter_mut().find(|(k, _)| *k == key) {
        entry.1.push(node);
    } else {
        groups.push((key, vec![node]));
    }
}

/// The selector of `node` if it is a regular gemspec attribute assignment —
/// RuboCop's `(send (lvar X) _ ...)` filtered by `assignment_method?`, where `X`
/// is an accepted block-variable name. `None` otherwise.
fn regular_assignment_method<'a>(
    node: NodeId,
    accepted: &[&str],
    cx: &Cx<'a>,
) -> Option<&'a str> {
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
    cx.method_name(node)
}

/// The grouping key `"attr\0<key-value>"` of `node` if it is an indexed gemspec
/// assignment — RuboCop's `(send (send (lvar X) attr) :[]= literal? _)`, where
/// `X` is an accepted block-variable name. `None` otherwise.
fn indexed_assignment_group_key(
    node: NodeId,
    accepted: &[&str],
    cx: &Cx<'_>,
) -> Option<String> {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(node)? != "[]=" {
        return None;
    }
    let receiver = cx.call_receiver(node).get()?;
    // The receiver must be a `(send (lvar X) attr)` — a single, argument-free
    // send on the block lvar. `spec.foo.bar["k"] = ` (two levels) and
    // `spec.foo(1)["k"] = ` (inner send has an arg) do not match RuboCop's
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
    let attr = cx.method_name(receiver)?;
    // `[]=` args are `[key, value]`; the key must be a literal (RuboCop's
    // `literal?`).
    let key = *cx.call_arguments(node).first()?;
    if !cx.is_literal(key) {
        return None;
    }
    // Group string keys by VALUE so `"k"` and `'k'` collide (RuboCop's
    // structural `Node#==`); other literal keys by raw source.
    let key_repr = match *cx.kind(key) {
        NodeKind::Str(id) => cx.string_str(id).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    };
    Some(format!("{attr}\0{key_repr}"))
}

/// The back-ticked assignment label for an indexed-assignment offense, e.g.
/// `metadata['k']=`. Uses the *raw source* of the key node (as written), even
/// though grouping is by value — matches RuboCop's `register_offense`.
fn indexed_assignment_label(node: NodeId, cx: &Cx<'_>) -> String {
    let receiver = cx
        .call_receiver(node)
        .get()
        .expect("indexed-assignment node has a receiver send");
    let attr = cx
        .method_name(receiver)
        .expect("indexed-assignment receiver has a selector");
    let key = *cx
        .call_arguments(node)
        .first()
        .expect("indexed-assignment node has a key argument");
    let key_src = cx.raw_source(cx.range(key));
    format!("{attr}[{key_src}]=")
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
/// allowed `_1`/`it` alternatives in the caller's accepted-name set, exactly as
/// RuboCop's `{#match_block_variable_name? :_1 :it}` alternation does.
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

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(DuplicatedAssignment);

#[cfg(test)]
mod tests {
    use super::DuplicatedAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_setter() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "x"
              spec.name = "y"
              ^^^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn allows_single_setter() {
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "x"
            end
        "#});
    }

    #[test]
    fn allows_distinct_setters() {
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "x"
              spec.version = "1.0"
            end
        "#});
    }

    #[test]
    fn triple_setter_flags_each_after_first_citing_first_line() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = "a"
              spec.name = "b"
              ^^^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
              spec.name = "c"
              ^^^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn allows_append_methods() {
        // `<<` and non-assignment method calls are intended appends, never flagged.
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.requirements << "libmagick, v6.0"
              spec.requirements << "A good graphics card"
            end
        "#});
    }

    #[test]
    fn allows_duplicate_add_dependency() {
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency("parallel", "~> 1.10")
              spec.add_dependency("parser", ">= 2.3.3.1")
            end
        "#});
    }

    #[test]
    fn flags_duplicate_indexed_assignment() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata["key"] = "a"
              spec.metadata["key"] = "b"
              ^^^^^^^^^^^^^^^^^^^^^^^^^^ `metadata["key"]=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn allows_single_indexed_assignment() {
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata["key"] = "value"
            end
        "#});
    }

    #[test]
    fn allows_distinct_indexed_keys() {
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata["a"] = "1"
              spec.metadata["b"] = "2"
            end
        "#});
    }

    #[test]
    fn flags_indexed_key_quote_style_mismatch_using_raw_source_label() {
        // Grouping is by VALUE so `"k"` and `'k'` collide; the message embeds the
        // duplicate node's RAW key (`'k'`). Verified against rubocop 1.87.0.
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata["k"] = "a"
              spec.metadata['k'] = "b"
              ^^^^^^^^^^^^^^^^^^^^^^^^ `metadata['k']=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn allows_non_literal_indexed_key() {
        // `literal?` excludes a variable/method key; neither pass matches.
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.metadata[key] = "a"
              spec.metadata[key] = "b"
            end
        "#});
    }

    #[test]
    fn does_not_flag_assignments_on_other_local_variable() {
        // `config` is not the block variable `spec`; RuboCop scopes the receiver
        // to the gemspec block parameter, so this is NOT flagged.
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              config = build
              config.foo = 1
              config.foo = 2
            end
        "#});
    }

    #[test]
    fn allows_indexed_assignment_when_inner_send_has_arguments() {
        // RuboCop's `(send (send (lvar X) _) :[]= ...)` requires the inner send
        // to be argument-free; `spec.foo(1)["k"]=` does not match. Verified
        // against rubocop 1.87.0 (no offenses).
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.foo(1)["k"] = "a"
              spec.foo(1)["k"] = "b"
            end
        "#});
    }

    #[test]
    fn flags_direct_index_on_block_var_under_index_selector() {
        // `spec["a"]=` / `spec["b"]=` have a DIRECT lvar receiver → regular pass,
        // selector `[]=`; they group together regardless of key. RuboCop flags
        // this (verified) — quirk preserved.
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec["a"] = 1
              spec["b"] = 2
              ^^^^^^^^^^^^^ `[]=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn flags_setter_in_numbered_parameter_block() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do
              _1.name = "x"
              _1.name = "y"
              ^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn flags_setter_in_it_parameter_block() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            Gem::Specification.new do
              it.name = "x"
              it.name = "y"
              ^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn flags_with_fully_qualified_constant() {
        test::<DuplicatedAssignment>().expect_offense(indoc! {r#"
            ::Gem::Specification.new do |spec|
              spec.name = "x"
              spec.name = "y"
              ^^^^^^^^^^^^^^^ `name=` method calls already given on line 2 of the gemspec.
            end
        "#});
    }

    #[test]
    fn ignores_assignments_outside_gem_specification_block() {
        // No `Gem::Specification.new` block → no accepted block var → nothing
        // matches (other than the always-allowed `_1`/`it`, absent here).
        test::<DuplicatedAssignment>().expect_no_offenses(indoc! {r#"
            spec.name = "x"
            spec.name = "y"
        "#});
    }
}
