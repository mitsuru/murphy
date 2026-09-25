//! `Rails/LexicallyScopedActionFilter` — flag `only:`/`except:` actions not defined in the class.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/LexicallyScopedActionFilter
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Send-dispatch port of RuboCop's on_send with RESTRICT_ON_SEND gating.
//!   Bare-call gating, whole-send offense range, and backtick message all
//!   mirror upstream. Defined methods (direct `def` children with no
//!   receiver), `delegate` with `:to`, and `alias`/`alias_method` (old->new)
//!   are all implemented. File scope (`Include: controllers/mailers`) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//!   default.yml, murphy-4gd.1.15); hash matching stays permissive (any
//!   hash arg / any pair position) to avoid false negatives for
//!   `only: ..., if: ...` multi-option calls.
//! ```
//!
//! Checks that methods specified in the filter's `only` or `except` options
//! are defined within the same class or module.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=None, method in FILTERS, args=[_, hash, ...?])` — a bare
//! call to one of the 13 action-filter methods with a trailing options hash
//! containing an `only:` or `except:` pair whose value is a sym, str, or
//! array of sym/str.
//!
//! Upstream NodePattern is exact:
//! `(send nil? {filters} _ (hash (pair (sym {:only :except}) $_)))`
//! (exactly 2 args, single-pair hash). Murphy is permissive here: any hash
//! argument containing an `only:`/`except:` pair matches, and extra pairs
//! (`if:`, `unless:`) or extra filter names do not suppress the check.
//! All upstream spec cases are 2-arg single-pair, so parity holds and the
//! permissive form only removes false negatives.
//!
//! ## Lexical scope
//!
//! The nearest ancestor `Class`/`Module` supplies the scope (mirrors
//! `node.each_ancestor(:class, :module).first`). No ancestor means no
//! offense (top-level filter calls are ignored, as upstream). `Sclass`
//! (`class << self`) is not a scope, matching upstream.
//!
//! Defined actions are the direct `Def` children (receiver `None` only —
//! `def self.foo` is `defs` upstream and does not count) of the scope's
//! `Begin` body, plus `delegate` targets and `alias`/`alias_method` new
//! names. A single-statement body (no `Begin`) yields zero defined methods,
//! so a lone `before_action` with `only:` always flags (matches upstream
//! spec "when no methods are defined").
//!
//! ## Offense range and message
//!
//! Whole outer send (`cx.range(node)`), message
//! `` `x` is not explicitly defined on the class. `` (singular) or
//! `` `x`, `y` are not explicitly defined on the class. `` (plural),
//! with `class`/`module` from the scope node. No autocorrect (upstream
//! has none; the fix requires defining the action).
//!
//! ## Known limitation
//!
//! Upstream `Include: ['**/app/controllers/**/*.rb', '**/app/mailers/**/*.rb']`
//! is enforced via the murphy-rails pack default.yml (engine
//! `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//! default.yml, murphy-4gd.1.15).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Upstream `RESTRICT_ON_SEND` (rubocop-rails 2.35.0).
const FILTER_METHODS: &[&str] = &[
    "after_action",
    "append_after_action",
    "append_around_action",
    "append_before_action",
    "around_action",
    "before_action",
    "prepend_after_action",
    "prepend_around_action",
    "prepend_before_action",
    "skip_after_action",
    "skip_around_action",
    "skip_before_action",
    "skip_action_callback",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LexicallyScopedActionFilter;

#[cop(
    name = "Rails/LexicallyScopedActionFilter",
    description = "Checks that methods specified in the filter's `only` or `except` options are explicitly defined in the class.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl LexicallyScopedActionFilter {
    // `methods = [...]` mirrors upstream `RESTRICT_ON_SEND` — dispatch
    // only on candidate selectors. The body still gates on bare receiver
    // and only/except hash shape.
    #[on_node(kind = "send", methods = [
        "after_action",
        "append_after_action",
        "append_around_action",
        "append_before_action",
        "around_action",
        "before_action",
        "prepend_after_action",
        "prepend_around_action",
        "prepend_before_action",
        "skip_after_action",
        "skip_around_action",
        "skip_before_action",
        "skip_action_callback",
    ])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(node)
        else {
            return;
        };
        // Bare call only — `(send nil? ...)` upstream. Explicit receivers
        // (`obj.before_action`, `self.before_action`) never match.
        if receiver.get().is_some() {
            return;
        }
        let method_str = cx.symbol_str(method);
        if !FILTER_METHODS.contains(&method_str) {
            return;
        }

        // Find the only:/except: value node (permissive: any hash arg,
        // any pair position — see module docs).
        let Some(value) = only_or_except_value(node, cx) else {
            return;
        };
        let referenced = array_values(value, cx);
        if referenced.is_empty() {
            return;
        }

        // Nearest enclosing class/module (upstream `each_ancestor`).
        let Some(scope) = cx
            .ancestors(node)
            .find(|&a| matches!(*cx.kind(a), NodeKind::Class { .. } | NodeKind::Module { .. }))
        else {
            return;
        };
        let (scope_type, statements) = match *cx.kind(scope) {
            NodeKind::Class { body, .. } => ("class", scope_statements(body, cx)),
            NodeKind::Module { body, .. } => ("module", scope_statements(body, cx)),
            _ => return,
        };

        let defined = defined_action_methods(&statements, cx);
        let unmatched: Vec<&str> = referenced
            .into_iter()
            .filter(|r| !defined.iter().any(|d| d == r))
            .collect();
        if unmatched.is_empty() {
            return;
        }

        let message = if unmatched.len() == 1 {
            format!("`{}` is not explicitly defined on the {}.", unmatched[0], scope_type)
        } else {
            let joined = unmatched.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ");
            format!("{joined} are not explicitly defined on the {scope_type}.")
        };
        cx.emit_offense(cx.range(node), &message, None);
    }
}

/// Direct-children statements of a class/module body.
///
/// Upstream `parent.each_child_node(:begin).first` yields `None` for a
/// single-statement body (no `Begin` wrapper), and `defined_action_methods`
/// then returns `[]`. Mirror that: only a `Begin` body contributes
/// statements; anything else (single `Send`/`Def`, empty) means zero
/// defined methods.
fn scope_statements(body: murphy_plugin_api::OptNodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(b) = body.get() else {
        return Vec::new();
    };
    match *cx.kind(b) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => Vec::new(),
    }
}

/// Collect a `Sym`/`Str` value as its action name.
///
/// `Sym(:foo)` → `"foo"`; `Str("foo")` → `"foo"` (upstream converts str
/// content `to_sym`, so string and symbol spellings compare equal).
fn sym_or_str_value<'a>(id: NodeId, cx: &'a Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
        NodeKind::Str(s) => Some(cx.string_str(s)),
        _ => None,
    }
}

/// Upstream `array_values`: `str`/`sym` → single, `array` → sym/str
/// members only, anything else → `[]` (no offense).
fn array_values<'a>(id: NodeId, cx: &'a Cx<'a>) -> Vec<&'a str> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => vec![cx.symbol_str(sym)],
        NodeKind::Str(s) => vec![cx.string_str(s)],
        NodeKind::Array(list) => cx
            .list(list)
            .iter()
            .filter_map(|&e| sym_or_str_value(e, cx))
            .collect(),
        _ => Vec::new(),
    }
}

/// Find the `only:`/`except:` value node in the filter call.
///
/// Permissive vs upstream (see module docs): scan all hash arguments for
/// the first `only:`/`except:` pair instead of requiring exactly
/// `(_, single-pair-hash)`.
fn only_or_except_value(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    for &arg in cx.call_arguments(node) {
        let NodeKind::Hash(pairs) = *cx.kind(arg) else {
            continue;
        };
        for &pair_id in cx.list(pairs) {
            let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                continue;
            };
            let NodeKind::Sym(key_sym) = *cx.kind(key) else {
                continue;
            };
            if matches!(cx.symbol_str(key_sym), "only" | "except") {
                return Some(value);
            }
        }
    }
    None
}

/// Upstream `defined_action_methods`: direct `def`s (receiver-less only)
/// plus delegated plus aliased-new names.
fn defined_action_methods(statements: &[NodeId], cx: &Cx<'_>) -> Vec<String> {
    let mut defined: Vec<String> = statements
        .iter()
        .filter_map(|&s| match *cx.kind(s) {
            NodeKind::Def { receiver, name, .. } if receiver.get().is_none() => {
                Some(cx.symbol_str(name).to_owned())
            }
            _ => None,
        })
        .collect();

    for &s in statements {
        for d in delegated_action_methods(s, cx) {
            defined.push(d);
        }
    }

    let alias_map = alias_methods(statements, cx);
    let mut aliased = Vec::new();
    for d in &defined {
        if let Some(n) = alias_map.get(d) {
            aliased.push(n.clone());
        }
    }
    defined.extend(aliased);
    defined
}

/// Upstream `delegated_action_methods` for one direct child: bare
/// `delegate :a, :b, to: :x` (plus extra hash pairs like `prefix:`).
/// Non-sym leading args or a missing `:to` hash mean no match.
fn delegated_action_methods(id: NodeId, cx: &Cx<'_>) -> Vec<String> {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return Vec::new();
    };
    if receiver.get().is_some() || cx.symbol_str(method) != "delegate" {
        return Vec::new();
    }
    let args = cx.call_arguments(id);
    if args.len() < 2 {
        return Vec::new();
    }
    let (names, hash_id) = match args.split_last() {
        Some((last, rest)) => (rest, *last),
        None => return Vec::new(),
    };
    if names.is_empty() {
        return Vec::new();
    }
    // All leading args must be syms (upstream `(sym $_)+`).
    let mut out = Vec::with_capacity(names.len());
    for &n in names {
        let NodeKind::Sym(sym) = *cx.kind(n) else {
            return Vec::new();
        };
        out.push(cx.symbol_str(sym).to_owned());
    }
    // Last arg must be a hash containing a `:to` pair (extra pairs OK —
    // upstream `<(pair (sym :to) _) ...>`).
    let NodeKind::Hash(pairs) = *cx.kind(hash_id) else {
        return Vec::new();
    };
    let has_to = cx.list(pairs).iter().any(|&p| match *cx.kind(p) {
        NodeKind::Pair { key, .. } => match *cx.kind(key) {
            NodeKind::Sym(k) => cx.symbol_str(k) == "to",
            _ => false,
        },
        _ => false,
    });
    if has_to { out } else { Vec::new() }
}

/// Upstream `alias_methods`: `old -> new` map from bare
/// `alias_method :new, :old` sends and `alias new old` keywords.
fn alias_methods(
    statements: &[NodeId],
    cx: &Cx<'_>,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for &s in statements {
        match *cx.kind(s) {
            NodeKind::Send {
                receiver, method, ..
            } if receiver.get().is_none() && cx.symbol_str(method) == "alias_method" => {
                let args = cx.call_arguments(s);
                if args.len() != 2 {
                    continue;
                }
                // `alias_method :new, :old` — first is new, last is old.
                let (Some(new), Some(old)) = (
                    sym_or_str_value(args[0], cx),
                    sym_or_str_value(args[1], cx),
                ) else {
                    continue;
                };
                map.insert(old.to_owned(), new.to_owned());
            }
            NodeKind::Alias { new_name, old_name } => {
                // `alias new old` — children are typically `Sym` nodes.
                let (Some(new), Some(old)) = (
                    alias_name(new_name, cx),
                    alias_name(old_name, cx),
                ) else {
                    continue;
                };
                map.insert(old.to_owned(), new.to_owned());
            }
            _ => {}
        }
    }
    map
}

/// Extract an `alias new old` endpoint name. Endpoints are `Sym` for
/// method aliases (upstream `old_identifier`/`new_identifier`); `Gvar`
/// global aliases are ignored (never action names).
fn alias_name<'a>(id: NodeId, cx: &'a Cx<'a>) -> Option<&'a str> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

murphy_plugin_api::submit_cop!(LexicallyScopedActionFilter);

#[cfg(test)]
mod tests {
    use super::LexicallyScopedActionFilter;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases (mirror upstream spec) ===

    #[test]
    fn flags_string_not_defined() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, except: 'health_check'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `health_check` is not explicitly defined on the class.
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn flags_symbol_not_defined() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class LoginController < ApplicationController
                  skip_before_action :require_login, only: :health_check
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `health_check` is not explicitly defined on the class.
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn flags_array_string_partial() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, only: %w[index settings]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `settings` is not explicitly defined on the class.
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn flags_array_symbol_multiple() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, only: %i[index settings logout]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `settings`, `logout` are not explicitly defined on the class.
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn flags_when_no_methods_defined() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, only: %i[index show]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `index`, `show` are not explicitly defined on the class.
                end
            "#});
    }

    #[test]
    fn flags_in_module_without_def() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                module FooMixin
                  extend ActiveSupport::Concern
                  included do
                    before_action proc { authenticate }, only: :foo
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `foo` is not explicitly defined on the module.
                  end
                end
            "#});
    }

    #[test]
    fn flags_alias_method_partial() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[foo show]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `foo` is not explicitly defined on the class.
                  def index
                  end
                  alias_method :show, :index
                  private
                  def authorize!
                  end
                end
            "#});
    }

    #[test]
    fn flags_delegate_partial() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[foo show]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `foo` is not explicitly defined on the class.
                  delegate :show, to: :bar
                end
            "#});
    }

    #[test]
    fn flags_alias_partial() {
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[foo show]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `foo` is not explicitly defined on the class.
                  def index
                  end
                  alias show index
                  private
                  def authorize!
                  end
                end
            "#});
    }

    // === no-offense cases ===

    #[test]
    fn no_offense_when_string_defined() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, except: 'health_check'
                  def health_check
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_symbol_defined() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  skip_before_action :require_login, only: :health_check
                  def health_check
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_array_strings_defined() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, only: %w[index settings]
                  def index
                  end
                  def settings
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_array_symbols_defined() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login, only: %i[index settings logout]
                  def index
                  end
                  def settings
                  end
                  def logout
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_aliased_by_alias_method() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[index show]
                  def index
                  end
                  alias_method :show, :index
                  private
                  def authorize!
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_delegated() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[index show]
                  delegate :index, :show, to: :foo
                end
            "#});
    }

    #[test]
    fn no_offense_when_aliased_by_alias() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class FooController < ApplicationController
                  before_action :authorize!, only: %i[index show]
                  def index
                  end
                  alias show index
                  private
                  def authorize!
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_when_conditional() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class Test < ActionController
                  before_action(:authenticate, only: %i[update cancel]) unless foo
                  def update; end
                  def cancel; end
                end
            "#});
    }

    #[test]
    fn no_offense_when_mixin_defines_action() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                module FooMixin
                  extend ActiveSupport::Concern
                  included do
                    before_action proc { authenticate }, only: :foo
                  end
                  def foo; end
                end
            "#});
    }

    #[test]
    fn no_offense_when_interpolated_array_defined() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class FooController < ApplicationController
                  before_action :foo, except: %I[index show]
                  def index
                  end
                  def show
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_without_only_or_except() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  before_action :require_login
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_with_explicit_receiver() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class LoginController < ApplicationController
                  obj.before_action :require_login, only: :missing
                  def index
                  end
                end
            "#});
    }

    #[test]
    fn no_offense_at_top_level() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(
            "before_action :require_login, only: :missing\n",
        );
    }

    #[test]
    fn no_offense_for_singleton_def_only() {
        // `def self.bar` is `defs` upstream — it does not count as a
        // lexically-scoped action, so `only: :bar` still flags.
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class C
                  before_action :foo, only: :bar
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `bar` is not explicitly defined on the class.
                  def self.bar; end
                end
            "#});
    }

    #[test]
    fn flags_multi_option_hash() {
        // Permissive improvement over upstream's single-pair pattern:
        // `only:` alongside `if:` still checks.
        test::<LexicallyScopedActionFilter>().expect_offense(indoc! {r#"
                class C
                  before_action :foo, only: :bar, if: :cond
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `bar` is not explicitly defined on the class.
                  def cond; end
                end
            "#});
    }

    #[test]
    fn no_offense_in_comments_and_strings() {
        test::<LexicallyScopedActionFilter>().expect_no_offenses(indoc! {r#"
                class C
                  # before_action :foo, only: :missing
                  x = "before_action :foo, only: :missing"
                  def foo; end
                end
            "#});
    }
}
