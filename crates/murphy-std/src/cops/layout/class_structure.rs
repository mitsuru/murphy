//! `Layout/ClassStructure` — enforces a configured order of definitions within
//! a class body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ClassStructure
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_class` (aliased `on_sclass`) detection: walks the class
//!   body elements in source order, classifies each into a category, looks up
//!   the category's index in `ExpectedOrder`, and flags any node whose index is
//!   smaller than the *running* `previous` index. RuboCop assigns `previous =
//!   index` unconditionally after every node (`class_structure.rb:201`), so
//!   after a regression resets the running index, a later node at a
//!   higher-but-valid index is not re-flagged. This is a stateful single pass,
//!   faithful to upstream by design (NOT max-index tracking).
//!
//!   Classification mirrors RuboCop's `classify` / `find_send_node_category` /
//!   `humanize_node`:
//!     * `block` → classify its `send_node`.
//!     * `send` → `find_send_node_category`: a category from `Categories` (or
//!       the bare method name) gains a visibility prefix
//!       (`<visibility>_<key>`); the visibility-qualified key is used only if it
//!       is in `ExpectedOrder`, otherwise the unqualified key.
//!     * `defs` (`def self.x`) → `public_class_methods` (no visibility applied).
//!     * `def initialize` → `initializer`.
//!     * other instance `def` → `<visibility>_methods`.
//!     * `casgn` → `constants`.
//!   Visibility is resolved per RuboCop's `VisibilityHelp#node_visibility`
//!   (`VISIBILITY_SCOPES = {private, protected, public}` only — NOT
//!   `module_function`): inline-on-def (`private def foo`), inline-on-name
//!   (`private :foo`), and block (nearest preceding bare modifier), defaulting
//!   to `public`.
//!
//!   `ignore?` is honoured: a node is skipped when its classification is nil,
//!   ends with `=` (setters), is not present in `ExpectedOrder`, or is a
//!   `private_constant`-marked constant. This is why `attr_reader` etc. pass
//!   under the default config (no `attribute_macros` category defined).
//!
//!   Default config matches upstream: `Enabled: false`, `SafeAutoCorrect:
//!   false`, `Categories: {module_inclusion: [include, prepend, extend]}`,
//!   `ExpectedOrder: [module_inclusion, constants, public_class_methods,
//!   initializer, public_methods, protected_methods, private_methods]`.
//!
//!   Autocorrect (murphy-kse6) ports RuboCop's unsafe node-swap corrector:
//!   the offending node is swapped one position upward past the nearest
//!   non-ignored left sibling with a different classification
//!   (`source_range_with_comment` + `insert_before`/`remove`). The swap carries
//!   contiguous own-line comments above each node (Murphy's
//!   `range_with_comments_and_lines`, mirroring RuboCop's `begin_pos_with_comment`
//!   / `end_position_for` whole-line convention). Like upstream, the correction
//!   is unsafe (`SafeAutoCorrect: false`, `safe_autocorrect = false` metadata)
//!   and only runs under `-A`: visibility modifiers (`private`/`public` bare
//!   sends) stay in place while methods move, so repeated passes bubble a node
//!   left until the order converges. Dynamic constants (a `casgn` whose value
//!   is a non-`freeze` method call, or `freeze` on a non-literal receiver) are
//!   offense-only, matching RuboCop's `dynamic_constant?` guard; heredoc
//!   constants are also offense-only because the heredoc body lives outside the
//!   node's line range and a whole-line swap would corrupt source.
//! ```

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, cop};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct ClassStructure;

/// `ExpectedOrder` (ordered list of category names) + `Categories` (macro →
/// category map). Hand-rolled because `#[derive(CopOptions)]` does not model
/// nested maps; defaults mirror `config/default.yml`.
#[derive(Clone, Debug)]
pub struct ClassStructureOptions {
    /// Ordered category names.
    pub expected_order: Vec<String>,
    /// Category name → the macro method names mapped into it.
    pub categories: BTreeMap<String, Vec<String>>,
}

impl Default for ClassStructureOptions {
    fn default() -> Self {
        let expected_order = [
            "module_inclusion",
            "constants",
            "public_class_methods",
            "initializer",
            "public_methods",
            "protected_methods",
            "private_methods",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let mut categories = BTreeMap::new();
        categories.insert(
            "module_inclusion".to_string(),
            vec![
                "include".to_string(),
                "prepend".to_string(),
                "extend".to_string(),
            ],
        );

        Self {
            expected_order,
            categories,
        }
    }
}

impl CopOptions for ClassStructureOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        // Error surface mirrors `#[derive(CopOptions)]`: non-object root →
        // `not_an_object`; per-field shape mismatches → `type_mismatch` with a
        // path-qualified field name. Absent fields fall back to defaults.
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let mut result = Self::default();

        if let Some(order_value) = obj.get("ExpectedOrder") {
            let array = order_value
                .as_array()
                .ok_or_else(|| ConfigError::type_mismatch("ExpectedOrder", "array of strings"))?;
            let mut order = Vec::with_capacity(array.len());
            for (i, elem) in array.iter().enumerate() {
                let s = elem
                    .as_str()
                    .ok_or_else(|| ConfigError::type_mismatch(format!("ExpectedOrder[{i}]"), "string"))?;
                order.push(s.to_string());
            }
            result.expected_order = order;
        }

        if let Some(categories_value) = obj.get("Categories") {
            let categories_obj = categories_value
                .as_object()
                .ok_or_else(|| ConfigError::type_mismatch("Categories", "object"))?;
            let mut categories = BTreeMap::new();
            for (key, values) in categories_obj {
                let array = values.as_array().ok_or_else(|| {
                    ConfigError::type_mismatch(format!("Categories.{key}"), "array of strings")
                })?;
                let mut names = Vec::with_capacity(array.len());
                for (i, elem) in array.iter().enumerate() {
                    let s = elem.as_str().ok_or_else(|| {
                        ConfigError::type_mismatch(format!("Categories.{key}[{i}]"), "string")
                    })?;
                    names.push(s.to_string());
                }
                categories.insert(key.clone(), names);
            }
            result.categories = categories;
        }

        Ok(result)
    }

    fn to_config_json(&self) -> String {
        let order: Vec<serde_json::Value> = self
            .expected_order
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect();
        let categories: serde_json::Map<String, serde_json::Value> = self
            .categories
            .iter()
            .map(|(k, vs)| {
                let arr: Vec<serde_json::Value> = vs
                    .iter()
                    .map(|v| serde_json::Value::String(v.clone()))
                    .collect();
                (k.clone(), serde_json::Value::Array(arr))
            })
            .collect();
        let mut top = serde_json::Map::new();
        top.insert("ExpectedOrder".to_string(), serde_json::Value::Array(order));
        top.insert(
            "Categories".to_string(),
            serde_json::Value::Object(categories),
        );
        serde_json::Value::Object(top).to_string()
    }
}

#[cop(
    name = "Layout/ClassStructure",
    description = "Enforces a configured order of definitions within a class body.",
    default_severity = "warning",
    default_enabled = false,
    safe_autocorrect = false,
    options = ClassStructureOptions
)]
impl ClassStructure {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// `on_class(class_node)` — walk the class elements in order, flagging any whose
/// category index regresses.
fn check(class_node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ClassStructureOptions>();
    let elements = class_elements(class_node, cx);

    let mut previous: i64 = -1;
    for &node in &elements {
        let Some(category) = classify(node, cx, &opts) else {
            continue;
        };
        if ignore(node, &category, cx, &opts) {
            continue;
        }
        let Some(index) = opts.expected_order.iter().position(|c| *c == category) else {
            continue;
        };
        let index = index as i64;
        if index < previous {
            let prev_category = &opts.expected_order[previous as usize];
            let message =
                format!("`{category}` is supposed to appear before `{prev_category}`.");
            cx.emit_offense(cx.range(node), &message, None);
            autocorrect(node, &category, cx, &opts);
        }
        previous = index;
    }
}

/// `autocorrect(corrector, node)` — swap the offending node one position upward
/// past the nearest swappable left sibling, carrying contiguous own-line
/// comments (RuboCop's `source_range_with_comment` + `insert_before`/`remove`).
fn autocorrect(node: NodeId, category: &str, cx: &Cx<'_>, opts: &ClassStructureOptions) {
    // `dynamic_constant?(node)` suppresses the correction entirely: every
    // sibling would be ignored, so there is no swap target.
    if dynamic_constant(node, cx) {
        return;
    }
    let Some(previous) = swap_target(node, category, cx, opts) else {
        return;
    };
    let current_range = source_range_with_comment(node, cx);
    let previous_range = source_range_with_comment(previous, cx);
    // Whole-line ranges of two same-line nodes coincide; swapping them would
    // delete both lines' text. Upstream never sees this shape (its ranges are
    // leading-newline based), so bail out instead of corrupting source.
    if previous_range.end > current_range.start
        || current_range.start < previous_range.start
        || current_range.end <= current_range.start
    {
        return;
    }
    // Heredoc bodies live outside the opener's line range. A whole-line swap
    // of a heredoc constant would strand the body/terminator, so stay
    // offense-only when any heredoc token sits inside the swap span.
    if swap_span_contains_heredoc(previous_range.start, current_range.end, cx) {
        return;
    }
    let text = cx.raw_source(current_range).to_string();
    if text.is_empty() {
        return;
    }
    cx.emit_edit(
        Range {
            start: previous_range.start,
            end: previous_range.start,
        },
        &text,
    );
    cx.emit_edit(current_range, "");
}

/// `node.left_siblings.reverse.find { !ignore_for_autocorrect?(node, sibling) }`.
fn swap_target(
    node: NodeId,
    category: &str,
    cx: &Cx<'_>,
    opts: &ClassStructureOptions,
) -> Option<NodeId> {
    let mut sibling = cx.left_sibling(node);
    while let Some(current) = sibling.get() {
        if !ignore_for_autocorrect(node, category, current, cx, opts) {
            return Some(current);
        }
        sibling = cx.left_sibling(current);
    }
    None
}

/// `ignore_for_autocorrect?(node, sibling)`.
fn ignore_for_autocorrect(
    node: NodeId,
    category: &str,
    sibling: NodeId,
    cx: &Cx<'_>,
    opts: &ClassStructureOptions,
) -> bool {
    if dynamic_constant(node, cx) {
        return true;
    }
    let Some(sibling_class) = classify(sibling, cx, opts) else {
        return true;
    };
    if sibling_class == category {
        return true;
    }
    ignore(sibling, &sibling_class, cx, opts)
}

/// `dynamic_constant?(node)` — a namespace-less `casgn` whose value is a method
/// call, except `freeze` on a recursive basic literal receiver.
fn dynamic_constant(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Casgn { scope, value, .. } = *cx.kind(node) else {
        return false;
    };
    if scope.get().is_some() {
        return false;
    }
    let Some(expression) = value.get() else {
        return false;
    };
    // RuboCop checks `send_type?` only — `csend` and block calls are not
    // dynamic constants.
    if !matches!(*cx.kind(expression), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(expression) != Some("freeze") {
        return true;
    }
    // `freeze` with no receiver (`CONST = freeze`) has a nil receiver, and
    // `nil&.recursive_basic_literal?` is falsy, so it stays dynamic.
    match cx.call_receiver(expression).get() {
        None => true,
        Some(receiver) => !cx.is_recursive_basic_literal(receiver),
    }
}

/// `source_range_with_comment(node)` — the node's whole-line range expanded
/// through contiguous own-line comments above it.
fn source_range_with_comment(node: NodeId, cx: &Cx<'_>) -> Range {
    cx.range_with_comments_and_lines(node)
}

/// True when a heredoc start/end token lies inside `[start, end)`.
fn swap_span_contains_heredoc(start: u32, end: u32, cx: &Cx<'_>) -> bool {
    cx.sorted_tokens().iter().any(|token| {
        matches!(
            token.kind,
            murphy_plugin_api::SourceTokenKind::HeredocStart
                | murphy_plugin_api::SourceTokenKind::HeredocEnd
        ) && token.range.start >= start
            && token.range.start < end
    })
}

/// `class_elements(class_node)` — the body's children (a `begin`'s children, or
/// a single `def`/`send` body, or empty for a nil body).
fn class_elements(class_node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(body) = class_or_sclass_body(class_node, cx) else {
        return Vec::new();
    };
    match cx.kind(body) {
        NodeKind::Begin(list) => cx.list(*list).to_vec(),
        // `class_def.type?(:def, :send)` → `[class_def]`; any other single
        // node is also returned as the lone element (RuboCop's else branch
        // takes `class_def.children.compact`, but a non-begin body has no
        // child list to unpack here, so the node itself is the element).
        _ => vec![body],
    }
}

fn class_or_sclass_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Class { body, .. } | NodeKind::Sclass { body, .. } => body.get(),
        _ => None,
    }
}

/// `classify(node)` — the category string, or `None` when the node has no
/// meaningful classification (RuboCop returns the raw type name in that case,
/// which `ignore?` then rejects as "not in ExpectedOrder"; we collapse that to
/// `None`-equivalent by returning the raw name and letting `ignore` filter it).
fn classify(node: NodeId, cx: &Cx<'_>, opts: &ClassStructureOptions) -> Option<String> {
    // `block` → classify the send_node (delegates across all block variants).
    if cx.is_any_block_type(node)
        && let Some(call) = cx.block_call(node).get()
    {
        return classify(call, cx, opts);
    }
    match *cx.kind(node) {
        // RuboCop's `classify` matches `when :send` only — `csend`
        // (safe-navigation) is NOT a macro send, so it falls through to the raw
        // type name (`csend`/`send`), which is not in ExpectedOrder → ignored.
        // A class body element like `plugins&.include M` must not be treated as
        // a `module_inclusion` macro.
        NodeKind::Send { .. } => Some(find_send_node_category(node, cx, opts)),
        _ => {
            let name = humanize_node(node, cx);
            Some(find_category(&name, opts).unwrap_or(name))
        }
    }
}

/// `find_send_node_category(node)`.
fn find_send_node_category(node: NodeId, cx: &Cx<'_>, opts: &ClassStructureOptions) -> String {
    let name = cx.method_name(node).unwrap_or("").to_string();
    let category = find_category(&name, opts);
    let key = category.clone().unwrap_or_else(|| name.clone());

    let visibility_key = if let Some(def) = cx.def_modifier(node).get() {
        // `node.def_modifier?` → `"#{name}_methods"`. `name` is the modifier's
        // own method name (e.g. `private` for `private def foo`).
        let _ = def;
        format!("{name}_methods")
    } else {
        format!("{}_{}", node_visibility(node, cx), key)
    };

    if opts.expected_order.contains(&visibility_key) {
        visibility_key
    } else {
        key
    }
}

/// `find_category(name)` — the first category whose macro list contains `name`.
fn find_category(name: &str, opts: &ClassStructureOptions) -> Option<String> {
    opts.categories
        .iter()
        .find(|(_, names)| names.iter().any(|n| n == name))
        .map(|(category, _)| category.clone())
}

/// `humanize_node(node)`.
fn humanize_node(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        // A singleton def (`def self.foo`) is Murphy's `Def` with a receiver,
        // which is RuboCop's `defs` type → `public_class_methods` (no visibility
        // applied, regardless of any surrounding modifier).
        NodeKind::Def { receiver, .. } if receiver.get().is_some() => {
            "public_class_methods".to_string()
        }
        NodeKind::Defs { .. } => "public_class_methods".to_string(),
        // Instance def: `initialize` → `initializer`, else `<visibility>_methods`.
        NodeKind::Def { .. } => {
            if cx.method_name(node) == Some("initialize") {
                "initializer".to_string()
            } else {
                format!("{}_methods", node_visibility(node, cx))
            }
        }
        // `HUMANIZED_NODE_TYPE` mappings (casgn/sclass).
        NodeKind::Casgn { .. } => "constants".to_string(),
        NodeKind::Sclass { .. } => "class_singleton".to_string(),
        // `HUMANIZED_NODE_TYPE[node.type] || node.type` — the raw type name.
        _ => node_type_name(node, cx).to_string(),
    }
}

/// `ignore?(node, classification)`.
fn ignore(node: NodeId, classification: &str, cx: &Cx<'_>, opts: &ClassStructureOptions) -> bool {
    classification.ends_with('=')
        || !opts
            .expected_order
            .iter()
            .any(|c| c.as_str() == classification)
        || private_constant(node, cx)
}

/// `private_constant?(node)` — a namespace-less `casgn` whose enclosing scope
/// marks the constant private via `private_constant :NAME`.
fn private_constant(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Casgn { scope, name, .. } = *cx.kind(node) else {
        return false;
    };
    if scope.get().is_some() {
        return false;
    }
    let const_name = cx.symbol_str(name);
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    // `parent.each_child_node(:send)` — scan the body's send children.
    let children: &[NodeId] = match cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(*list),
        _ => std::slice::from_ref(&parent),
    };
    children
        .iter()
        .any(|&child| marked_as_private_constant(child, const_name, cx))
}

/// `marked_as_private_constant?(node, name)`.
fn marked_as_private_constant(node: NodeId, const_name: &str, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("private_constant") {
        return false;
    }
    // RuboCop matches `({sym str} $_)` and compares the node's *decoded* value,
    // so an escaped form like `private_constant "CON\x53T"` (whose AST value is
    // already `CONST`) must compare equal. Use `string_str` (the unescaped
    // contents), not the raw quoted source.
    cx.call_arguments(node).iter().any(|&arg| match *cx.kind(arg) {
        NodeKind::Sym(s) => cx.symbol_str(s) == const_name,
        NodeKind::Str(s) => cx.string_str(s) == const_name,
        _ => false,
    })
}

/// RuboCop's `VisibilityHelp#node_visibility` — `private`/`protected`/`public`
/// only, defaulting to `public`. Covers inline-on-def, inline-on-method-name,
/// and the nearest preceding bare modifier block.
fn node_visibility(node: NodeId, cx: &Cx<'_>) -> &'static str {
    if let Some(v) = node_visibility_inline(node, cx) {
        return v;
    }
    if let Some(v) = node_visibility_block(node, cx) {
        return v;
    }
    "public"
}

/// Inline forms: `private def foo` (parent send) and `private :foo` (a right
/// sibling naming this def's method). Only for `def` nodes.
fn node_visibility_inline(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    if !matches!(*cx.kind(node), NodeKind::Def { .. }) {
        return None;
    }
    // inline-on-def: `(send nil? VISIBILITY def)` — the parent wraps this def.
    if let Some(parent) = cx.parent(node).get()
        && cx.call_receiver(parent).get().is_none()
        && let Some(v) = as_visibility(cx.method_name(parent))
        && cx.def_modifier(parent).get() == Some(node)
    {
        return Some(v);
    }
    // inline-on-method-name: a right sibling `private :foo` naming this method.
    // RuboCop's `visibility_inline_on_method_name?` is
    // `(send nil? VISIBILITY_SCOPES (sym %method_name))` — a *send* with exactly
    // one symbol argument. A multi-target call like `private :foo, :bar` does NOT
    // match upstream, so it must not reclassify `foo`.
    let method = cx.method_name(node)?;
    let mut sibling = cx.right_sibling(node);
    let mut last: Option<&'static str> = None;
    while let Some(s) = sibling.get() {
        if matches!(*cx.kind(s), NodeKind::Send { .. })
            && cx.call_receiver(s).get().is_none()
            && let Some(v) = as_visibility(cx.method_name(s))
            && let [arg] = cx.call_arguments(s)
            && matches!(*cx.kind(*arg), NodeKind::Sym(sym) if cx.symbol_str(sym) == method)
        {
            // `right_siblings.reverse.find` → the *last* matching sibling.
            last = Some(v);
        }
        sibling = cx.right_sibling(s);
    }
    last
}

/// Block form: the nearest preceding bare `private`/`protected`/`public`.
///
/// RuboCop's `visibility_block?` is `(send nil? VISIBILITY_SCOPES)` — a bare
/// *send* with no receiver and no arguments. A method literally named `private`
/// (`def private; end`) is a `def` node, not a send, so it must not act as a
/// visibility boundary for the methods that follow it.
fn node_visibility_block(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    let mut sibling = cx.left_sibling(node);
    while let Some(s) = sibling.get() {
        if matches!(*cx.kind(s), NodeKind::Send { .. })
            && cx.call_receiver(s).get().is_none()
            && cx.call_arguments(s).is_empty()
            && let Some(v) = as_visibility(cx.method_name(s))
        {
            return Some(v);
        }
        sibling = cx.left_sibling(s);
    }
    None
}

fn as_visibility(name: Option<&str>) -> Option<&'static str> {
    match name {
        Some("private") => Some("private"),
        Some("protected") => Some("protected"),
        Some("public") => Some("public"),
        _ => None,
    }
}

fn node_type_name(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match *cx.kind(node) {
        NodeKind::Send { .. } => "send",
        NodeKind::Csend { .. } => "csend",
        NodeKind::Def { .. } => "def",
        NodeKind::Defs { .. } => "defs",
        NodeKind::Casgn { .. } => "casgn",
        NodeKind::Class { .. } => "class",
        NodeKind::Module { .. } => "module",
        _ => "other",
    }
}

murphy_plugin_api::submit_cop!(ClassStructure);

#[cfg(test)]
mod tests {
    use super::{ClassStructure, ClassStructureOptions};
    use murphy_plugin_api::{
        Cop,
        test_support::{indoc, run_cop_with_edits, run_cop_with_options, run_cop, test},
        ConfigError, CopOptions,
    };

    // The cop is default-disabled, but the test harness dispatches it directly
    // regardless of `default_enabled`, so `run_cop` exercises the default config.

    #[test]
    fn accepts_well_ordered_class() {
        let src = "class Foo\n  include M\n  CONST = 1\n  def self.cm; end\n  def initialize; end\n  def pub; end\n  protected\n  def prot; end\n  private\n  def priv; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn flags_constant_before_module_inclusion() {
        // `CONST` (constants) appears before `include` (module_inclusion).
        let src = "class Foo\n  CONST = 1\n  include M\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`module_inclusion` is supposed to appear before `constants`."
        );
    }

    #[test]
    fn flags_method_before_constant() {
        let src = "class Foo\n  def pub; end\n  CONST = 1\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`constants` is supposed to appear before `public_methods`."
        );
    }

    #[test]
    fn flags_private_method_before_public_method() {
        let src = "class Foo\n  private\n  def priv; end\n  public\n  def pub; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_methods` is supposed to appear before `private_methods`."
        );
    }

    #[test]
    fn class_method_classified_as_public_class_methods_regardless_of_visibility() {
        // `def self.cm` under `private` is still `public_class_methods`, so an
        // instance public method after it is in order (public_methods comes
        // after public_class_methods).
        let src = "class Foo\n  private\n  def self.cm; end\n  def im; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        // `def self.cm` → public_class_methods (idx 2), `def im` under private →
        // private_methods (idx 6). In order, no offense.
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn initializer_ordered_after_class_methods() {
        let src = "class Foo\n  def initialize; end\n  def self.cm; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        // initializer (idx 3) then public_class_methods (idx 2) → out of order.
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_class_methods` is supposed to appear before `initializer`."
        );
    }

    #[test]
    fn attr_reader_ignored_under_default_config() {
        // No `attribute_macros` category by default → `attr_reader` classifies
        // to a non-ExpectedOrder key and is ignored, so a constant after it is
        // not flagged against it.
        let src = "class Foo\n  attr_reader :x\n  CONST = 1\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn private_constant_is_ignored() {
        // A `private_constant`-marked constant is skipped, so an earlier method
        // is not compared against it.
        let src = "class Foo\n  def pub; end\n  CONST = 1\n  private_constant :CONST\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Parity pin (roborev #386): RuboCop's `marked_as_private_constant?` uses
    /// `node.method?(:private_constant)`, and `Node#method?` checks the method
    /// NAME ONLY (`method_name == name`), ignoring the receiver. So even an
    /// explicit-receiver `obj.private_constant :CONST` marks `CONST` private and
    /// excludes it from the order check — exactly as upstream. Requiring a nil
    /// receiver would make Murphy stricter than RuboCop and break parity.
    #[test]
    fn private_constant_with_explicit_receiver_still_ignores_constant() {
        let src =
            "class Foo\n  def pub; end\n  CONST = 1\n  obj.private_constant :CONST\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn inline_private_def_classified_as_private() {
        // `private def priv` after a public method is in order; the reverse is not.
        let src = "class Foo\n  private def priv; end\n  def pub; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        // private_methods (idx 6) then public_methods (idx 4) → out of order.
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_methods` is supposed to appear before `private_methods`."
        );
    }

    /// Parity pin (roborev #386): RuboCop's `on_class` assigns `previous =
    /// index` UNCONDITIONALLY after every node (`class_structure.rb:201`), so
    /// only inversions against the *running* index are flagged. After a
    /// regression resets `previous` to a smaller index, a later node at a
    /// higher-but-still-valid index is NOT flagged. Here:
    ///   `def a` (private_methods=6) → previous=6
    ///   `CONST` (constants=1) → 1<6 → OFFENSE, previous=1
    ///   `def b` (public_methods=4) → 4<1 false → previous=4 (no offense)
    /// Exactly one offense — matching RuboCop. A "max-tracking" variant would
    /// emit a second offense and diverge from upstream, breaking parity.
    #[test]
    fn unconditional_previous_assignment_matches_rubocop() {
        let src =
            "class Foo\n  private\n  def a; end\n  CONST = 1\n  public\n  def b; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`constants` is supposed to appear before `private_methods`."
        );
        assert!(
            !offenses.iter().any(|o| o.message.contains("public_methods")),
            "must not flag public_methods: {offenses:?}"
        );
    }

    #[test]
    fn sclass_body_is_checked() {
        let src = "class << self\n  def pub; end\n  CONST = 1\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    /// Parity pin (Codex #386): RuboCop's `classify` matches `when :send` only,
    /// NOT `csend`. A safe-navigation call like `plugins&.include M` in a class
    /// body must NOT be treated as a `module_inclusion` macro — it falls through
    /// to the raw type name (not in ExpectedOrder) and is ignored. A constant
    /// before it must therefore not be flagged.
    #[test]
    fn safe_navigation_call_is_not_a_class_macro() {
        let src = "class Foo\n  CONST = 1\n  plugins&.include M\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn custom_expected_order_with_categories() {
        let opts = ClassStructureOptions {
            expected_order: vec![
                "public_attribute_macros".to_string(),
                "initializer".to_string(),
            ],
            categories: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    "attribute_macros".to_string(),
                    vec!["attr_reader".to_string()],
                );
                m
            },
        };
        // `def initialize` before `attr_reader :x` → out of order.
        let src = "class Foo\n  def initialize; end\n  attr_reader :x\nend\n";
        let offenses = run_cop_with_options::<ClassStructure>(src, &opts);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_attribute_macros` is supposed to appear before `initializer`."
        );
    }

    /// Parity check (roborev/CodeRabbit #386): a block-form visibility macro
    /// (`private` then `attr_reader :x`) must classify the inner macro with the
    /// block visibility prefix. With a `attribute_macros` category and both
    /// `public_attribute_macros` and `private_attribute_macros` in
    /// `ExpectedOrder`, `attr_reader :x` under a bare `private` resolves to
    /// `private_attribute_macros` via `node_visibility_block`'s left-sibling
    /// scan. Here a public macro appears AFTER the private one → out of order.
    #[test]
    fn block_visibility_prefix_on_attribute_macro() {
        let opts = ClassStructureOptions {
            expected_order: vec![
                "public_attribute_macros".to_string(),
                "private_attribute_macros".to_string(),
            ],
            categories: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    "attribute_macros".to_string(),
                    vec!["attr_reader".to_string()],
                );
                m
            },
        };
        // private attr_reader (private_attribute_macros, idx 1) then a public
        // attr_reader (public_attribute_macros, idx 0) → out of order.
        let src =
            "class Foo\n  private\n  attr_reader :x\n  public\n  attr_reader :y\nend\n";
        let offenses = run_cop_with_options::<ClassStructure>(src, &opts);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_attribute_macros` is supposed to appear before `private_attribute_macros`."
        );
    }

    /// Parity pin (Codex #386, discussion_r3407448923): RuboCop's
    /// `visibility_inline_on_method_name?` is
    /// `(send nil? VISIBILITY_SCOPES (sym %method_name))` — it matches only a
    /// *single* symbol argument. A multi-target call like `private :foo, :bar`
    /// does NOT mark `foo` private upstream, so `foo` stays `public_methods` and
    /// the following public method must not be flagged out of order.
    #[test]
    fn multi_target_inline_visibility_does_not_reclassify() {
        let src = "class Foo\n  def foo; end\n  private :foo, :bar\n  def baz; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Companion to the multi-target pin: a *single*-symbol `private :foo`
    /// (matching `(send nil? VISIBILITY_SCOPES (sym %1))`) DOES mark `foo`
    /// private, so a public method after it is out of order. Guards against the
    /// single-arg path being lost when tightening the multi-arg case.
    #[test]
    fn single_target_inline_visibility_reclassifies() {
        let src = "class Foo\n  def foo; end\n  private :foo\n  def baz; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`public_methods` is supposed to appear before `private_methods`."
        );
    }

    /// Parity pin (Codex #386, discussion_r3407448924): RuboCop's
    /// `visibility_block?` is `(send nil? VISIBILITY_SCOPES)` — a bare *send*. A
    /// method literally named `private` (`def private; end`) is a `def` node, not
    /// a send, so it must NOT act as a visibility boundary for the methods that
    /// follow it. Here `def normal` stays `public_methods`, so the later
    /// `protected def prot` is in order and must not be flagged.
    #[test]
    fn def_named_private_is_not_a_visibility_boundary() {
        let src =
            "class Foo\n  def private; end\n  def normal; end\n  protected def prot; end\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Parity pin (Codex #386, discussion_r3407542494): RuboCop compares the
    /// *decoded* value of a `private_constant` string argument, so an escaped
    /// form like `private_constant "CON\x53T"` still marks `CONST` private and
    /// excludes it from the order check. Compare the decoded `cx.string_str`
    /// value, not the raw quoted source (which keeps the `\x53` escape verbatim).
    #[test]
    fn private_constant_with_escaped_string_is_ignored() {
        let src =
            "class Foo\n  def pub; end\n  CONST = 1\n  private_constant \"CON\\x53T\"\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    // --- autocorrect (murphy-kse6): unsafe node-swap ---

    #[test]
    fn unsafe_autocorrect_metadata_is_declared() {
        assert_eq!(
            <ClassStructure as Cop>::SAFE_AUTOCORRECT,
            Some(false),
            "node-swap reordering is unsafe and must only run under -A"
        );
    }

    #[test]
    fn corrects_constant_before_module_inclusion() {
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  CONST = 1
                  include M
                  ^^^^^^^^^ `module_inclusion` is supposed to appear before `constants`.
                end
            "#},
            indoc! {r#"
                class Foo
                  include M
                  CONST = 1
                end
            "#},
        );
    }

    #[test]
    fn corrects_method_before_constant() {
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  def pub; end
                  CONST = 1
                  ^^^^^^^^^ `constants` is supposed to appear before `public_methods`.
                end
            "#},
            indoc! {r#"
                class Foo
                  CONST = 1
                  def pub; end
                end
            "#},
        );
    }

    #[test]
    fn correction_carries_comment_above_moved_node() {
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  CONST = 1
                  # comment for include
                  include M
                  ^^^^^^^^^ `module_inclusion` is supposed to appear before `constants`.
                end
            "#},
            indoc! {r#"
                class Foo
                  # comment for include
                  include M
                  CONST = 1
                end
            "#},
        );
    }

    #[test]
    fn correction_carries_comment_above_staying_node() {
        // The staying node keeps its comment; the moved node is inserted above
        // the whole comment block.
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  # comment for CONST
                  CONST = 1
                  include M
                  ^^^^^^^^^ `module_inclusion` is supposed to appear before `constants`.
                end
            "#},
            indoc! {r#"
                class Foo
                  include M
                  # comment for CONST
                  CONST = 1
                end
            "#},
        );
    }

    #[test]
    fn no_correction_for_dynamic_constant() {
        // `CONST = foo` is a dynamic constant: offense is still reported, but
        // no edits are emitted (RuboCop's `dynamic_constant?` guard).
        let src = "class Foo\n  def pub; end\n  CONST = foo\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        test::<ClassStructure>().expect_no_corrections(src);
    }

    #[test]
    fn no_correction_for_freeze_on_non_literal() {
        // `CONST = FOO.freeze` — the receiver is not a recursive basic
        // literal, so the constant is dynamic and stays offense-only.
        let src = "class Foo\n  def pub; end\n  CONST = FOO.freeze\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        test::<ClassStructure>().expect_no_corrections(src);
    }

    #[test]
    fn corrects_frozen_literal_constant() {
        // `CONST = [1, 2].freeze` — `freeze` on a recursive basic literal is
        // NOT dynamic, so the swap applies.
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  def pub; end
                  CONST = [1, 2].freeze
                  ^^^^^^^^^^^^^^^^^^^^^ `constants` is supposed to appear before `public_methods`.
                end
            "#},
            indoc! {r#"
                class Foo
                  CONST = [1, 2].freeze
                  def pub; end
                end
            "#},
        );
    }

    #[test]
    fn no_correction_for_heredoc_constant() {
        // The heredoc body lives outside the opener's line range; a whole-line
        // swap would strand it, so heredoc constants stay offense-only.
        let src = "class Foo\n  def pub; end\n  CONST = <<~EOS\n    hi\n  EOS\nend\n";
        let offenses = run_cop::<ClassStructure>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        test::<ClassStructure>().expect_no_corrections(src);
    }

    #[test]
    fn skips_ignored_sibling_when_swapping() {
        // `attr_reader` is ignored under the default config, so the offending
        // `include` swaps past `CONST`, not past the ignored macro.
        test::<ClassStructure>().expect_correction(
            indoc! {r#"
                class Foo
                  attr_reader :x
                  CONST = 1
                  include M
                  ^^^^^^^^^ `module_inclusion` is supposed to appear before `constants`.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_reader :x
                  include M
                  CONST = 1
                end
            "#},
        );
    }

    /// Convergence pin: repeated correction passes bubble the node left until
    /// the class is ordered, and a final pass reports no offenses (idempotent
    /// fixpoint). Mirrors RuboCop's multi-pass `-A` loop.
    #[test]
    fn correction_converges_to_ordered_class() {
        let mut src = "class Foo\n  def pub; end\n  CONST = 1\n  include M\nend\n".to_string();
        for _ in 0..5 {
            let run = run_cop_with_edits::<ClassStructure>(&src);
            if run.edits.is_empty() {
                break;
            }
            src = apply_edits_for_test(&src, &run.edits);
        }
        assert_eq!(
            src, "class Foo\n  include M\n  CONST = 1\n  def pub; end\nend\n",
            "correction did not converge"
        );
        let offenses = run_cop::<ClassStructure>(&src);
        assert!(offenses.is_empty(), "not idempotent: {offenses:?}");
    }

    /// Descending-start edit application, mirroring the host/test-harness
    /// order so the convergence loop above observes the same source the
    /// correction loop would produce.
    fn apply_edits_for_test(
        src: &str,
        edits: &[murphy_plugin_api::test_support::CapturedEdit],
    ) -> String {
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|b| std::cmp::Reverse(b.range.start));
        let mut out = src.to_string();
        for edit in ordered {
            out.replace_range(
                edit.range.start as usize..edit.range.end as usize,
                &edit.replacement,
            );
        }
        out
    }

    // --- option decoding error surface ---

    #[test]
    fn rejects_non_object_root() {
        let err = <ClassStructureOptions as CopOptions>::from_config_json(b"[]")
            .expect_err("array root is invalid");
        assert!(matches!(err, ConfigError { .. }));
    }

    #[test]
    fn rejects_expected_order_wrong_shape() {
        let json = br#"{"ExpectedOrder": "constants"}"#;
        let err = <ClassStructureOptions as CopOptions>::from_config_json(json)
            .expect_err("string ExpectedOrder is invalid");
        let _ = err;
    }

    #[test]
    fn rejects_categories_value_wrong_shape() {
        let json = br#"{"Categories": {"association": "has_many"}}"#;
        let err = <ClassStructureOptions as CopOptions>::from_config_json(json)
            .expect_err("string category value is invalid");
        let _ = err;
    }

    #[test]
    fn config_roundtrips() {
        let opts = ClassStructureOptions::default();
        let json = opts.to_config_json();
        let decoded =
            <ClassStructureOptions as CopOptions>::from_config_json(json.as_bytes()).unwrap();
        assert_eq!(decoded.expected_order, opts.expected_order);
        assert_eq!(decoded.categories, opts.categories);
    }
}
