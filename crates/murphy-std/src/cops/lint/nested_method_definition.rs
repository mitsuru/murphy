//! `Lint/NestedMethodDefinition` — flags method definitions inside other methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NestedMethodDefinition
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/NestedMethodDefinition. A nested `def`/`defs`
//!   fires when any ancestor is a `def`/`defs`, unless an ancestor
//!   `sclass` or scoping block suppresses it: `instance_eval`/
//!   `class_eval`/`module_eval`, `instance_exec`/`class_exec`/
//!   `module_exec`, `Class.new`/`Module.new`/`Struct.new`/`Data.define`,
//!   or a block whose method name matches `AllowedMethods` /
//!   `AllowedPatterns`. A `defs` whose definee is a variable, constant,
//!   or call (`def obj.foo`) is allowed, matching RuboCop's
//!   `allowed_subject_type?`.
//! ```
//!
//! ## Matched shapes
//!
//! - `def outer; def inner; end; end` — method inside method
//! - `def self.x; def self.y; end; end` — nested class method
//! - `def foo; -> { def bar; end }; end` — nested method inside lambda
//!
//! ## Excluded shapes (not flagged)
//!
//! - `def foo; def obj.bar; end; end` — defining on a variable/const/call
//! - `def foo; def @ivar.bar; end; end` — defining on instance variable
//! - `def foo; def Const.bar; end; end` — defining on constant
//! - `def foo; instance_eval { def bar; end }; end` — inside eval block
//! - `def foo; class_eval { def bar; end }; end` — inside class_eval block
//! - `def foo; module_eval { def bar; end }; end` — inside module_eval block
//! - `def foo; instance_exec { def bar; end }; end` — inside exec block
//! - `def foo; class_exec { def bar; end }; end` — inside class_exec block
//! - `def foo; module_exec { def bar; end }; end` — inside module_exec block
//! - `def foo; class << self; def bar; end; end; end` — inside sclass
//! - `def foo; Class.new { def bar; end }; end` — inside class constructor
//! - `def foo; Module.new { def bar; end }; end` — inside class constructor
//! - `def foo; Struct.new { def bar; end }; end` — inside class constructor
//! - `def foo; Data.define { def bar; end }; end` — inside class constructor
//!
//! ## No autocorrect
//!
//! Autocorrect is not provided because converting a nested method
//! definition to a lambda (or restructuring) is non-trivial and not
//! amenable to safe automated fix.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Method definitions must not be nested. Use `lambda` instead.";

const EVAL_EXEC_METHODS: &[&str] = &[
    "instance_eval",
    "class_eval",
    "module_eval",
    "instance_exec",
    "class_exec",
    "module_exec",
];

/// Returns `true` if the receiver of a singleton method definition is a
/// variable, constant, or method call — those indicate a method being
/// defined on a specific object, not a global method definition.
fn is_allowed_def_receiver(receiver_id: NodeId, cx: &Cx<'_>) -> bool {
    let mut id = receiver_id;
    while let NodeKind::Begin(list) = *cx.kind(id) {
        let children = cx.list(list);
        if children.len() == 1 {
            id = children[0];
        } else {
            break;
        }
    }
    matches!(
        *cx.kind(id),
        NodeKind::Lvar(_)
            | NodeKind::Ivar(_)
            | NodeKind::Cvar(_)
            | NodeKind::Gvar(_)
            | NodeKind::Const { .. }
            | NodeKind::Send { .. }
            | NodeKind::Csend { .. }
    )
}

/// Returns `true` when `block_id` (a Block/Numblock/Itblock) is a scoping
/// construct that makes nested method definitions safe: eval/exec calls,
/// class constructors (Class.new, Module.new, Struct.new, Data.define),
/// or a block whose method name matches `AllowedMethods`/`AllowedPatterns`.
fn is_scoping_block(block_id: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_class_constructor(block_id) {
        return true;
    }
    if let Some(m) = cx.method_name(block_id)
        && EVAL_EXEC_METHODS.contains(&m)
    {
        return true;
    }
    is_allowed_block_method(block_id, cx)
}

/// RuboCop's `allowed_method_name?`: the scoping block's method name matches
/// `AllowedMethods` exactly or any `AllowedPatterns` regex.
fn is_allowed_block_method(block_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(name) = cx.method_name(block_id) else {
        return false;
    };
    let opts = cx.options_or_default::<Options>();
    opts.allowed_methods.iter().any(|m| m == name)
        || cx.matches_any_pattern(name, &opts.allowed_patterns)
}

#[derive(Default)]
pub struct NestedMethodDefinition;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Block method names inside which nested method definitions are allowed."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns of block method names inside which nested method definitions are allowed."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Lint/NestedMethodDefinition",
    description = "Do not nest method definitions inside other methods.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl NestedMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { receiver, .. } = *cx.kind(node) else {
            return;
        };

        // Singleton method with a receiver that is a variable/const/call:
        // `def obj.method` inside another method defines on a specific
        // object, not creating a nested global method.
        if let Some(recv) = receiver.get()
            && is_allowed_def_receiver(recv, cx) {
                return;
            }

        check_nesting(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Defs { receiver, .. } = *cx.kind(node) else {
            return;
        };

        // Same as check_def: singleton methods with variable/const/call
        // receivers define on specific objects.
        if is_allowed_def_receiver(receiver, cx) {
            return;
        }

        check_nesting(node, cx);
    }
}

/// Core check: walk ancestors looking for a Def/Defs ancestor (nesting)
/// and for scoping constructs that suppress the offense.
fn check_nesting(node: NodeId, cx: &Cx<'_>) {
    let has_def_ancestor = cx.ancestors(node).any(|a| {
        matches!(
            *cx.kind(a),
            NodeKind::Def { .. } | NodeKind::Defs { .. }
        )
    });

    if !has_def_ancestor {
        return;
    }

    // Check for scoping ancestors (block with eval/exec/class_constructor,
    // or sclass) anywhere between this node and the root.
    let within_scoping = cx.ancestors(node).any(|ancestor| {
        match *cx.kind(ancestor) {
            NodeKind::Sclass { .. } => true,
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. } => is_scoping_block(ancestor, cx),
            _ => false,
        }
    });

    if !within_scoping {
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

murphy_plugin_api::submit_cop!(NestedMethodDefinition);

#[cfg(test)]
mod tests {
    use super::{NestedMethodDefinition, Options};
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_options, test};

    // Helper: count offenses for a given source.
    fn hits(source: &str) -> usize {
        run_cop::<NestedMethodDefinition>(source).len()
    }

    // ── positive cases ──────────────────────────────────────────────────────

    #[test]
    fn flags_nested_oneliner() {
        test::<NestedMethodDefinition>().expect_offense(indoc! {r#"
            def x; def y; end; end
                   ^^^^^^^^^^ Method definitions must not be nested. Use `lambda` instead.
        "#});
    }

    #[test]
    fn flags_nested_method_multi_line() {
        assert_eq!(
            hits(indoc! {r#"
                def x
                  def y
                  end
                end
            "#}),
            1
        );
    }

    #[test]
    fn flags_nested_singleton_method() {
        assert_eq!(
            hits(indoc! {r#"
                class Foo
                end
                foo = Foo.new
                def foo.bar
                  def baz
                  end
                end
            "#}),
            1
        );
    }

    #[test]
    fn flags_nested_class_method() {
        assert_eq!(
            hits(indoc! {r#"
                class Foo
                  def self.x
                    def self.y
                    end
                  end
                end
            "#}),
            1
        );
    }

    #[test]
    fn flags_nested_inside_lambda() {
        test::<NestedMethodDefinition>().expect_offense(indoc! {r#"
            def foo
              bar = -> { def baz; puts; end }
                         ^^^^^^^^^^^^^^^^^^ Method definitions must not be nested. Use `lambda` instead.
            end
        "#});
    }

    // ── negative cases: lambda ──────────────────────────────────────────────

    #[test]
    fn does_not_flag_lambda_definition() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            def foo
              bar = -> { puts }
              bar.call
            end
        "#});
    }

    // ── negative cases: eval/exec scoping ───────────────────────────────────

    #[test]
    fn does_not_flag_instance_eval() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x(obj)
                obj.instance_eval do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_instance_exec() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x(obj)
                obj.instance_exec do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_class_eval() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x(klass)
                klass.class_eval do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_class_exec() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x(klass)
                klass.class_exec do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_module_eval() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define(mod)
                mod.module_eval do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_module_exec() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define(mod)
                mod.module_exec do
                  def y
                  end
                end
              end
            end
        "#});
    }

    // ── negative cases: def on variable/const/call ───────────────────────────

    #[test]
    fn does_not_flag_def_on_local_var() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x(obj)
                def obj.y
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_def_on_instance_var() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x
                def @obj.y
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_def_on_class_var() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x
                def @@obj.y
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_def_on_global_var() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x
                def $obj.y
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_def_on_constant() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x
                def Const.y
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_def_on_method_call() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def x
                def do_something.y
                end
              end
            end
        "#});
    }

    // ── negative cases: sclass ──────────────────────────────────────────────

    #[test]
    fn does_not_flag_class_shovel() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def bar
                class << self
                  def baz
                  end
                end
              end
            end
        "#});
    }

    // ── negative cases: class constructors ──────────────────────────────────

    #[test]
    fn does_not_flag_class_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                Class.new(S) do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_double_colon_class_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                ::Class.new(S) do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_module_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                Module.new do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_double_colon_module_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                ::Module.new do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_struct_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                Struct.new(:name) do
                  def y
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_double_colon_struct_new() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo
              def self.define
                ::Struct.new(:name) do
                  def y
                  end
                end
              end
            end
        "#});
    }

    // ── edge cases ─────────────────────────────────────────────────────────

    #[test]
    fn does_not_flag_top_level_method() {
        test::<NestedMethodDefinition>().expect_no_offenses(indoc! {r#"
            def foo
            end
        "#});
    }

    #[test]
    fn flags_triple_nested_method() {
        assert_eq!(
            hits(indoc! {r#"
                def a
                  def b
                    def c
                    end
                  end
                end
            "#}),
            2
        );
    }

    // ── AllowedMethods / AllowedPatterns ────────────────────────────────────

    #[test]
    fn flags_nested_in_plain_block_by_default() {
        assert_eq!(
            hits(indoc! {r#"
                def do_something
                  has_many :articles do
                    def find_or_create_by_name(name)
                    end
                  end
                end
            "#}),
            1
        );
    }

    #[test]
    fn does_not_flag_allowed_method_block() {
        let opts = Options {
            allowed_methods: vec!["has_many".to_string()],
            allowed_patterns: vec![],
        };
        test::<NestedMethodDefinition>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def do_something
                  has_many :articles do
                    def find_or_create_by_name(name)
                    end
                  end
                end
            "#});
    }

    #[test]
    fn allowed_methods_is_exact_match() {
        let opts = Options {
            allowed_methods: vec!["has".to_string()],
            allowed_patterns: vec![],
        };
        assert_eq!(
            run_cop_with_options::<NestedMethodDefinition>(
                indoc! {r#"
                    def do_something
                      has_many :articles do
                        def find_or_create_by_name(name)
                        end
                      end
                    end
                "#},
                &opts
            )
            .len(),
            1
        );
    }

    #[test]
    fn does_not_flag_allowed_pattern_block() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["baz".to_string()],
        };
        test::<NestedMethodDefinition>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def foo(obj)
                  obj.do_baz do
                    def bar
                    end
                  end
                end
            "#});
    }

    #[test]
    fn allowed_pattern_is_regex() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["^do_.*$".to_string()],
        };
        test::<NestedMethodDefinition>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def foo(obj)
                  obj.do_baz do
                    def bar
                    end
                  end
                end
            "#});
    }

    #[test]
    fn allowed_pattern_non_match_still_flags() {
        let opts = Options {
            allowed_methods: vec![],
            allowed_patterns: vec!["qux".to_string()],
        };
        assert_eq!(
            run_cop_with_options::<NestedMethodDefinition>(
                indoc! {r#"
                    def foo(obj)
                      obj.do_baz do
                        def bar
                        end
                      end
                    end
                "#},
                &opts
            )
            .len(),
            1
        );
    }

    #[test]
    fn allowed_method_covers_outer_block_ancestor() {
        // Any scoping ancestor suppresses the offense, even when an inner
        // non-allowed block sits between the def and the allowed block.
        let opts = Options {
            allowed_methods: vec!["has_many".to_string()],
            allowed_patterns: vec![],
        };
        test::<NestedMethodDefinition>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def do_something
                  has_many :articles do
                    foo do
                      def bar
                      end
                    end
                  end
                end
            "#});
    }
}
