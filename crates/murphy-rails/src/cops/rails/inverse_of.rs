//! `Rails/InverseOf` — require `inverse_of` when the inverse cannot be inferred.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/InverseOf
//! upstream_version_checked: 2.35.0
//! version_added: "0.52"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [has_many has_one belongs_to] with scope (block-type arg) and
//!   conditions/foreign_key (plus `as` below Rails 5.2) requiring
//!   `inverse_of`; through/polymorphic (non-nil) suppress; dynamic
//!   `**options` suppress unless `inverse_of: nil` is present;
//!   `with_options` ancestor hashes (implicit and `|assoc|` explicit
//!   receivers) contribute options. Offense is the association selector;
//!   `inverse_of: nil` reports the nil message. Upstream Include path
//!   gating (app/models) absent in Murphy.
//! ```
//!
//! Looks for `has_(one|many)` and `belongs_to` associations where Active
//! Record can't automatically determine the inverse association.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct InverseOf;

#[derive(CopOptions)]
pub struct InverseOfOptions {
    #[option(
        name = "IgnoreScopes",
        default = false,
        description = "Whether to ignore scopes (lambdas) when determining inverse_of requirement."
    )]
    pub ignore_scopes: bool,
}

#[cop(
    name = "Rails/InverseOf",
    description = "Checks for associations where the inverse cannot be determined automatically.",
    default_severity = "warning",
    default_enabled = true,
    options = InverseOfOptions,
)]
impl InverseOf {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["has_many", "has_one", "belongs_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return;
    };
    let arg_ids = cx.list(args).to_vec();
    // Upstream `(send $_ :assoc _ $...)` — needs at least the name arg.
    if arg_ids.is_empty() {
        return;
    }
    let option_args = &arg_ids[1..];

    let opts_cfg = cx.options_or_default::<InverseOfOptions>();
    let ignore_scopes = opts_cfg.ignore_scopes;

    // Collect option pairs from direct hash args + matching with_options.
    let mut pairs: Vec<NodeId> = Vec::new();
    let mut has_kwsplat = false;
    for &a in option_args {
        collect_hash_pairs(cx, a, &mut pairs, &mut has_kwsplat);
    }
    let recv_opt = receiver.get();
    for (wpairs, wkwsplat) in with_options_pairs(cx, node, recv_opt) {
        pairs.extend(wpairs);
        has_kwsplat = has_kwsplat || wkwsplat;
    }

    if options_ignoring(cx, &pairs) {
        return;
    }

    let has_scope = !ignore_scopes && option_args.iter().any(|&a| is_block_type(cx, a));
    let needs_for_options = options_requiring(cx, &pairs);

    if !(has_scope || needs_for_options) {
        return;
    }

    if options_contain_inverse_of(cx, &pairs) {
        return;
    }

    if has_kwsplat && !pairs.iter().any(|&p| is_inverse_of_nil(cx, p)) {
        return;
    }

    let msg = if pairs.iter().any(|&p| is_inverse_of_nil(cx, p)) {
        "You specified `inverse_of: nil`, you probably meant to use `inverse_of: false`."
    } else {
        "Specify an `:inverse_of` option."
    };
    cx.emit_offense(cx.loc(node).name, msg, None);
}

fn is_block_type(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

fn collect_hash_pairs(cx: &Cx<'_>, arg: NodeId, out: &mut Vec<NodeId>, kwsplat: &mut bool) {
    let NodeKind::Hash(list) = *cx.kind(arg) else {
        return;
    };
    for &child in cx.list(list) {
        match *cx.kind(child) {
            NodeKind::Pair { .. } => out.push(child),
            NodeKind::Kwsplat(_) => {
                *kwsplat = true;
            }
            _ => {}
        }
    }
}

fn pair_key_is(cx: &Cx<'_>, pair: NodeId, name: &str) -> bool {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return false;
    };
    if let NodeKind::Sym(sym) = *cx.kind(key) {
        cx.symbol_str(sym) == name
    } else {
        false
    }
}

fn pair_value(cx: &Cx<'_>, pair: NodeId) -> Option<NodeId> {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return None;
    };
    Some(value)
}

fn value_is_nil(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(*cx.kind(id), NodeKind::Nil)
}

fn options_ignoring(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    pairs.iter().any(|&p| {
        (pair_key_is(cx, p, "through") || pair_key_is(cx, p, "polymorphic"))
            && pair_value(cx, p).is_some_and(|v| !value_is_nil(cx, v))
    })
}

fn options_requiring(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    let mut required = pairs.iter().any(|&p| {
        (pair_key_is(cx, p, "conditions") || pair_key_is(cx, p, "foreign_key"))
            && pair_value(cx, p).is_some_and(|v| !value_is_nil(cx, v))
    });
    if required {
        return true;
    }
    // `as` only requires below Rails 5.2.
    if !cx.rails_version_at_least(5, 2) {
        required = pairs.iter().any(|&p| {
            pair_key_is(cx, p, "as") && pair_value(cx, p).is_some_and(|v| !value_is_nil(cx, v))
        });
    }
    required
}

fn options_contain_inverse_of(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    pairs.iter().any(|&p| {
        pair_key_is(cx, p, "inverse_of") && pair_value(cx, p).is_some_and(|v| !value_is_nil(cx, v))
    })
}

fn is_inverse_of_nil(cx: &Cx<'_>, pair: NodeId) -> bool {
    pair_key_is(cx, pair, "inverse_of") && pair_value(cx, pair).is_some_and(|v| value_is_nil(cx, v))
}

fn with_options_pairs(
    cx: &Cx<'_>,
    node: NodeId,
    recv: Option<NodeId>,
) -> Vec<(Vec<NodeId>, bool)> {
    let mut out = Vec::new();
    for anc in cx.ancestors(node) {
        let (call, block_args) = match *cx.kind(anc) {
            NodeKind::Block { call, args, .. } => (call, Some(args)),
            NodeKind::Numblock { send, .. } => (send, None),
            NodeKind::Itblock { send, .. } => (send, None),
            _ => continue,
        };
        if cx.method_name(call) != Some("with_options") {
            continue;
        }
        if !same_context(cx, block_args, recv) {
            continue;
        }
        let mut wpairs = Vec::new();
        let mut wkwsplat = false;
        for &a in cx.call_arguments(call) {
            collect_hash_pairs(cx, a, &mut wpairs, &mut wkwsplat);
        }
        out.push((wpairs, wkwsplat));
    }
    out
}

fn same_context(cx: &Cx<'_>, block_args: Option<NodeId>, recv: Option<NodeId>) -> bool {
    let first_arg_sym: Option<String> = block_args.and_then(|args_id| {
        let NodeKind::Args(list) = *cx.kind(args_id) else {
            return None;
        };
        let first = *cx.list(list).first()?;
        if let NodeKind::Arg(sym) = *cx.kind(first) {
            Some(cx.symbol_str(sym).to_owned())
        } else {
            None
        }
    });
    match (first_arg_sym, recv) {
        (None, None) => true,
        (Some(a), Some(r)) => {
            // Explicit receiver must be a bare lvar with the same name.
            if let NodeKind::Lvar(sym) = *cx.kind(r) {
                cx.symbol_str(sym) == a
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{InverseOf, InverseOfOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_scope_without_inverse() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              has_one :foo, -> () { where(bar: true) }
              ^^^^^^^ Specify an `:inverse_of` option.
            end
        "#});
    }

    #[test]
    fn allows_scope_with_false() {
        test::<InverseOf>().expect_no_offenses(
            "has_many :foo, -> () { where(bar: true) }, inverse_of: false\n",
        );
    }

    #[test]
    fn flags_inverse_nil() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              has_many :foo, -> () { where(bar: true) }, inverse_of: nil
              ^^^^^^^^ You specified `inverse_of: nil`, you probably meant to use `inverse_of: false`.
            end
        "#});
    }

    #[test]
    fn flags_foreign_key_without_inverse() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              belongs_to :foo, foreign_key: 'foo_id'
              ^^^^^^^^^^ Specify an `:inverse_of` option.
            end
        "#});
    }

    #[test]
    fn allows_foreign_key_with_inverse() {
        test::<InverseOf>()
            .expect_no_offenses("has_one :foo, foreign_key: 'foo_id', inverse_of: :bar\n");
    }

    #[test]
    fn flags_conditions_option() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              has_many :foo, conditions: -> { where(bar: true) }
              ^^^^^^^^ Specify an `:inverse_of` option.
            end
        "#});
    }

    #[test]
    fn allows_no_options() {
        test::<InverseOf>().expect_no_offenses("has_one :foo\n");
    }

    #[test]
    fn allows_other_options() {
        test::<InverseOf>().expect_no_offenses("has_one :foo, dependent: :nullify\n");
    }

    #[test]
    fn allows_through_with_scope() {
        test::<InverseOf>().expect_no_offenses(
            "has_many :patients, -> () { where(bar: true) }, through: :appointments\n",
        );
    }

    #[test]
    fn allows_polymorphic_with_scope() {
        test::<InverseOf>().expect_no_offenses(
            "belongs_to :imageable, -> () { where(bar: true) }, polymorphic: true\n",
        );
    }

    #[test]
    fn allows_dynamic_options() {
        test::<InverseOf>().expect_no_offenses(
            "has_many :foo, conditions: -> { where(bar: true) }, **options\n",
        );
    }

    #[test]
    fn flags_dynamic_with_inverse_nil() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              def define_association(**options)
                has_many :foo, -> () { where(bar: true) }, inverse_of: nil, **options
                ^^^^^^^^ You specified `inverse_of: nil`, you probably meant to use `inverse_of: false`.
              end
            end
        "#});
    }

    #[test]
    fn allows_with_options_inverse() {
        test::<InverseOf>().expect_no_offenses(indoc! {r#"
            class Person
              with_options inverse_of: false do
                has_one :foo, -> () { where(bar: true) }
              end
            end
        "#});
    }

    #[test]
    fn allows_with_options_explicit_receiver() {
        test::<InverseOf>().expect_no_offenses(indoc! {r#"
            class Person
              with_options inverse_of: :bar do |assoc|
                assoc.belongs_to :foo, foreign_key: 'foo_id'
              end
            end
        "#});
    }

    #[test]
    fn flags_with_options_wrong_receiver() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              with_options inverse_of: :bar do |_assoc|
                belongs_to :foo, -> () { where(baz: true) }
                ^^^^^^^^^^ Specify an `:inverse_of` option.
              end
            end
        "#});
    }

    #[test]
    fn flags_with_options_invalid() {
        test::<InverseOf>().expect_offense(indoc! {r#"
            class Person
              with_options foreign_key: 'foo_id' do
                has_one :foo
                ^^^^^^^ Specify an `:inverse_of` option.
              end
            end
        "#});
    }

    #[test]
    fn ignores_scopes_when_configured() {
        let opts = InverseOfOptions {
            ignore_scopes: true,
        };
        test::<InverseOf>().with_options(&opts).expect_no_offenses(
            "has_many :foo, -> () { where(bar: true) }\n",
        );
    }

    #[test]
    fn allows_as_on_new_rails() {
        test::<InverseOf>()
            .expect_no_offenses("has_many :pictures, as: :imageable\n");
    }

    #[test]
    fn flags_as_on_old_rails() {
        test::<InverseOf>()
            .with_target_rails_version(5, 1)
            .expect_offense(indoc! {r#"
                class Person
                  has_many :pictures, as: :imageable
                  ^^^^^^^^ Specify an `:inverse_of` option.
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(InverseOf);
