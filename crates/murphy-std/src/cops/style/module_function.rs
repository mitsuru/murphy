//! `Style/ModuleFunction` — enforces consistent use of `module_function` vs
//! `extend self` in modules.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ModuleFunction
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Supports all three EnforcedStyle values: module_function (default),
//!   extend_self, and forbidden.
//!   module_function style: flags `extend self` nodes in module bodies,
//!   unless any `private` directive exists in the module body (in that case
//!   extend self and module_function have different semantics for private
//!   methods, so the offense is suppressed).
//!   extend_self style: flags bare `module_function` (no arguments) nodes.
//!   `module_function` with arguments (e.g. `module_function :foo`) is not
//!   flagged because that form designates specific methods — it has no
//!   equivalent in extend_self style.
//!   forbidden style: flags both `extend self` and bare `module_function`.
//!   Only Module nodes are checked; Class nodes are skipped.
//!   Autocorrect is NOT provided. RuboCop marks autocorrect unsafe
//!   (SafeAutoCorrect: false) because the two idioms have subtle behavioral
//!   differences around private methods.
//! ```
//!
//! ## Matched shapes
//!
//! - `module_function` style: `send nil :extend (self)` inside module body,
//!   when no `private` directive is present.
//! - `extend_self` style: `send nil :module_function` with no arguments.
//! - `forbidden` style: both of the above.
//!
//! ## Examples
//!
//! ```ruby
//! # EnforcedStyle: module_function (default)
//! # bad
//! module Foo
//!   extend self
//!   def bar; end
//! end
//!
//! # good
//! module Foo
//!   module_function
//!   def bar; end
//! end
//!
//! # also good (has private — semantics differ)
//! module Foo
//!   extend self
//!   private
//!   def secret; end
//! end
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const MODULE_FUNCTION_MSG: &str = "Use `module_function` instead of `extend self`.";
const EXTEND_SELF_MSG: &str = "Use `extend self` instead of `module_function`.";
const FORBIDDEN_MSG: &str = "Do not use `module_function` or `extend self`.";

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ModuleFunctionStyle {
    #[default]
    #[option(value = "module_function")]
    ModuleFunction,
    #[option(value = "extend_self")]
    ExtendSelf,
    #[option(value = "forbidden")]
    Forbidden,
}

#[derive(CopOptions)]
pub struct ModuleFunctionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "module_function",
        description = "Enforces which style to use when declaring module functions."
    )]
    pub enforced_style: ModuleFunctionStyle,
}

#[derive(Default)]
pub struct ModuleFunction;

#[cop(
    name = "Style/ModuleFunction",
    description = "Checks for usage of `extend self` in modules.",
    default_severity = "warning",
    default_enabled = true,
    options = ModuleFunctionOptions,
)]
impl ModuleFunction {
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>, opts: &ModuleFunctionOptions) {
        check(node, cx, opts);
    }
}

/// Returns true if the node is `extend self` — i.e., a Send with no receiver,
/// method `extend`, and a single argument that is `self`.
fn is_extend_self(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    if receiver.get().is_some() {
        return false;
    }
    if cx.symbol_str(method) != "extend" {
        return false;
    }
    let arg_ids = cx.list(args);
    if arg_ids.len() != 1 {
        return false;
    }
    matches!(*cx.kind(arg_ids[0]), NodeKind::SelfExpr)
}

/// Returns true if the node is a bare `module_function` — a Send with no receiver,
/// method `module_function`, and no arguments.
fn is_bare_module_function(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    if receiver.get().is_some() {
        return false;
    }
    if cx.symbol_str(method) != "module_function" {
        return false;
    }
    cx.list(args).is_empty()
}

/// Returns true if any child of the module body is a `private` directive
/// (a Send with no receiver, method `private`, with or without arguments).
fn has_private_directive(body_id: NodeId, cx: &Cx<'_>) -> bool {
    let children: &[NodeId] = if let NodeKind::Begin(list) = *cx.kind(body_id) {
        cx.list(list)
    } else {
        return false;
    };

    children.iter().any(|&child| {
        if let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(child)
        {
            receiver.get().is_none() && cx.symbol_str(method) == "private"
        } else {
            false
        }
    })
}

fn check(node: NodeId, cx: &Cx<'_>, opts: &ModuleFunctionOptions) {
    let NodeKind::Module { body, .. } = *cx.kind(node) else {
        return;
    };

    let Some(body_id) = body.get() else {
        return;
    };

    // Collect children of the body.
    let children: &[NodeId] = if let NodeKind::Begin(list) = *cx.kind(body_id) {
        cx.list(list)
    } else {
        // Body is a single node (not a Begin); treat it as the only child.
        std::slice::from_ref(&body_id)
    };

    match opts.enforced_style {
        ModuleFunctionStyle::ModuleFunction => {
            // Flag `extend self`, unless a private directive is present.
            if has_private_directive(body_id, cx) {
                return;
            }
            for &child in children {
                if is_extend_self(child, cx) {
                    cx.emit_offense(cx.range(child), MODULE_FUNCTION_MSG, None);
                }
            }
        }
        ModuleFunctionStyle::ExtendSelf => {
            // Flag bare `module_function` (no args).
            for &child in children {
                if is_bare_module_function(child, cx) {
                    cx.emit_offense(cx.range(child), EXTEND_SELF_MSG, None);
                }
            }
        }
        ModuleFunctionStyle::Forbidden => {
            // Flag both `extend self` and bare `module_function`.
            for &child in children {
                if is_extend_self(child, cx) || is_bare_module_function(child, cx) {
                    cx.emit_offense(cx.range(child), FORBIDDEN_MSG, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModuleFunction, ModuleFunctionOptions, ModuleFunctionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn extend_self_opts() -> ModuleFunctionOptions {
        ModuleFunctionOptions {
            enforced_style: ModuleFunctionStyle::ExtendSelf,
        }
    }

    fn forbidden_opts() -> ModuleFunctionOptions {
        ModuleFunctionOptions {
            enforced_style: ModuleFunctionStyle::Forbidden,
        }
    }

    // ── module_function style (default) ──────────────────────────────────────

    #[test]
    fn flags_extend_self_in_module() {
        test::<ModuleFunction>().expect_offense(indoc! {"
            module Foo
              extend self
              ^^^^^^^^^^^ Use `module_function` instead of `extend self`.
              def bar; end
            end
        "});
    }

    #[test]
    fn accepts_extend_self_with_bare_private() {
        // `private` directive present → extend self and module_function differ
        // for private methods; no offense.
        test::<ModuleFunction>().expect_no_offenses(indoc! {"
            module Foo
              extend self
              private
              def secret; end
            end
        "});
    }

    #[test]
    fn accepts_extend_self_with_declarative_private() {
        // `private :method` is also a private directive.
        test::<ModuleFunction>().expect_no_offenses(indoc! {"
            module Foo
              extend self
              private :secret
              def secret; end
            end
        "});
    }

    #[test]
    fn accepts_extend_self_in_class() {
        // Only modules are checked.
        test::<ModuleFunction>().expect_no_offenses(indoc! {"
            class Foo
              extend self
            end
        "});
    }

    #[test]
    fn accepts_module_function_in_module() {
        test::<ModuleFunction>().expect_no_offenses(indoc! {"
            module Foo
              module_function
              def bar; end
            end
        "});
    }

    // ── extend_self style ────────────────────────────────────────────────────

    #[test]
    fn extend_self_style_flags_bare_module_function() {
        test::<ModuleFunction>()
            .with_options(&extend_self_opts())
            .expect_offense(indoc! {"
                module Foo
                  module_function
                  ^^^^^^^^^^^^^^^ Use `extend self` instead of `module_function`.
                  def bar; end
                end
            "});
    }

    #[test]
    fn extend_self_style_accepts_module_function_with_args() {
        // `module_function :foo` with arguments is not flagged.
        test::<ModuleFunction>()
            .with_options(&extend_self_opts())
            .expect_no_offenses(indoc! {"
                module Foo
                  module_function :bar, :baz
                  def bar; end
                end
            "});
    }

    #[test]
    fn extend_self_style_accepts_extend_self() {
        test::<ModuleFunction>()
            .with_options(&extend_self_opts())
            .expect_no_offenses(indoc! {"
                module Foo
                  extend self
                  def bar; end
                end
            "});
    }

    // ── forbidden style ──────────────────────────────────────────────────────

    #[test]
    fn forbidden_style_flags_extend_self() {
        test::<ModuleFunction>()
            .with_options(&forbidden_opts())
            .expect_offense(indoc! {"
                module Foo
                  extend self
                  ^^^^^^^^^^^ Do not use `module_function` or `extend self`.
                  def bar; end
                end
            "});
    }

    #[test]
    fn forbidden_style_flags_extend_self_with_private() {
        // In forbidden style, private directive does NOT suppress the offense.
        test::<ModuleFunction>()
            .with_options(&forbidden_opts())
            .expect_offense(indoc! {"
                module Foo
                  extend self
                  ^^^^^^^^^^^ Do not use `module_function` or `extend self`.
                  private
                  def secret; end
                end
            "});
    }

    #[test]
    fn forbidden_style_flags_bare_module_function() {
        test::<ModuleFunction>()
            .with_options(&forbidden_opts())
            .expect_offense(indoc! {"
                module Foo
                  module_function
                  ^^^^^^^^^^^^^^^ Do not use `module_function` or `extend self`.
                  def bar; end
                end
            "});
    }

    #[test]
    fn forbidden_style_accepts_extend_self_in_class() {
        test::<ModuleFunction>()
            .with_options(&forbidden_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  extend self
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(ModuleFunction);
