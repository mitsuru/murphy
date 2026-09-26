//! `Naming/MethodName` — enforce `snake_case` / `camelCase` for method names
//! in definitions, aliases, and method-generating declarations.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/MethodName
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `on_def` (aliased to `on_defs`):
//!
//!     return if node.operator_method? || matches_allowed_pattern?(name)
//!     if forbidden_name?(name)         # Forbidden{Identifiers,Patterns}
//!       register_forbidden_name(node)  # MSG_FORBIDDEN, range = loc.name
//!     else
//!       check_name(node, name, loc.name)   # valid_name? = style_regex
//!
//!   Control-flow precedence is exact and DIFFERS from `Naming/VariableName`:
//!   AllowedPatterns is an EARLY return BEFORE the forbidden check, so a name
//!   matching both an AllowedPattern and a ForbiddenPattern is skipped
//!   (verified against rubocop 1.87.0: `AllowedPatterns: ['Foo']` +
//!   `ForbiddenPatterns: ['Foo']` on `def fooFoo` → no offense). Precedence:
//!   operator (skip) > AllowedPatterns (skip) > Forbidden{Identifiers,Patterns}
//!   (forbidden offense) > style-regex (style offense).
//!
//!   Operator methods are exempt via `node.operator_method?`, mirrored by
//!   `method_predicates::is_operator_method` over RuboCop's OPERATOR_METHODS set
//!   (`| ^ & <=> == === =~ > >= < <= << >> + - * / % ** ~ +@ -@ !@ ~@ [] []= !
//!   != !~ \``). Verified: `def coerce` (regular, snake-valid → no offense),
//!   `def []=`, `def +@`, `def \`` → all skipped.
//!
//!   ForbiddenIdentifiers defaults to `[__id__, __send__]` (RuboCop default.yml),
//!   so with no config `def __send__` fires MSG_FORBIDDEN (verified). Identifier
//!   matching uses RuboCop's `name.delete("@$")` then exact membership; this
//!   matters for unary operator aliases such as `+@`.
//!   Allowed/Forbidden Patterns match the FULL method name (unanchored) via
//!   `cx.matches_any_pattern`.
//!
//!   Offense range mirrors `node.loc.name`: the bare method-name token,
//!   INCLUDING any trailing `=` (setters), `?` (predicates), or `!` (bang).
//!   Murphy leaves `loc.name == ZERO` on `def`/`defs`, so the name is located by
//!   source search starting past any singleton receiver (`def self.x`). The def
//!   symbol Murphy interns already carries the suffix (`:setSomething=`,
//!   `:isReady?`), so the range spans it (verified `def setSomething=` col 5..17,
//!   `def isReady?` col 5..12).
//!
//!   Style regexes are byte-level ports of RuboCop's FORMATS hash, shared in
//!   spirit with `Naming/VariableName`:
//!     snake_case: /^@{0,2}[\d[[:lower:]]_]+[!?=]?$/
//!     camelCase:  /^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/
//!   The `@{0,2}` prefix is vestigial for method names (they never start `@`),
//!   but is kept verbatim so the port matches RuboCop's source. Verified
//!   column-for-column against rubocop 1.87.0 for both styles.
//!
//!   The `on_send` handler family is ported: static literal names from
//!   `define_method`/`define_singleton_method`, `Struct.new`/`Data.define`,
//!   `alias_method`, and nil-receiver `attr`/`attr_reader`/`attr_writer`/
//!   `attr_accessor` calls. Bare `alias new_name old_name` is handled when the
//!   new name is a symbol; global-variable and interpolated names are ignored,
//!   matching RuboCop's `on_alias` matcher. Unsupported dynamic/interpolated
//!   send names remain ignored. For aliases, `handle_method_name` checks
//!   AllowedPatterns, then forbidden names, then operator exemption/style (so
//!   a forbidden `+@` alias still reports); this differs from `on_def`'s early
//!   operator return. Attached-block send ranges are trimmed to the call, not
//!   the block body.
//!
//!   Class-emitter behavior is ported: a singleton method whose name exactly
//!   matches the direct sibling class's `loc.name` source text (including any
//!   qualification) is style-valid. Scope
//!   selection climbs only through a contiguous chain of singleton definitions;
//!   it does not cross other AST parents. Direct sibling class names are indexed
//!   lazily once per selected scope, so repeated methods do not rescan it. This
//!   exemption affects style checks only; forbidden-name and AllowedPatterns
//!   precedence is unchanged.
//!
//!   Remaining limitation: Ruby's `[[:lower:]]`/`[[:upper:]]` regex classes are
//!   Unicode-aware; the byte checks here are ASCII-only (the same documented
//!   limitation `Naming/VariableName` and `Naming/ConstantName` carry).
//!   `Naming/AsciiIdentifiers` already flags non-ASCII method names.
//! ```
//!
//! ## Offense range
//!
//! `node.loc.name`: the bare method-name token including any trailing
//! `=`/`?`/`!`, excluding a singleton receiver (`def self.x` → `x`).

use std::collections::{HashMap, HashSet};

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop, def_node_matcher, method_predicates};

// RuboCop parity: `Naming/MethodName` `new_struct?` is
// `(send (const {nil? cbase} :Struct) :new ...)` and `define_data?` is
// `(send (const {nil? cbase} :Data) :define ...)` (send-only, top-level
// only, any args). In Murphy `::Struct` / `::Data` collapse to
// `Const{scope:None}`, so `nil?` covers bare + `::` (pinned by
// `flags_struct_and_data_member_names` +
// `boundary_flags_cbase_data_define_member`); namespaced `Other::Struct` /
// `Other::Data` still silent (pinned by
// `ignores_nonliteral_or_unmatched_dynamic_definitions`); send-only dispatch
// keeps `&.` silent (pinned by `boundary_ignores_csend_*`, matching RuboCop
// send-only). Predicate-only, no captures, byte-identical offense emission.
def_node_matcher!(is_struct_new_call, "(send (const nil? :Struct) :new ...)");
def_node_matcher!(is_data_define_call, "(send (const nil? :Data) :define ...)");

#[derive(Default)]
pub struct MethodName;

/// Enforced naming style for method definitions.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MethodNameStyle {
    /// `snake_case` — RuboCop default.
    #[default]
    #[option(value = "snake_case")]
    SnakeCase,
    /// `camelCase`.
    #[option(value = "camelCase")]
    CamelCase,
}

impl MethodNameStyle {
    fn as_str(self) -> &'static str {
        match self {
            MethodNameStyle::SnakeCase => "snake_case",
            MethodNameStyle::CamelCase => "camelCase",
        }
    }

    /// RuboCop's `FORMATS.fetch(style).match?(name)`.
    fn matches(self, name: &str) -> bool {
        match self {
            MethodNameStyle::SnakeCase => is_snake_case(name),
            MethodNameStyle::CamelCase => is_camel_case(name),
        }
    }
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "snake_case",
        description = "Required method-name style: `snake_case` (default) or `camelCase`."
    )]
    pub enforced_style: MethodNameStyle,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a method whose name matches any is always allowed."
    )]
    pub allowed_patterns: Vec<String>,
    #[option(
        name = "ForbiddenIdentifiers",
        default = ["__id__", "__send__"],
        description = "Exact method names that are forbidden."
    )]
    pub forbidden_identifiers: Vec<String>,
    #[option(
        name = "ForbiddenPatterns",
        default = [],
        description = "Regexes; a method whose name matches any is forbidden."
    )]
    pub forbidden_patterns: Vec<String>,
}

const MSG_FORBIDDEN: &str = "is forbidden, use another method name instead.";

#[cop(
    name = "Naming/MethodName",
    description = "Use the configured style when naming methods.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MethodName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root; append it to preserve top-level
        // `def` handling. Class-emitter sibling names are indexed lazily for
        // only the scopes that contain style-invalid singleton definitions.
        let root = cx.root();
        let mut nodes = cx.descendants(root);
        nodes.push(root);
        let mut class_names_by_scope = HashMap::new();
        for id in nodes {
            match *cx.kind(id) {
                NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => {
                    let name = cx.symbol_str(name);

                    // `return if node.operator_method? || matches_allowed_pattern?(name)`.
                    // Both are early returns BEFORE the forbidden check.
                    if method_predicates::is_operator_method(name)
                        || cx.matches_any_pattern(name, &opts.allowed_patterns)
                    {
                        continue;
                    }

                    let range = def_name_range(id, name, cx);

                    if forbidden_name(name, &opts, cx) {
                        let msg = format!("`{name}` {MSG_FORBIDDEN}");
                        cx.emit_offense(range, &msg, None);
                    } else if !opts.enforced_style.matches(name)
                        && !is_class_emitter_method(
                            id,
                            name,
                            &mut class_names_by_scope,
                            cx,
                        )
                    {
                        // RuboCop's `valid_name?` accepts class-emitter
                        // singleton methods after the style regex fails.
                        let msg = format!("Use {} for method names.", opts.enforced_style.as_str());
                        cx.emit_offense(range, &msg, None);
                    }
                }
                NodeKind::Alias { new_name, .. } => {
                    // RuboCop's on_alias only handles symbolic method names;
                    // global-variable and interpolated aliases are ignored.
                    if let NodeKind::Sym(name) = *cx.kind(new_name) {
                        check_dynamic_name(new_name, cx.symbol_str(name), true, &opts, cx);
                    }
                }
                NodeKind::Send { .. } => check_send(id, &opts, cx),
                _ => {}
            }
        }
    }
}

/// RuboCop's `defs_type?`: Murphy represents singleton definitions as a
/// `Def` with a receiver (or as a `Defs` node in legacy ASTs).
fn is_singleton_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { receiver, .. } => receiver.get().is_some(),
        NodeKind::Defs { .. } => true,
        _ => false,
    }
}

/// Return the scope RuboCop selects for a class-emitter check. It climbs only
/// through directly nested singleton method definitions, not through
/// arbitrary block or instance-method nodes.
fn class_emitter_scope(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if !is_singleton_method_definition(node, cx) {
        return None;
    }

    let mut current = node;
    let mut parent = cx.parent(current).get()?;
    while is_singleton_method_definition(parent, cx) {
        current = parent;
        parent = cx.parent(current).get()?;
    }
    Some(parent)
}

/// Match RuboCop's `class_emitter_method?` while scanning direct siblings once
/// per relevant scope. The cache stays empty for files with no style-invalid
/// singleton definitions.
fn is_class_emitter_method<'a>(
    node: NodeId,
    name: &'a str,
    class_names_by_scope: &mut HashMap<NodeId, HashSet<&'a str>>,
    cx: &Cx<'a>,
) -> bool {
    let Some(scope) = class_emitter_scope(node, cx) else {
        return false;
    };

    class_names_by_scope
        .entry(scope)
        .or_insert_with(|| {
            cx.children(scope)
                .into_iter()
                .filter_map(|child| {
                    let NodeKind::Class {
                        name: class_name, ..
                    } = *cx.kind(child)
                    else {
                        return None;
                    };
                    // RuboCop compares `c.loc.name` source text, so qualified
                    // names such as `X::Foo` do not match method name `Foo`.
                    Some(cx.raw_source(cx.range(class_name)))
                })
                .collect()
        })
        .contains(name)
}

/// RuboCop's `on_send` handler family. These checks run in the same linear
/// tree walk as `def`/`defs`; only target send selectors inspect their args.
fn check_send(node: NodeId, opts: &Options, cx: &Cx<'_>) {
    let Some(selector) = cx.method_name(node) else {
        return;
    };
    let args = cx.call_arguments(node);

    if matches!(selector, "define_method" | "define_singleton_method") {
        let Some(&first) = args.first() else {
            return;
        };
        let Some(name) = literal_method_name(first, cx) else {
            return;
        };
        // RuboCop passes the send node here. Its style range spans the
        // selector's argument tail; a forbidden-name offense narrows back to
        // the first argument in `register_forbidden_name`.
        check_dynamic_name(node, name, true, opts, cx);
        return;
    }

    if selector == "new" && is_struct_new_call(node, cx) {
        let first_member = usize::from(
            args.first()
                .is_some_and(|&arg| matches!(*cx.kind(arg), NodeKind::Str(_))),
        );
        for &member in &args[first_member..] {
            if let Some(name) = literal_method_name(member, cx) {
                check_dynamic_name(member, name, true, opts, cx);
            }
        }
        return;
    }

    if selector == "define" && is_data_define_call(node, cx) {
        for &member in args {
            if let Some(name) = literal_method_name(member, cx) {
                check_dynamic_name(member, name, true, opts, cx);
            }
        }
        return;
    }

    if selector == "alias_method" {
        if args.len() == 2
            && let Some(name) = literal_method_name(args[0], cx)
        {
            // `handle_alias_method` passes the first literal node, unlike
            // `handle_define_method`, so the style range is just that name.
            check_dynamic_name(args[0], name, true, opts, cx);
        }
        return;
    }

    if cx.call_receiver(node).get().is_none()
        && matches!(selector, "attr" | "attr_reader" | "attr_writer" | "attr_accessor")
    {
        check_attribute_accessor(node, args, opts, cx);
    }
}

/// Static method-name values accepted by RuboCop's `sym_name` / `str_name`
/// matchers. Interpolated strings/symbols and other expressions are ignored.
fn literal_method_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Sym(name) => Some(cx.symbol_str(name)),
        NodeKind::Str(value) => Some(cx.string_str(value)),
        _ => None,
    }
}

/// `handle_method_name`: AllowedPatterns wins over forbidden names; operator
/// names are exempt only after the forbidden check. `node` is either a literal
/// name or the `define_method` send, matching RuboCop's distinct ranges.
fn check_dynamic_name(
    node: NodeId,
    name: &str,
    operator_methods_exempt: bool,
    opts: &Options,
    cx: &Cx<'_>,
) {
    if cx.matches_any_pattern(name, &opts.allowed_patterns) {
        return;
    }

    if forbidden_name(name, opts, cx) {
        let (range, reported_name) = match *cx.kind(node) {
            NodeKind::Sym(_) | NodeKind::Str(_) => (cx.range(node), name),
            _ => match cx.first_argument(node).get() {
                Some(first) => (
                    cx.range(first),
                    literal_method_name(first, cx).unwrap_or(name),
                ),
                None => (cx.range(node), name),
            },
        };
        let msg = format!("`{reported_name}` {MSG_FORBIDDEN}");
        cx.emit_offense(range, &msg, None);
    } else if !(opts.enforced_style.matches(name)
        || operator_methods_exempt && method_predicates::is_operator_method(name))
    {
        let msg = format!("Use {} for method names.", opts.enforced_style.as_str());
        cx.emit_offense(range_position(node, cx), &msg, None);
    }
}

/// `handle_attr_accessor`: each offending member calls `add_offense` on the
/// same send/member range. Collapse identical reports here, as the RuboCop
/// offense collection does, while preserving the peculiar last-member range
/// used by `register_forbidden_name`.
fn check_attribute_accessor(node: NodeId, args: &[NodeId], opts: &Options, cx: &Cx<'_>) {
    let Some(&last_arg) = args.last() else {
        return;
    };
    let style_range = range_position(node, cx);
    let forbidden_range = cx.range(last_arg);
    let forbidden_reported_name = literal_method_name(last_arg, cx).unwrap_or("");
    let mut style_emitted = false;
    let mut forbidden_emitted = false;

    for &arg in args {
        let Some(name) = literal_method_name(arg, cx) else {
            continue;
        };
        if cx.matches_any_pattern(name, &opts.allowed_patterns) {
            continue;
        }

        if forbidden_name(name, opts, cx) {
            if !forbidden_emitted {
                let msg = format!("`{forbidden_reported_name}` {MSG_FORBIDDEN}");
                cx.emit_offense(forbidden_range, &msg, None);
                forbidden_emitted = true;
            }
        } else if !opts.enforced_style.matches(name) && !style_emitted {
            let msg = format!("Use {} for method names.", opts.enforced_style.as_str());
            cx.emit_offense(style_range, &msg, None);
            style_emitted = true;
        }
    }
}

/// RuboCop's `range_position`: selector end + one byte through the node end,
/// or the whole node when it has no selector (literal method-name nodes).
fn range_position(node: NodeId, cx: &Cx<'_>) -> Range {
    let mut expression = cx.range(node);
    if let Some(parent) = cx.parent(node).get() {
        let is_attached_block = match *cx.kind(parent) {
            NodeKind::Block { call, .. } => call == node,
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send == node,
            _ => false,
        };
        if is_attached_block {
            // Prism gives the send the attached block's full expression range,
            // but RuboCop's `on_send` node ends before `do` / `{`. Use `)`
            // only when it directly follows this call's selector; after a space,
            // it may instead close a grouped command-style argument.
            let loc = cx.loc(node);
            let selector = loc.name;
            let opening_paren = loc.begin();
            let has_call_parens = selector != Range::ZERO && opening_paren.start == selector.end;
            let closing_paren = if has_call_parens {
                loc.end()
            } else {
                Range::ZERO
            };
            expression.end = if closing_paren != Range::ZERO {
                closing_paren.end
            } else if let Some(last_arg) = cx.call_arguments(node).last() {
                cx.range(*last_arg).end
            } else {
                selector.end
            };
        }
    }

    let selector = cx.loc(node).name;
    if selector == Range::ZERO {
        expression
    } else {
        Range {
            start: selector.end.saturating_add(1),
            end: expression.end,
        }
    }
}

/// `forbidden_name?`: `forbidden_identifier?(name) || forbidden_pattern?(name)`.
fn forbidden_name(name: &str, opts: &Options, cx: &Cx<'_>) -> bool {
    forbidden_identifier(name, &opts.forbidden_identifiers)
        || cx.matches_any_pattern(name, &opts.forbidden_patterns)
}

/// RuboCop's `forbidden_identifier?`: `name.delete("@$")` then exact membership.
/// This matters for unary operator names such as `+@` as well as other sigils.
fn forbidden_identifier(name: &str, forbidden: &[String]) -> bool {
    if forbidden.is_empty() {
        return false;
    }
    let stripped: String = name.chars().filter(|&c| c != '@' && c != '$').collect();
    forbidden.contains(&stripped)
}

/// Byte range of the method name within a `def`/`defs` definition, mirroring
/// RuboCop's `node.loc.name`.
///
/// Murphy leaves `loc.name` as `Range::ZERO` for `Def`/`Defs`, so the name is
/// located by its first occurrence in the node's source, starting past any
/// singleton receiver (`def self.x` / `def Foo.x`) so a receiver whose source
/// contains the name as a substring cannot mis-anchor the caret. Beyond the
/// receiver the name always precedes the argument list and body. The interned
/// def symbol already carries any trailing `=`/`?`/`!`, so the range spans it.
/// Falls back to a zero-width caret at the node start if the name is not found.
fn def_name_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    let from = cx
        .def_receiver(id)
        .get()
        .map_or(0, |r| (cx.range(r).end - expr.start) as usize);
    match src[from..].find(name) {
        Some(off) => {
            let start = expr.start + (from + off) as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start,
        },
    }
}

/// snake_case: RuboCop's `/^@{0,2}[\d[[:lower:]]_]+[!?=]?$/`.
///
/// Leading `@{0,2}` (0–2 `@`), then one-or-more of digit / ASCII-lowercase /
/// `_`, then an optional single `!`/`?`/`=`.
fn is_snake_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;

    // `@{0,2}`
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }

    // optional trailing `[!?=]`
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }

    // `[\d[[:lower:]]_]+` — at least one character required.
    if end <= i {
        return false;
    }
    bytes[i..end]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// camelCase: RuboCop's
/// `/^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/`.
///
/// Leading `@{0,2}`; then EITHER a single `_`, OR an optional leading `_`
/// followed by an ASCII-lowercase letter and any run of digit / lower / upper;
/// then an optional single `!`/`?`/`=`.
fn is_camel_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;

    // `@{0,2}`
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }

    // optional trailing `[!?=]`
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }

    let body = &bytes[i..end];

    // Alternative 1: a single `_`.
    if body == b"_" {
        return true;
    }

    // Alternative 2: `_?[[:lower:]][\d[[:lower:]][[:upper:]]]*`.
    let mut j = 0;
    if j < body.len() && body[j] == b'_' {
        j += 1;
    }
    // Required ASCII-lowercase letter.
    if j >= body.len() || !body[j].is_ascii_lowercase() {
        return false;
    }
    j += 1;
    // Remaining: digit / lower / upper.
    body[j..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{MethodName, MethodNameStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(style: MethodNameStyle) -> Options {
        Options {
            enforced_style: style,
            allowed_patterns: vec![],
            forbidden_identifiers: vec![],
            forbidden_patterns: vec![],
        }
    }

    // --- snake_case (default); carets from rubocop 1.87.0 col..last_column. ---

    #[test]
    fn flags_camel_case_method_definition() {
        // rubocop: `def fooBar` col 5..10.
        test::<MethodName>().expect_offense(indoc! {r#"
            def fooBar
                ^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_camel_case_method_no_underscore() {
        // `def goodName` col 5..12.
        test::<MethodName>().expect_offense(indoc! {r#"
            def goodName
                ^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_setter_definition_including_equals() {
        // `def setSomething=(val)` — loc.name spans the trailing `=`, col 5..17.
        test::<MethodName>().expect_offense(indoc! {r#"
            def setSomething=(val)
                ^^^^^^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_predicate_definition_including_question() {
        // `def isReady?` — loc.name spans the trailing `?`, col 5..12.
        test::<MethodName>().expect_offense(indoc! {r#"
            def isReady?
                ^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_bang_definition_including_bang() {
        // `def doIt!` — loc.name spans the trailing `!`, col 5..9.
        test::<MethodName>().expect_offense(indoc! {r#"
            def doIt!
                ^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_singleton_method_definition() {
        // `def self.classMethod` — name after `self.`, col 10..20.
        test::<MethodName>().expect_offense(indoc! {r#"
            def self.classMethod
                     ^^^^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn accepts_direct_sibling_class_emitter_methods() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            class Container
            def self.Foo
            end
            class Foo
            end
            end

            module ModuleContainer
            def self.Model
            end
            class Model
            end
            end
        "#});
    }

    #[test]
    fn accepts_nested_singleton_class_emitter_methods() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            class NestedEmitter
            def self.included(base)
            def base.Bar; end
            end
            class Bar
            end
            end
        "#});
    }

    #[test]
    fn class_emitter_method_is_valid_for_either_style() {
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_no_offenses(indoc! {r#"
                class CamelContainer
                def self.Foo
                end
                class Foo
                end
                end
            "#});
    }

    #[test]
    fn class_emitter_requires_a_direct_sibling_class_and_singleton_method() {
        test::<MethodName>().expect_offense(indoc! {r#"
            class WrongSibling
            def self.Foo
                     ^^^ Use snake_case for method names.
            end
            class Bar
            end
            end

            class InstanceMethod
            def Foo
                ^^^ Use snake_case for method names.
            end
            class Foo
            end
            end

            class NestedSibling
            def self.Baz
                     ^^^ Use snake_case for method names.
            end
            class Inner
            class Baz
            end
            end
            end

            class QualifiedSibling
            def self.Named
                     ^^^^^ Use snake_case for method names.
            end
            class Parent::Named
            end
            end

            class ModuleSibling
            def self.ModuleName
                     ^^^^^^^^^^ Use snake_case for method names.
            end
            module ModuleName
            end
            end

            class InterruptedNested
            def self.included
            value = 1
            def self.Nested; end
                     ^^^^^^ Use snake_case for method names.
            end
            class Nested
            end
            end
        "#});
    }

    #[test]
    fn class_emitter_exemption_does_not_override_forbidden_names() {
        let options = Options {
            forbidden_identifiers: vec!["Foo".to_string()],
            ..opts(MethodNameStyle::SnakeCase)
        };
        test::<MethodName>()
            .with_options(&options)
            .expect_offense(indoc! {r#"
                class Container
                def self.Foo
                         ^^^ `Foo` is forbidden, use another method name instead.
                end
                class Foo
                end
                end
            "#});
    }

    // --- conforming / exempt (snake_case) ---

    #[test]
    fn accepts_snake_case_definitions() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            def foo_bar
            end
            def foo1
            end
            def foo_1
            end
            def coerce(other)
            end
            def valid?
            end
            def save!
            end
        "#});
    }

    #[test]
    fn ignores_operator_methods() {
        // `==`, `[]=`, `+@`, `` ` `` are operator methods — all exempt.
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            def ==(other)
            end
            def [](key)
            end
            def []=(key, value)
            end
            def +@
            end
        "#});
    }

    #[test]
    fn ignores_method_calls_and_variables() {
        // Only `def`/`defs` are checked, not calls or assignments.
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            obj.fooBar
            barBaz = 1
        "#});
    }

    #[test]
    fn flags_dynamic_definition_names() {
        test::<MethodName>().expect_offense(indoc! {r#"
            define_method :badName, :extraName
                          ^^^^^^^^^^^^^^^^^^^^ Use snake_case for method names.
            define_method :blockName do
                          ^^^^^^^^^^ Use snake_case for method names.
              true
            end
            define_method(:parenthesizedBlock) { true }
                          ^^^^^^^^^^^^^^^^^^^^ Use snake_case for method names.
            define_singleton_method("badSingletonName")
                                    ^^^^^^^^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn flags_struct_and_data_member_names() {
        test::<MethodName>().expect_offense(indoc! {r#"
            Struct.new(:badStructName)
                       ^^^^^^^^^^^^^^ Use snake_case for method names.
            ::Struct.new("Record", :badMemberName)
                                   ^^^^^^^^^^^^^^ Use snake_case for method names.
            Data.define("badDataName")
                        ^^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn flags_alias_method_name_only() {
        test::<MethodName>().expect_offense(indoc! {r#"
            alias_method :badAliasName, :old_name
                         ^^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn flags_bare_alias_new_name_with_exact_range() {
        test::<MethodName>().expect_offense(indoc! {r#"
            alias badName badOldName
                  ^^^^^^^ Use snake_case for method names.
            alias :badSymbolName :old_name
                  ^^^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn bare_alias_ignores_old_global_and_interpolated_names() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            alias good_name badOldName
            alias [] get
            alias $badName $old_name
            alias :"bad#{name}" :old_name
        "#});
    }

    #[test]
    fn bare_alias_preserves_forbidden_and_allowed_pattern_precedence() {
        test::<MethodName>().expect_offense(indoc! {r#"
            alias __send__ old_name
                  ^^^^^^^^ `__send__` is forbidden, use another method name instead.
        "#});

        let forbidden_operator = Options {
            forbidden_identifiers: vec!["+".to_string()],
            ..opts(MethodNameStyle::SnakeCase)
        };
        test::<MethodName>()
            .with_options(&forbidden_operator)
            .expect_offense(indoc! {r#"
                alias :+@ :plus
                      ^^^ `+@` is forbidden, use another method name instead.
            "#});

        let forbidden_pattern = Options {
            forbidden_patterns: vec!["badName".to_string()],
            ..opts(MethodNameStyle::SnakeCase)
        };
        test::<MethodName>()
            .with_options(&forbidden_pattern)
            .expect_offense(indoc! {r#"
                alias badName old_name
                      ^^^^^^^ `badName` is forbidden, use another method name instead.
            "#});

        let allowed_pattern = Options {
            allowed_patterns: vec!["badName".to_string()],
            forbidden_patterns: vec!["badName".to_string()],
            ..opts(MethodNameStyle::SnakeCase)
        };
        test::<MethodName>()
            .with_options(&allowed_pattern)
            .expect_no_offenses("alias badName old_name\n");
    }

    #[test]
    fn flags_attr_macro_member_names() {
        test::<MethodName>().expect_offense(indoc! {r#"
            attr_accessor :badAccessorName, :badOtherAccessor
                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use snake_case for method names.
            attr_accessor (:foo), :badName do
                          ^^^^^^^^^^^^^^^^ Use snake_case for method names.
              nil
            end
            attr_reader :badReaderName
                        ^^^^^^^^^^^^^^ Use snake_case for method names.
            attr_writer :badWriterName
                        ^^^^^^^^^^^^^^ Use snake_case for method names.
            attr :badAttrName
                 ^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn flags_forbidden_dynamic_names() {
        test::<MethodName>().expect_offense(indoc! {r#"
            define_method :__send__, :otherBadName
                          ^^^^^^^^^ `__send__` is forbidden, use another method name instead.
            alias_method :__id__, :old_name
                         ^^^^^^^ `__id__` is forbidden, use another method name instead.
        "#});
    }

    #[test]
    fn forbidden_attr_macro_preserves_rubocop_last_member_range() {
        test::<MethodName>().expect_offense(indoc! {r#"
            attr_accessor :__send__, :ok_name
                                     ^^^^^^^^ `ok_name` is forbidden, use another method name instead.
        "#});
    }

    #[test]
    fn operator_names_are_exempt_except_for_attribute_macros() {
        test::<MethodName>().expect_offense(indoc! {r#"
            define_method :[]
            attr_accessor :[]
                          ^^^ Use snake_case for method names.
            alias_method :[], :old_name
        "#});
    }

    #[test]
    fn ignores_nonliteral_or_unmatched_dynamic_definitions() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            define_method(method_name)
            define_method(:"bad#{name}")
            alias_method :badAliasName
            alias_method :badAliasName, :old_name, :extra
            Struct.new("badClassName")
            Other::Struct.new(:badMemberName)
            Other::Data.define(:badDataName)
            obj.attr_accessor :badAccessorName
            self.attr_reader(:badReaderName)
            attr_accessor method_name
        "#});
    }

    #[test]
    fn dynamic_allowed_pattern_precedes_forbidden_and_style() {
        let options = Options {
            allowed_patterns: vec!["badName".to_string()],
            forbidden_patterns: vec!["badName".to_string()],
            ..opts(MethodNameStyle::SnakeCase)
        };
        test::<MethodName>()
            .with_options(&options)
            .expect_no_offenses("define_method :badName\n");
    }

    #[test]
    fn dynamic_names_use_configured_camel_case_style() {
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_offense(indoc! {r#"
                Data.define(:snake_name)
                            ^^^^^^^^^^^ Use camelCase for method names.
            "#});
    }

    // --- ForbiddenIdentifiers (default __id__/__send__) ---

    #[test]
    fn flags_forbidden_default_identifier() {
        // Default ForbiddenIdentifiers carries `__send__`; with no options the
        // Rust `Options::default()` must include it.
        test::<MethodName>().expect_offense(indoc! {r#"
            def __send__
                ^^^^^^^^ `__send__` is forbidden, use another method name instead.
            end
        "#});
    }

    #[test]
    fn flags_forbidden_default_id() {
        test::<MethodName>().expect_offense(indoc! {r#"
            def __id__
                ^^^^^^ `__id__` is forbidden, use another method name instead.
            end
        "#});
    }

    #[test]
    fn forbidden_identifier_flags_conforming_name() {
        // A snake_case-valid name still fires when forbidden.
        test::<MethodName>()
            .with_options(&Options {
                forbidden_identifiers: vec!["foo_bar".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                def foo_bar
                    ^^^^^^^ `foo_bar` is forbidden, use another method name instead.
                end
            "#});
    }

    #[test]
    fn forbidden_pattern_flags_name() {
        test::<MethodName>()
            .with_options(&Options {
                forbidden_patterns: vec![r"_v\d+\z".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                def release_v1
                    ^^^^^^^^^^ `release_v1` is forbidden, use another method name instead.
                end
            "#});
    }

    // --- AllowedPatterns (early-return precedence) ---

    #[test]
    fn allowed_pattern_skips_offense() {
        test::<MethodName>()
            .with_options(&Options {
                allowed_patterns: vec![r"\AonSelection".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                def onSelectionChange
                end
            "#});
    }

    #[test]
    fn allowed_pattern_beats_forbidden_pattern() {
        // RuboCop's AllowedPatterns early-return precedes the forbidden check:
        // a name matching BOTH is skipped (verified against rubocop 1.87.0).
        test::<MethodName>()
            .with_options(&Options {
                allowed_patterns: vec!["Foo".to_string()],
                forbidden_patterns: vec!["Foo".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                def fooFoo
                end
            "#});
    }

    // --- camelCase style ---

    #[test]
    fn camel_flags_snake_case_definition() {
        // With camelCase: `def foo_bar` col 5..11 flagged.
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_offense(indoc! {r#"
                def foo_bar
                    ^^^^^^^ Use camelCase for method names.
                end
            "#});
    }

    #[test]
    fn camel_accepts_camel_case_definitions() {
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_no_offenses(indoc! {r#"
                def fooBar
                end
                def fooBar2
                end
                def foo
                end
            "#});
    }

    // --- Boundary characterization (murphy-puku batch 27): pin the exact
    // node set the hand-rolled `const_receiver_is` Struct/Data guards match,
    // so the verbatim `(send (const nil? :Struct) :new ...)` +
    // `(send (const nil? :Data) :define ...)` refactor can be proven
    // equivalent. `::Struct` / `::Data` collapse to Const scope None in
    // Murphy, so `nil?` covers bare + `::` (both flag, cbase pre-pinned for
    // Struct via `flags_struct_and_data_member_names`); namespaced
    // `Foo::Struct` / `Foo::Data` still silent (pre-pinned via
    // `ignores_nonliteral_or_unmatched_dynamic_definitions`); send-only
    // dispatch keeps `&.` silent (new pins below, matching RuboCop
    // send-only `new_struct?` / `define_data?`).

    #[test]
    fn boundary_flags_cbase_data_define_member() {
        test::<MethodName>().expect_offense(indoc! {r#"
            ::Data.define(:badMemberName)
                          ^^^^^^^^^^^^^^ Use snake_case for method names.
        "#});
    }

    #[test]
    fn boundary_ignores_csend_struct_new_member() {
        test::<MethodName>().expect_no_offenses("Struct&.new(:badMemberName)\n");
    }

    #[test]
    fn boundary_ignores_csend_data_define_member() {
        test::<MethodName>().expect_no_offenses("Data&.define(:badMemberName)\n");
    }
}
murphy_plugin_api::submit_cop!(MethodName);
