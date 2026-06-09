//! `Lint/HashNewWithKeywordArgumentsAsDefault` — checks deprecated keyword defaults in `Hash.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/HashNewWithKeywordArgumentsAsDefault
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's `Hash.new` / `::Hash.new` coverage for braceless hash
//!   defaults, hash rockets, method-call keys, the `capacity:`-only exemption,
//!   and autocorrection by wrapping the braceless hash in `{}`.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG: &str = "Use a hash literal instead of keyword arguments.";

#[derive(Default)]
pub struct HashNewWithKeywordArgumentsAsDefault;

#[cop(
    name = "Lint/HashNewWithKeywordArgumentsAsDefault",
    description = "Checks deprecated keyword arguments as Hash.new defaults.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HashNewWithKeywordArgumentsAsDefault {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(hash_arg) = deprecated_hash_new_default(node, cx) else {
            return;
        };
        cx.emit_offense(cx.range(hash_arg), MSG, None);
        cx.emit_edit(cx.range(hash_arg), &format!("{{{}}}", cx.raw_source(cx.range(hash_arg))));
    }
}

fn deprecated_hash_new_default(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return None;
    };
    if !receiver.get().is_some_and(|receiver| cx.is_global_const(receiver, "Hash")) {
        return None;
    }
    let args = cx.list(args);
    let [first] = args else {
        return None;
    };
    let NodeKind::Hash(pairs) = *cx.kind(*first) else {
        return None;
    };
    if is_braced_hash(*first, cx) || is_capacity_only_hash(pairs, cx) {
        return None;
    }
    Some(*first)
}

fn is_braced_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.raw_source(cx.range(node)).trim_start().starts_with('{')
}

fn is_capacity_only_hash(pairs: murphy_plugin_api::NodeList, cx: &Cx<'_>) -> bool {
    let pairs = cx.list(pairs);
    let [pair] = pairs else {
        return false;
    };
    let NodeKind::Pair { key, .. } = *cx.kind(*pair) else {
        return false;
    };
    matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "capacity")
}

murphy_plugin_api::submit_cop!(HashNewWithKeywordArgumentsAsDefault);

#[cfg(test)]
mod tests {
    use super::HashNewWithKeywordArgumentsAsDefault;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_hash_new_with_keyword_arguments() {
        test::<HashNewWithKeywordArgumentsAsDefault>().expect_correction(
            indoc! {r#"
                Hash.new(key: :value)
                         ^^^^^^^^^^^ Use a hash literal instead of keyword arguments.
            "#},
            "Hash.new({key: :value})\n",
        );
    }

    #[test]
    fn flags_hash_rocket_and_cbase_forms() {
        test::<HashNewWithKeywordArgumentsAsDefault>()
            .expect_correction(
                indoc! {r#"
                    ::Hash.new(key => 'value')
                               ^^^^^^^^^^^^^^ Use a hash literal instead of keyword arguments.
                "#},
                "::Hash.new({key => 'value'})\n",
            )
            .expect_correction(
                indoc! {r#"
                    Hash.new(capacity: 42, key: :value)
                             ^^^^^^^^^^^^^^^^^^^^^^^^^ Use a hash literal instead of keyword arguments.
                "#},
                "Hash.new({capacity: 42, key: :value})\n",
            );
    }

    #[test]
    fn accepts_non_deprecated_hash_new_forms() {
        test::<HashNewWithKeywordArgumentsAsDefault>()
            .expect_no_offenses("Hash.new({key: :value})\n")
            .expect_no_offenses("Hash.new(capacity: 42)\n")
            .expect_no_offenses("Hash.new\n")
            .expect_no_offenses("Foo.new(key: :value)\n")
            .expect_no_offenses("Hash.new(42)\n");
    }
}
