//! `Style/Alias` — enforces consistent use of `alias` or `alias_method`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Alias
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues:
//!   - murphy-ctk3
//! notes: >
//!   The Murphy translator does not yet emit Alias AST nodes. Both alias bar foo
//!   and alias :bar :foo produce (unknown). Only the alias_method Send form is
//!   dispatched. prefer_alias mode (default) flags alias_method :bar, :foo in
//!   lexical scope and autocorrects to alias bar foo. prefer_alias_method mode
//!   cannot flag alias bar foo. Symbol-args form not flagged. Gap tracked in
//!   murphy-ctk3.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "prefer_alias")]
    PreferAlias,
    #[option(value = "prefer_alias_method")]
    PreferAliasMethod,
}

#[derive(CopOptions)]
pub struct AliasOptions {
    #[option(
        name = "EnforcedStyle",
        default = "prefer_alias",
        description = "Preferred alias form."
    )]
    pub enforced_style: EnforcedStyle,
}

fn lexical_scope_type(node: NodeId, cx: &Cx<'_>) -> &'static str {
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Class { .. } | NodeKind::Sclass { .. } => return "in a class body",
            NodeKind::Module { .. } => return "in a module body",
            _ => {}
        }
    }
    "at the top level"
}

#[derive(PartialEq, Eq)]
enum ScopeType {
    Lexical,
    Dynamic,
    InstanceEval,
}

fn scope_type(node: NodeId, cx: &Cx<'_>) -> ScopeType {
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. } => {
                return ScopeType::Lexical;
            }
            NodeKind::Def { .. } | NodeKind::Defs { .. } => {
                return ScopeType::Dynamic;
            }
            NodeKind::Block { call, .. }
            | NodeKind::Numblock { send: call, .. }
            | NodeKind::Itblock { send: call, .. } => {
                let call = *call;
                if cx.method_name(call) == Some("instance_eval") {
                    return ScopeType::InstanceEval;
                }
                return ScopeType::Dynamic;
            }
            _ => {}
        }
    }
    ScopeType::Lexical
}

fn alias_keyword_possible(node: NodeId, args: &[NodeId], cx: &Cx<'_>) -> bool {
    if scope_type(node, cx) == ScopeType::Dynamic {
        return false;
    }
    if args.len() != 2 {
        return false;
    }
    args.iter()
        .all(|&arg| matches!(cx.kind(arg), NodeKind::Sym(_)))
}

fn sym_bareword<'cx>(sym_node: NodeId, cx: &Cx<'cx>) -> Option<&'cx str> {
    match cx.kind(sym_node) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(*sym)),
        _ => None,
    }
}

#[derive(Default)]
pub struct Alias;

#[cop(
    name = "Style/Alias",
    description = "Use `alias` instead of `alias_method` (or vice versa, based on EnforcedStyle).",
    default_severity = "warning",
    default_enabled = true,
    options = AliasOptions
)]
impl Alias {
    #[on_node(kind = "send", methods = ["alias_method"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };

        if receiver.get().is_some() {
            return;
        }

        let opts = cx.options_or_default::<AliasOptions>();

        if opts.enforced_style != EnforcedStyle::PreferAlias {
            return;
        }

        let args_slice = cx.list(args);
        if !alias_keyword_possible(node, args_slice, cx) {
            return;
        }

        let scope_suffix = lexical_scope_type(node, cx);
        let msg = format!("Use `alias` instead of `alias_method` {scope_suffix}.");

        let selector = cx.selector(node);
        cx.emit_offense(selector, &msg, None);

        if let (Some(new_name), Some(old_name)) = (
            sym_bareword(args_slice[0], cx),
            sym_bareword(args_slice[1], cx),
        ) {
            let replacement = format!("alias {new_name} {old_name}");
            let node_range = cx.range(node);
            cx.emit_edit(node_range, &replacement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Alias, AliasOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_alias_method_at_top_level() {
        test::<Alias>().expect_correction(
            indoc! {r#"
                alias_method :bar, :foo
                ^^^^^^^^^^^^ Use `alias` instead of `alias_method` at the top level.
            "#},
            "alias bar foo\n",
        );
    }

    #[test]
    fn flags_alias_method_in_class_body() {
        test::<Alias>().expect_correction(
            indoc! {r#"
                class Foo
                  alias_method :bar, :foo
                  ^^^^^^^^^^^^ Use `alias` instead of `alias_method` in a class body.
                end
            "#},
            indoc! {r#"
                class Foo
                  alias bar foo
                end
            "#},
        );
    }

    #[test]
    fn flags_alias_method_in_module_body() {
        test::<Alias>().expect_correction(
            indoc! {r#"
                module Foo
                  alias_method :bar, :foo
                  ^^^^^^^^^^^^ Use `alias` instead of `alias_method` in a module body.
                end
            "#},
            indoc! {r#"
                module Foo
                  alias bar foo
                end
            "#},
        );
    }

    #[test]
    fn allows_alias_method_inside_def() {
        test::<Alias>().expect_no_offenses(indoc! {r#"
            def foo
              alias_method :bar, :baz
            end
        "#});
    }

    #[test]
    fn allows_alias_method_inside_defs() {
        test::<Alias>().expect_no_offenses(indoc! {r#"
            def self.foo
              alias_method :bar, :baz
            end
        "#});
    }

    #[test]
    fn allows_alias_method_inside_block() {
        test::<Alias>().expect_no_offenses(indoc! {r#"
            [].each do
              alias_method :bar, :baz
            end
        "#});
    }

    #[test]
    fn flags_alias_method_inside_instance_eval() {
        test::<Alias>().expect_correction(
            indoc! {r#"
                instance_eval do
                  alias_method :bar, :foo
                  ^^^^^^^^^^^^ Use `alias` instead of `alias_method` at the top level.
                end
            "#},
            indoc! {r#"
                instance_eval do
                  alias bar foo
                end
            "#},
        );
    }

    #[test]
    fn allows_alias_method_with_non_sym_args() {
        test::<Alias>().expect_no_offenses(indoc! {r#"
            alias_method new_name, :foo
        "#});
    }

    #[test]
    fn allows_alias_method_with_wrong_arg_count() {
        test::<Alias>().expect_no_offenses("alias_method :bar\n");
    }

    #[test]
    fn allows_alias_method_with_receiver() {
        test::<Alias>().expect_no_offenses("obj.alias_method(:bar, :foo)\n");
    }

    #[test]
    fn prefer_alias_method_mode_no_offense_on_alias_method() {
        test::<Alias>()
            .with_options(&AliasOptions {
                enforced_style: EnforcedStyle::PreferAliasMethod,
            })
            .expect_no_offenses("alias_method :bar, :foo\n");
    }

    #[test]
    fn prefer_alias_method_mode_alias_keyword_not_flagged_known_gap() {
        // alias bar foo cannot be flagged: Alias AST node not yet translated.
        test::<Alias>()
            .with_options(&AliasOptions {
                enforced_style: EnforcedStyle::PreferAliasMethod,
            })
            .expect_no_offenses("alias bar foo\n");
    }
}

murphy_plugin_api::submit_cop!(Alias);
