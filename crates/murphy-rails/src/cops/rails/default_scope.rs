//! `Rails/DefaultScope` — flag `default_scope` calls and `default_scope` method definitions.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DefaultScope
//! upstream_version_checked: 2.38.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Upstream ships `Enabled: false` (opt-in); that default lives in the
//!   rails pack's bundled `config/default.yml` layer, not in this file.
//!   Mirrors rubocop-rails 2.38.0: RESTRICT_ON_SEND [:default_scope] gating,
//!   bare `(send nil? :default_scope ...)` shape with selector-only offense
//!   range, `defs :default_scope` (singleton `def self.default_scope`) with
//!   name-only range, and `sclass (self) (def :default_scope ...)` eigenclass
//!   form. Plain instance `def default_scope` does not flag. `Def` with an
//!   explicit `self` receiver (`def self.default_scope` lowered as `Def`
//!   rather than `Defs` by some frontends) is also flagged. No autocorrect.
//! ```
//!
//! ## Matched shapes
//!
//! - `default_scope -> { where(hidden: false) }` → offense on `default_scope` selector.
//! - `def self.default_scope` (`Defs` or `Def` with `self` receiver) → offense on method name.
//! - `class << self; def default_scope; ...; end; end` → offense on inner `def` name.
//!
//! Plain `def default_scope` (instance method outside `class << self`) does not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DefaultScope;

#[cop(
    name = "Rails/DefaultScope",
    description = "Avoid use of `default_scope`. It is better to use explicitly named scopes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DefaultScope {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[default_scope]`.
    #[on_node(kind = "send", methods = ["default_scope"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `(send nil? :default_scope ...)`.
        if receiver.get().is_some() {
            return;
        }
        cx.emit_offense(
            cx.loc(node).name,
            "Avoid use of `default_scope`. It is better to use explicitly named scopes.",
            None,
        );
    }

    // Mirrors upstream `on_defs` (`def self.default_scope`).
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Defs { name, .. } = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(name) != "default_scope" {
            return;
        }
        cx.emit_offense(
            method_name_range(cx, node, "default_scope"),
            "Avoid use of `default_scope`. It is better to use explicitly named scopes.",
            None,
        );
    }

    // Covers `def self.default_scope` lowered as `Def` with receiver, plus
    // `class << self; def default_scope; end`.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { receiver, name, .. } = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(name) != "default_scope" {
            return;
        }
        if receiver.get().is_some() {
            // `def self.default_scope` (Def-form).
            cx.emit_offense(
                method_name_range(cx, node, "default_scope"),
                "Avoid use of `default_scope`. It is better to use explicitly named scopes.",
                None,
            );
            return;
        }
        // Bare `def default_scope` flags only inside `class << self`.
        if inside_self_sclass(cx, node) {
            cx.emit_offense(
                method_name_range(cx, node, "default_scope"),
                "Avoid use of `default_scope`. It is better to use explicitly named scopes.",
                None,
            );
        }
    }
}

/// Method-name range inside a `def` node: search `default_scope` within the
/// node source (translator does not populate `loc.name` for `Def`).
fn method_name_range(cx: &Cx<'_>, node: NodeId, name: &str) -> murphy_plugin_api::Range {
    let r = cx.range(node);
    let src = cx.source();
    if let Some(slice) = src.get(r.start as usize..r.end as usize)
        && let Some(off) = slice.find(name) {
            let s = r.start + off as u32;
            return murphy_plugin_api::Range {
                start: s,
                end: s + name.len() as u32,
            };
        }
    r
}

/// Walk ancestors for `class << self` (`Sclass` with `self` expression).
fn inside_self_sclass(cx: &Cx<'_>, node: NodeId) -> bool {
    for ancestor in cx.ancestors(node) {
        if let NodeKind::Sclass { expr, .. } = *cx.kind(ancestor) {
            return matches!(*cx.kind(expr), NodeKind::SelfExpr);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::DefaultScope;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_default_scope() {
        test::<DefaultScope>().expect_offense(indoc! {r#"
            default_scope -> { where(hidden: false) }
            ^^^^^^^^^^^^^ Avoid use of `default_scope`. It is better to use explicitly named scopes.
        "#});
    }

    #[test]
    fn flags_default_scope_with_args() {
        test::<DefaultScope>().expect_offense(indoc! {r#"
            default_scope(where(hidden: false))
            ^^^^^^^^^^^^^ Avoid use of `default_scope`. It is better to use explicitly named scopes.
        "#});
    }

    #[test]
    fn does_not_flag_receiver_default_scope() {
        test::<DefaultScope>().expect_no_offenses("foo.default_scope\n");
    }

    #[test]
    fn flags_defs_default_scope() {
        test::<DefaultScope>().expect_offense(indoc! {r#"
            def self.default_scope
                     ^^^^^^^^^^^^^ Avoid use of `default_scope`. It is better to use explicitly named scopes.
            end
        "#});
    }

    #[test]
    fn flags_sclass_default_scope() {
        test::<DefaultScope>().expect_offense(indoc! {r#"
            class << self
              def default_scope
                  ^^^^^^^^^^^^^ Avoid use of `default_scope`. It is better to use explicitly named scopes.
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_plain_def() {
        test::<DefaultScope>().expect_no_offenses("def default_scope\nend\n");
    }

    #[test]
    fn does_not_flag_named_scope() {
        test::<DefaultScope>().expect_no_offenses("scope :published, -> { where(hidden: false) }\n");
    }
}
murphy_plugin_api::submit_cop!(DefaultScope);
