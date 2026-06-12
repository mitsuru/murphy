//! `Lint/DuplicateMethods` — flag a method that is defined twice in the same
//! scope.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateMethods
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   RuboCop accumulates `@definitions` across the whole file (instance-variable
//!   state) and keys methods by their fully-qualified `parent_module_name`. Murphy
//!   cops are `&self`-only and run in parallel, so we cannot accumulate per-file
//!   state. Instead we dispatch on the scope container (program-root `begin`,
//!   `class`/`module`/`sclass` body, and method bodies for nested defs) and dedup
//!   the *direct* method definers within that one container. A method definer in a
//!   nested scope is checked by that scope's own dispatch, and a `def` inside an
//!   `if` is never scanned together with its sibling branch — reproducing
//!   RuboCop's "ignore conditional defs" behavior for free.
//!
//!   Covered definers: `def` (instance and `def self.x` / `def Const.x`
//!   singleton), `alias_method :new, :old`, and `attr`/`attr_reader`/`attr_writer`/
//!   `attr_accessor`. Self-aliasing (`alias_method :foo, :foo`) is allowed, as in
//!   RuboCop.
//!
//!   Divergence — offense message location. RuboCop's message embeds source
//!   locations as `path:line`, but the plugin API exposes neither the file path
//!   nor a line-number lookup. Murphy emits `line N` (1-based) instead of
//!   `path:N`, so the message reads
//!   `Method \`foo\` is defined at both line 2 and line 3.`.
//!
//!   Known v1 limitations (conservative skips):
//!   - `alias new old` (the `alias` keyword) is not translated by murphy-translate
//!     yet (it lowers to `Unknown`), so keyword-form aliases are not checked. Only
//!     `alias_method` (a `Send`) is.
//!   - Cross-container tracking is not done: a method redefined across two reopened
//!     `class C` blocks is not flagged (RuboCop keys both under the same
//!     `parent_module_name`).
//!   - ActiveSupport `delegate` and Forwardable `def_delegator`/`def_delegators`
//!     are not recognized.
//!   - Anonymous `Class.new`/`Module.new` block scopes and `rescue`/`ensure`
//!     re-definition allowances are not modelled.
//! ```

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct DuplicateMethods;

#[cop(
    name = "Lint/DuplicateMethods",
    description = "Checks for duplicate method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateMethods {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { body, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(body) = body.get() {
            scan_scope(body, cx);
        }
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module { body, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(body) = body.get() {
            scan_scope(body, cx);
        }
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sclass { body, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(body) = body.get() {
            scan_scope(body, cx);
        }
    }

    #[on_node(kind = "def")]
    fn check_def_body(&self, node: NodeId, cx: &Cx<'_>) {
        // Nested method definitions inside a method body form their own scope.
        let NodeKind::Def { body, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(body) = body.get() {
            scan_scope(body, cx);
        }
    }

    #[on_node(kind = "begin")]
    fn check_program_root(&self, node: NodeId, cx: &Cx<'_>) {
        // Only the program root (top-level statement sequence) is a scope of its
        // own here; class/module/sclass/def bodies are handled by their owning
        // container above. A `begin` nested inside one of those would otherwise
        // double-process its statements.
        if cx.parent(node).get().is_none() {
            scan_statements(cx.node_list_begin(node), cx);
        }
    }
}

/// A method definition's key within a scope. Instance methods and singleton
/// methods are distinct, so `def foo` and `def self.foo` never collide.
#[derive(PartialEq, Eq, Hash)]
enum MethodKey {
    Instance(String),
    /// `def self.foo` / `def Const.foo` / `alias`/`attr` on a singleton — keyed
    /// by `receiver_source.method`.
    Singleton(String, String),
}

/// Scan a scope whose body is `body` (a single statement or a `Begin` list).
fn scan_scope(body: NodeId, cx: &Cx<'_>) {
    match *cx.kind(body) {
        NodeKind::Begin(list) => scan_statements(cx.list(list), cx),
        _ => scan_statements(std::slice::from_ref(&body), cx),
    }
}

/// Dedup method definitions among `statements` (direct children of one scope).
fn scan_statements(statements: &[NodeId], cx: &Cx<'_>) {
    let mut seen: HashMap<MethodKey, Range> = HashMap::new();
    for &stmt in statements {
        collect_definers(stmt, cx, &mut |key, range| {
            if let Some(&first) = seen.get(&key) {
                let name = key.method_name();
                let first_line = line_number(cx, first.start);
                let dup_line = line_number(cx, range.start);
                let message = format!(
                    "Method `{name}` is defined at both line {first_line} and line {dup_line}."
                );
                cx.emit_offense(range, &message, None);
            } else {
                seen.insert(key, range);
            }
        });
    }
}

impl MethodKey {
    fn method_name(&self) -> &str {
        match self {
            MethodKey::Instance(name) => name,
            MethodKey::Singleton(_, name) => name,
        }
    }
}

/// Invoke `emit(key, offense_range)` for each method this statement defines.
fn collect_definers(stmt: NodeId, cx: &Cx<'_>, emit: &mut impl FnMut(MethodKey, Range)) {
    match *cx.kind(stmt) {
        NodeKind::Def { receiver, name, .. } => {
            let method = cx.symbol_str(name).to_string();
            let key = match receiver.get() {
                None => MethodKey::Instance(method),
                Some(recv) => MethodKey::Singleton(receiver_key(recv, cx), method),
            };
            emit(key, def_range(stmt, cx));
        }
        NodeKind::Defs { receiver, name, .. } => {
            let method = cx.symbol_str(name).to_string();
            emit(MethodKey::Singleton(receiver_key(receiver, cx), method), def_range(stmt, cx));
        }
        NodeKind::Send { .. } => collect_send_definers(stmt, cx, emit),
        _ => {}
    }
}

/// Handle `alias_method` and `attr*` send statements.
fn collect_send_definers(stmt: NodeId, cx: &Cx<'_>, emit: &mut impl FnMut(MethodKey, Range)) {
    // Only bare-receiver calls (e.g. `attr_reader`, not `x.attr_reader`).
    if cx.call_receiver(stmt).get().is_some() {
        return;
    }
    let Some(method) = cx.method_name(stmt) else {
        return;
    };
    let args = cx.call_arguments(stmt);
    let range = cx.range(stmt);

    match method {
        "alias_method" => {
            // `alias_method :new, :old` — defines `:new`. Self-alias is allowed.
            let (Some(&new_arg), Some(&old_arg)) = (args.first(), args.get(1)) else {
                return;
            };
            let (Some(new_name), Some(old_name)) = (sym_name(new_arg, cx), sym_name(old_arg, cx))
            else {
                return;
            };
            if new_name == old_name {
                return;
            }
            emit(MethodKey::Instance(new_name.to_string()), range);
        }
        // RuboCop's `on_attr`/`found_attr`: for each named attribute, a reader
        // defines `name` and a writer defines `name=`.
        "attr" => {
            // `attr :foo` is read-only; `attr :foo, true` is read+write. Only the
            // first argument is the attribute name (the second is the flag).
            if let Some(name) = args.first().and_then(|&a| sym_name(a, cx)) {
                let writable = args.len() == 2 && matches!(*cx.kind(args[1]), NodeKind::True_);
                emit_attr(name, true, writable, range, emit);
            }
        }
        "attr_reader" => {
            for &arg in args {
                if let Some(name) = sym_name(arg, cx) {
                    emit_attr(name, true, false, range, emit);
                }
            }
        }
        "attr_writer" => {
            for &arg in args {
                if let Some(name) = sym_name(arg, cx) {
                    emit_attr(name, false, true, range, emit);
                }
            }
        }
        "attr_accessor" => {
            for &arg in args {
                if let Some(name) = sym_name(arg, cx) {
                    emit_attr(name, true, true, range, emit);
                }
            }
        }
        _ => {}
    }
}

/// Emit the reader (`name`) and/or writer (`name=`) instance methods an
/// attribute macro defines.
fn emit_attr(
    name: &str,
    readable: bool,
    writable: bool,
    range: Range,
    emit: &mut impl FnMut(MethodKey, Range),
) {
    if readable {
        emit(MethodKey::Instance(name.to_string()), range);
    }
    if writable {
        emit(MethodKey::Instance(format!("{name}=")), range);
    }
}

/// Source-string key for a singleton-method receiver (`self`, a constant, etc.).
fn receiver_key(recv: NodeId, cx: &Cx<'_>) -> String {
    cx.raw_source(cx.range(recv)).to_string()
}

/// Symbol value of a `:sym` argument, if any.
fn sym_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s)),
        _ => None,
    }
}

/// Offense range for a `def`/`defs`: from the `def` keyword to the method name,
/// matching RuboCop's `keyword.join(name)`. `Def`/`Defs` nodes are built with
/// `push` (not `push_named`), so `loc.name` is `Range::ZERO`; the name-token end
/// is found by scanning tokens after the receiver (or the `def` keyword).
fn def_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let (name, search_from) = match *cx.kind(node) {
        NodeKind::Def { receiver, name, .. } => {
            let from = match receiver.get() {
                Some(recv) => cx.range(recv).end,
                None => node_range.start,
            };
            (name, from)
        }
        NodeKind::Defs { receiver, name, .. } => (name, cx.range(receiver).end),
        _ => return node_range,
    };

    let name_bytes = cx.symbol_str(name).as_bytes();
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_from);
    let name_end = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_bytes
        })
        .map_or(node_range.end, |t| t.range.end);

    Range { start: node_range.start, end: name_end }
}

/// 1-based line number of a byte `offset` in the source.
fn line_number(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source().as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Statements of a top-level `Begin`, or a one-element slice for a lone
/// statement. Returns an empty slice if `node` is not a `Begin`.
trait BeginList {
    fn node_list_begin(&self, node: NodeId) -> &[NodeId];
}

impl BeginList for Cx<'_> {
    fn node_list_begin(&self, node: NodeId) -> &[NodeId] {
        match *self.kind(node) {
            NodeKind::Begin(list) => self.list(list),
            _ => &[],
        }
    }
}

murphy_plugin_api::submit_cop!(DuplicateMethods);

#[cfg(test)]
mod tests {
    use super::DuplicateMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_instance_method_at_top_level() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            def foo
              1
            end

            def foo
            ^^^^^^^ Method `foo` is defined at both line 1 and line 5.
              2
            end
        "#});
    }

    #[test]
    fn flags_duplicate_method_in_class() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              def foo
                1
              end

              def foo
              ^^^^^^^ Method `foo` is defined at both line 2 and line 6.
                2
              end
            end
        "#});
    }

    #[test]
    fn allows_distinct_methods() {
        test::<DuplicateMethods>().expect_no_offenses(indoc! {r#"
            def foo
              1
            end

            def bar
              2
            end
        "#});
    }

    #[test]
    fn instance_and_singleton_with_same_name_are_distinct() {
        test::<DuplicateMethods>().expect_no_offenses(indoc! {r#"
            class C
              def foo
                1
              end

              def self.foo
                2
              end
            end
        "#});
    }

    #[test]
    fn flags_duplicate_singleton_method() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              def self.foo
                1
              end

              def self.foo
              ^^^^^^^^^^^^ Method `foo` is defined at both line 2 and line 6.
                2
              end
            end
        "#});
    }

    #[test]
    fn methods_in_different_conditional_branches_are_allowed() {
        test::<DuplicateMethods>().expect_no_offenses(indoc! {r#"
            if cond
              def foo
                1
              end
            else
              def foo
                2
              end
            end
        "#});
    }

    #[test]
    fn flags_attr_reader_collision_with_def() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              attr_reader :foo

              def foo
              ^^^^^^^ Method `foo` is defined at both line 2 and line 4.
                1
              end
            end
        "#});
    }

    #[test]
    fn flags_attr_accessor_writer_collision_with_def() {
        // `attr_accessor :foo` defines both `foo` and `foo=`; the explicit
        // `def foo=` is a duplicate of the generated writer.
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              attr_accessor :foo

              def foo=(value)
              ^^^^^^^^ Method `foo=` is defined at both line 2 and line 4.
              end
            end
        "#});
    }

    #[test]
    fn attr_reader_does_not_define_writer() {
        // `attr_reader :foo` defines only `foo`, so `def foo=` is not a dup.
        test::<DuplicateMethods>().expect_no_offenses(indoc! {r#"
            class C
              attr_reader :foo

              def foo=(value)
              end
            end
        "#});
    }

    #[test]
    fn flags_attr_with_writable_flag_collision() {
        // `attr :foo, true` defines both reader and writer.
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              attr :foo, true

              def foo=(value)
              ^^^^^^^^ Method `foo=` is defined at both line 2 and line 4.
              end
            end
        "#});
    }

    #[test]
    fn flags_alias_method_collision() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            class C
              def foo
                1
              end

              alias_method :foo, :bar
              ^^^^^^^^^^^^^^^^^^^^^^^^ Method `foo` is defined at both line 2 and line 6.
            end
        "#});
    }

    #[test]
    fn allows_self_alias_method() {
        test::<DuplicateMethods>().expect_no_offenses(indoc! {r#"
            class C
              alias_method :foo, :foo
              def foo
                1
              end
            end
        "#});
    }

    #[test]
    fn flags_duplicate_nested_method() {
        test::<DuplicateMethods>().expect_offense(indoc! {r#"
            def outer
              def foo
                1
              end

              def foo
              ^^^^^^^ Method `foo` is defined at both line 2 and line 6.
                2
              end
            end
        "#});
    }
}
