//! `Style/TrivialAccessors` — prefer `attr_*` methods to trivial readers/writers.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrivialAccessors
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Offense range: the `def` keyword only (matches RuboCop's `node.loc.keyword`).
//!   top_level_node? check: defs whose parent is `None` (file-level bare def) are
//!   skipped — matches RuboCop's `top_level_node?` returning true when parent.nil?.
//!   in_module_or_instance_eval? check: defs inside a Module ancestor (before
//!   reaching a Class or Sclass) are skipped. instance_eval blocks are not checked
//!   (conservative v1 gap).
//!   ExactNameMatch: true (default) — offense only when method name matches ivar
//!   name (e.g. `def foo; @foo; end` but not `def name; @other_name; end`).
//!   AllowPredicates: true (default) — `def foo?; @foo; end` is allowed.
//!   AllowDSLWriters: true (default) — only `foo=`-style writers fire; methods
//!   with a parameter that don't end in `=` are allowed.
//!   IgnoreClassMethods: false (default) — `def self.foo; @foo; end` is flagged.
//!   AllowedMethods: to_ary, to_a, to_c, to_enum, to_h, to_hash, to_i, to_int,
//!   to_io, to_open, to_path, to_proc, to_r, to_regexp, to_str, to_s, to_sym,
//!   plus `initialize` (always allowed regardless of config).
//!   Autocorrect: replaces the whole def node with `attr_reader :name` or
//!   `attr_writer :name`. For `def self.foo`, wraps in `class << self`.
//!   The `parent&.send_type?` guard (RuboCop skips autocorrect when the def is
//!   inside a `private def foo`) — not implemented (conservative gap; the offense
//!   is still emitted but no autocorrect guard fires).
//!   AllowedMethods option: not configurable in v1 (uses hardcoded defaults).
//! ```
//!
//! ## Trivial reader
//!
//! A `def` node with no arguments whose body is a single `Ivar` node.
//!
//! ## Trivial writer
//!
//! A `def` node whose name ends in `=` with exactly one argument whose body is
//! a single `Ivasgn` assigning the argument's value to an ivar.
//!
//! ## DSL writer
//!
//! A `def` node with exactly one argument that does NOT end in `=`. These are
//! allowed when `AllowDSLWriters: true` (default).
//!
//! ## Autocorrect
//!
//! Replaces the entire method definition with:
//! - `attr_reader :name` for readers
//! - `attr_writer :name` for writers (name without the trailing `=`)
//! - For `def self.foo` singleton receivers: wraps in `class << self` block.

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, cop};
use serde_json::Value;

const DEFAULT_ALLOWED_METHODS: &[&str] = &[
    "to_ary",
    "to_a",
    "to_c",
    "to_enum",
    "to_h",
    "to_hash",
    "to_i",
    "to_int",
    "to_io",
    "to_open",
    "to_path",
    "to_proc",
    "to_r",
    "to_regexp",
    "to_str",
    "to_s",
    "to_sym",
    "initialize",
];

/// Stateless unit struct.
#[derive(Default)]
pub struct TrivialAccessors;

#[derive(Clone, Debug)]
pub struct TrivialAccessorsOptions {
    /// When `true` (default), method name must match ivar name exactly.
    pub exact_name_match: bool,
    /// When `true` (default), predicate methods (`foo?`) are allowed.
    pub allow_predicates: bool,
    /// When `true` (default), DSL-style writers (no trailing `=`) are allowed.
    pub allow_dsl_writers: bool,
    /// When `true`, `def self.foo` methods are ignored.
    pub ignore_class_methods: bool,
}

impl Default for TrivialAccessorsOptions {
    fn default() -> Self {
        Self {
            exact_name_match: true,
            allow_predicates: true,
            allow_dsl_writers: true,
            ignore_class_methods: false,
        }
    }
}

impl CopOptions for TrivialAccessorsOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let exact_name_match = decode_bool(obj, "ExactNameMatch", true)?;
        let allow_predicates = decode_bool(obj, "AllowPredicates", true)?;
        let allow_dsl_writers = decode_bool(obj, "AllowDSLWriters", true)?;
        let ignore_class_methods = decode_bool(obj, "IgnoreClassMethods", false)?;

        Ok(Self {
            exact_name_match,
            allow_predicates,
            allow_dsl_writers,
            ignore_class_methods,
        })
    }

    fn to_config_json(&self) -> String {
        format!(
            r#"{{"ExactNameMatch":{},"AllowPredicates":{},"AllowDSLWriters":{},"IgnoreClassMethods":{}}}"#,
            self.exact_name_match,
            self.allow_predicates,
            self.allow_dsl_writers,
            self.ignore_class_methods,
        )
    }
}

fn decode_bool(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, ConfigError> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ConfigError::type_mismatch(key, "bool")),
    }
}

#[cop(
    name = "Style/TrivialAccessors",
    description = "Prefer attr_* methods to trivial readers/writers.",
    default_severity = "warning",
    default_enabled = true,
    options = TrivialAccessorsOptions,
)]
impl TrivialAccessors {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<TrivialAccessorsOptions>();
        check(node, cx, &opts);
    }
}

/// Check a def (or defs via singleton receiver) for trivial accessor patterns.
fn check(node: NodeId, cx: &Cx<'_>, opts: &TrivialAccessorsOptions) {
    // `top_level_node?`: skip if this def's parent is None (file root).
    if cx.parent(node).get().is_none() {
        return;
    }

    // `in_module_or_instance_eval?`: skip defs directly inside a Module
    // (without an intervening Class/Sclass boundary).
    if in_module_ancestor(node, cx) {
        return;
    }

    let NodeKind::Def {
        receiver,
        name,
        args,
        body,
    } = *cx.kind(node)
    else {
        return;
    };

    // `IgnoreClassMethods`: skip singleton receivers.
    if opts.ignore_class_methods && receiver.get().is_some() {
        return;
    }

    let method_name = cx.symbol_str(name);

    // Always-allowed methods.
    if DEFAULT_ALLOWED_METHODS.contains(&method_name) {
        return;
    }

    // AllowPredicates: skip predicate methods.
    if opts.allow_predicates && method_name.ends_with('?') {
        return;
    }

    let Some(body_id) = body.get() else {
        // Empty body — not a trivial accessor.
        return;
    };

    let arg_nodes = match cx.kind(args) {
        NodeKind::Args(list) => cx.list(*list),
        _ => return,
    };

    // Determine kind and whether the names match (for safe autocorrect).
    let (kind, names_match) = if let Some(nm) =
        trivial_reader_names_match(method_name, arg_nodes, body_id, cx, opts)
    {
        ("reader", nm)
    } else if let Some(nm) = trivial_writer_names_match(method_name, arg_nodes, body_id, cx, opts) {
        ("writer", nm)
    } else {
        return;
    };

    let msg = format!("Use `attr_{kind}` to define trivial {kind} methods.");
    let keyword_range = cx.loc(node).keyword();
    cx.emit_offense(keyword_range, &msg, None);

    // Autocorrect: only safe when names match and method is a proper `=` writer
    // (not a DSL writer whose API would change). Mirrors RuboCop's `names_match?`
    // guard in `autocorrect_instance`.
    // For singleton methods (`def self.foo`), only autocorrect when the receiver
    // is exactly `self` — arbitrary receivers (`def other.foo`) would require
    // `class << other` which is a different semantic; skip those.
    let is_dsl_writer = kind == "writer" && !method_name.ends_with('=');
    let is_self_receiver = receiver
        .get()
        .is_some_and(|r| matches!(cx.kind(r), NodeKind::SelfExpr));
    let has_non_self_receiver = receiver.get().is_some() && !is_self_receiver;
    if names_match && !is_dsl_writer && !has_non_self_receiver {
        autocorrect(node, cx, kind, method_name, is_self_receiver);
    }
}

/// Returns `true` if there's a Module ancestor before any Class/Sclass boundary.
fn in_module_ancestor(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Module { .. } => return true,
            NodeKind::Class { .. } | NodeKind::Sclass { .. } => return false,
            _ => {}
        }
    }
    false
}

/// Trivial reader: no args, body is a single `Ivar` node.
/// Returns `Some(names_match)` if this is a trivial reader, where `names_match`
/// indicates whether ivar name == method base name (safe to autocorrect).
/// Returns `None` if not a trivial reader.
///
/// Name matching strips trailing `?` and `=` suffixes before comparing, matching
/// RuboCop's `names_match?` (`method_name.to_s.sub(/[=?]$/, '')`).
fn trivial_reader_names_match(
    method_name: &str,
    args: &[NodeId],
    body_id: NodeId,
    cx: &Cx<'_>,
    opts: &TrivialAccessorsOptions,
) -> Option<bool> {
    if !args.is_empty() {
        return None;
    }
    let NodeKind::Ivar(ivar_sym) = *cx.kind(body_id) else {
        return None;
    };
    let ivar_name = cx.symbol_str(ivar_sym);
    // Strip trailing `?` or `=` before comparing (mirrors RuboCop's names_match?).
    let method_base = method_name.trim_end_matches(['?', '=']);
    let names_match = ivar_name
        .strip_prefix('@')
        .is_some_and(|n| n == method_base);
    if opts.exact_name_match && !names_match {
        return None;
    }
    Some(names_match)
}

/// Trivial writer: exactly one arg, body is `Ivasgn` with the arg's lvar as value.
/// DSL writers (no trailing `=`) are skipped when `allow_dsl_writers` is true.
/// Returns `Some(names_match)` if this is a trivial writer, where `names_match`
/// indicates whether ivar name == method base name (safe to autocorrect).
/// Returns `None` if not a trivial writer.
fn trivial_writer_names_match(
    method_name: &str,
    args: &[NodeId],
    body_id: NodeId,
    cx: &Cx<'_>,
    opts: &TrivialAccessorsOptions,
) -> Option<bool> {
    if args.len() != 1 {
        return None;
    }

    // DSL writer: method with one arg that doesn't end in `=`.
    if opts.allow_dsl_writers && !method_name.ends_with('=') {
        return None;
    }

    // The argument must be a plain `Arg`.
    let NodeKind::Arg(arg_sym) = *cx.kind(args[0]) else {
        return None;
    };

    // Body must be `Ivasgn` assigning the arg's lvar.
    let NodeKind::Ivasgn {
        name: ivar_sym,
        value,
    } = *cx.kind(body_id)
    else {
        return None;
    };
    let val_id = value.get()?;
    let NodeKind::Lvar(val_sym) = *cx.kind(val_id) else {
        return None;
    };
    if val_sym != arg_sym {
        return None;
    }

    let ivar_name = cx.symbol_str(ivar_sym);
    let method_base = method_name.trim_end_matches('=');
    let names_match = ivar_name
        .strip_prefix('@')
        .is_some_and(|n| n == method_base);

    // ExactNameMatch: ivar name must match method name (sans trailing `=`).
    if opts.exact_name_match && !names_match {
        return None;
    }

    Some(names_match)
}

fn autocorrect(node: NodeId, cx: &Cx<'_>, kind: &str, method_name: &str, is_singleton: bool) {
    let node_range = cx.range(node);
    let attr_name = method_name.trim_end_matches('=');

    if is_singleton {
        // def self.foo → class << self\n  attr_reader :foo\nend
        let indent = compute_indent(cx, node_range.start);
        let replacement = format!("class << self\n{indent}  attr_{kind} :{attr_name}\n{indent}end");
        cx.emit_edit(node_range, &replacement);
    } else {
        cx.emit_edit(node_range, &format!("attr_{kind} :{attr_name}"));
    }
}

/// Compute the indent string for a node by extracting the actual leading
/// whitespace (spaces and tabs) from the node's line in the source buffer.
/// This preserves tabs and is safe with multi-byte characters.
fn compute_indent(cx: &Cx<'_>, node_start: u32) -> String {
    let src = cx.source().as_bytes();
    let start = node_start as usize;
    // Find start of the current line.
    let line_start = src[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    // Extract the actual leading whitespace up to the node start.
    let leading = &src[line_start..start];
    leading
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .map(|&b| b as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{TrivialAccessors, TrivialAccessorsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic reader offense (inside class) ---

    #[test]
    fn flags_trivial_reader_in_class() {
        test::<TrivialAccessors>().expect_offense(indoc! {r#"
            class Foo
              def foo
              ^^^ Use `attr_reader` to define trivial reader methods.
                @foo
              end
            end
        "#});
    }

    #[test]
    fn flags_trivial_writer_in_class() {
        test::<TrivialAccessors>().expect_offense(indoc! {r#"
            class Foo
              def bar=(val)
              ^^^ Use `attr_writer` to define trivial writer methods.
                @bar = val
              end
            end
        "#});
    }

    // --- Autocorrect: reader ---

    #[test]
    fn corrects_trivial_reader() {
        test::<TrivialAccessors>().expect_correction(
            indoc! {r#"
                class Foo
                  def foo
                  ^^^ Use `attr_reader` to define trivial reader methods.
                    @foo
                  end
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_reader :foo
                end
            "#},
        );
    }

    #[test]
    fn corrects_trivial_writer() {
        test::<TrivialAccessors>().expect_correction(
            indoc! {r#"
                class Foo
                  def bar=(val)
                  ^^^ Use `attr_writer` to define trivial writer methods.
                    @bar = val
                  end
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_writer :bar
                end
            "#},
        );
    }

    // --- Singleton method ---

    #[test]
    fn flags_trivial_reader_singleton() {
        test::<TrivialAccessors>().expect_offense(indoc! {r#"
            class Foo
              def self.baz
              ^^^ Use `attr_reader` to define trivial reader methods.
                @baz
              end
            end
        "#});
    }

    #[test]
    fn corrects_singleton_reader() {
        test::<TrivialAccessors>().expect_correction(
            indoc! {r#"
                class Foo
                  def self.baz
                  ^^^ Use `attr_reader` to define trivial reader methods.
                    @baz
                  end
                end
            "#},
            indoc! {r#"
                class Foo
                  class << self
                    attr_reader :baz
                  end
                end
            "#},
        );
    }

    // --- No offense: top-level def ---

    #[test]
    fn no_offense_top_level_def() {
        // top_level_node? — parent is nil → skip
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            def foo
              @foo
            end
        "#});
    }

    // --- No offense: inside module ---

    #[test]
    fn no_offense_inside_module() {
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            module Foo
              def foo
                @foo
              end
            end
        "#});
    }

    // --- No offense: predicate allowed (default) ---

    #[test]
    fn no_offense_predicate_method_default() {
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            class Foo
              def foo?
                @foo
              end
            end
        "#});
    }

    // --- No offense: DSL writer allowed (default) ---

    #[test]
    fn no_offense_dsl_writer_default() {
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            class Foo
              def on_exception(action)
                @on_exception = action
              end
            end
        "#});
    }

    // --- No offense: exact name mismatch (default ExactNameMatch=true) ---

    #[test]
    fn no_offense_exact_name_mismatch_default() {
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            class Foo
              def name
                @other_name
              end
            end
        "#});
    }

    // --- No offense: allowed methods ---

    #[test]
    fn no_offense_to_s_allowed() {
        test::<TrivialAccessors>().expect_no_offenses(indoc! {r#"
            class Foo
              def to_s
                @to_s
              end
            end
        "#});
    }

    // --- IgnoreClassMethods: true ---

    #[test]
    fn no_offense_singleton_when_ignore_class_methods() {
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                ignore_class_methods: true,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                class Foo
                  def self.baz
                    @baz
                  end
                end
            "#});
    }

    // --- AllowPredicates: false ---

    #[test]
    fn flags_predicate_when_allow_predicates_false_default_exact_match() {
        // With allow_predicates=false and ExactNameMatch=true (default),
        // `def foo?; @foo; end` fires because `foo?` strips to `foo` == `@foo[1..]`.
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                allow_predicates: false,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                class Foo
                  def foo?
                  ^^^ Use `attr_reader` to define trivial reader methods.
                    @foo
                  end
                end
            "#});
    }

    #[test]
    fn flags_predicate_when_allow_predicates_false() {
        // With allow_predicates=false and exact_name_match=false, a predicate
        // method that returns a non-matching ivar also fires.
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                allow_predicates: false,
                exact_name_match: false,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                class Foo
                  def foo?
                  ^^^ Use `attr_reader` to define trivial reader methods.
                    @bar
                  end
                end
            "#});
    }

    // --- ExactNameMatch: false ---

    #[test]
    fn flags_name_mismatch_when_exact_match_false() {
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                exact_name_match: false,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                class Foo
                  def name
                  ^^^ Use `attr_reader` to define trivial reader methods.
                    @other_name
                  end
                end
            "#});
    }

    // --- AllowDSLWriters: false ---

    #[test]
    fn flags_dsl_writer_when_option_false() {
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                allow_dsl_writers: false,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                class Foo
                  def on_exception(action)
                  ^^^ Use `attr_writer` to define trivial writer methods.
                    @on_exception = action
                  end
                end
            "#});
    }

    // --- No autocorrect when names don't match (ExactNameMatch: false) ---

    #[test]
    fn no_autocorrect_when_names_mismatch() {
        // With ExactNameMatch=false, offense fires but autocorrect is NOT emitted
        // because replacing `def name; @other_name` with `attr_reader :name` would
        // change behavior (ivar @name vs @other_name).
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                exact_name_match: false,
                ..Default::default()
            })
            .expect_no_corrections(indoc! {r#"
                class Foo
                  def name
                    @other_name
                  end
                end
            "#});
    }

    // --- No autocorrect for DSL writer (AllowDSLWriters: false) ---

    #[test]
    fn no_autocorrect_for_dsl_writer() {
        // With AllowDSLWriters=false, offense fires but autocorrect is NOT emitted
        // because `attr_writer :on_exception` would change the method signature from
        // `on_exception(action)` to `on_exception=(value)`, breaking callers.
        test::<TrivialAccessors>()
            .with_options(&TrivialAccessorsOptions {
                allow_dsl_writers: false,
                ..Default::default()
            })
            .expect_no_corrections(indoc! {r#"
                class Foo
                  def on_exception(action)
                    @on_exception = action
                  end
                end
            "#});
    }

    // --- No autocorrect for non-self singleton receiver ---

    #[test]
    fn flags_non_self_singleton_receiver() {
        // `def SomeConst.foo` inside a class is detected but NOT autocorrected —
        // the receiver is not `self`, so `class << self` would change the target.
        test::<TrivialAccessors>().expect_offense(indoc! {r#"
            class Bar
              def SomeConst.foo
              ^^^ Use `attr_reader` to define trivial reader methods.
                @foo
              end
            end
        "#});
    }

    #[test]
    fn no_autocorrect_for_non_self_singleton_receiver() {
        test::<TrivialAccessors>().expect_no_corrections(indoc! {r#"
            class Bar
              def SomeConst.foo
                @foo
              end
            end
        "#});
    }

    // --- CopOptions config tests ---

    #[test]
    fn config_parse_error() {
        let err = <TrivialAccessorsOptions as murphy_plugin_api::CopOptions>::from_config_json(
            b"not json",
        )
        .expect_err("invalid json");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::Parse { .. }
        ));
    }

    #[test]
    fn config_not_object_error() {
        let err =
            <TrivialAccessorsOptions as murphy_plugin_api::CopOptions>::from_config_json(b"true")
                .expect_err("not an object");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::NotAnObject
        ));
    }

    #[test]
    fn config_type_mismatch_error() {
        let err = <TrivialAccessorsOptions as murphy_plugin_api::CopOptions>::from_config_json(
            br#"{"ExactNameMatch":"yes"}"#,
        )
        .expect_err("string is not bool");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "ExactNameMatch");
        assert_eq!(*expected, "bool");
    }
}

murphy_plugin_api::submit_cop!(TrivialAccessors);
