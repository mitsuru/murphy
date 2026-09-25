//! `Rails/EnumUniqueness` — flag duplicate values in enum declarations.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnumUniqueness
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:enum] gating,
//!   new-syntax `(send nil? :enum $_ ${array hash} ...)` plus old-syntax
//!   `(send nil? :enum (hash $...))` with `(pair $_ ${array hash})`.
//!   `consecutive_duplicates` (Duplication mixin) grouped by value source;
//!   offense on the duplicate value node with
//!   `Duplicate value \`v\` found in \`e\` enum declaration.` No autocorrect
//!   upstream. Include path gating (`**/app/models/**/*.rb`) is enforced
//!   via the murphy-rails pack default.yml (engine `cop_applies_to_file`
//!   gate, verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! - `enum :status, { active: 0, archived: 0 }` → second `0` flags.
//! - `enum :status, [:active, :archived, :active]` → second `:active` flags.
//! - `enum status: { active: 0, archived: 0 }` → second `0` flags.
//!
//! `enum :status, { active: 0, archived: 1 }` does not flag.

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

fn message(enum_name: &str, value_src: &str) -> String {
    format!("Duplicate value `{value_src}` found in `{enum_name}` enum declaration.")
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnumUniqueness;

#[cop(
    name = "Rails/EnumUniqueness",
    description = "Avoid duplicate integers in hash-syntax `enum` declaration.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EnumUniqueness {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[enum]`.
    #[on_node(kind = "send", methods = ["enum"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return;
    };
    if receiver.get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }
    // New syntax: `enum <key>, <array|hash>, ...`
    if args.len() >= 2
        && (matches!(*cx.kind(args[1]), NodeKind::Array(_))
            || matches!(*cx.kind(args[1]), NodeKind::Hash(_)))
    {
        let key = args[0];
        // Guard: first arg must not itself be a hash (that would be old syntax
        // with a single hash arg, not new syntax). Old syntax has exactly 1 arg.
        // Here args.len() >= 2, so safe.
        let name = enum_name(cx, key);
        for dup in consecutive_duplicates(cx, args[1]) {
            let vsrc = cx.raw_source(cx.range(dup)).to_owned();
            cx.emit_offense(cx.range(dup), &message(&name, &vsrc), None);
        }
    }
    // Old syntax: `enum status: [...], ...` — single hash arg.
    if args.len() != 1 {
        return;
    }
    let hash_id = args[0];
    if !matches!(*cx.kind(hash_id), NodeKind::Hash(_)) {
        return;
    }
    let NodeKind::Hash(list) = *cx.kind(hash_id) else {
        return;
    };
    for &pair_id in cx.list(list) {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        if !(matches!(*cx.kind(value), NodeKind::Array(_))
            || matches!(*cx.kind(value), NodeKind::Hash(_)))
        {
            continue;
        }
        let name = enum_name(cx, key);
        for dup in consecutive_duplicates(cx, value) {
            let vsrc = cx.raw_source(cx.range(dup)).to_owned();
            cx.emit_offense(cx.range(dup), &message(&name, &vsrc), None);
        }
    }
}

fn enum_name(cx: &Cx<'_>, key: NodeId) -> String {
    match *cx.kind(key) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    }
}

/// Upstream `Duplication#consecutive_duplicates`: group values by equality,
/// return all but the first of each duplicate group. Grouping key is the
/// raw source (int `0` vs float `0.0`, sym `:a` vs str `"a"` differ).
fn consecutive_duplicates(cx: &Cx<'_>, container: NodeId) -> Vec<NodeId> {
    let values: Vec<NodeId> = match *cx.kind(container) {
        NodeKind::Array(list) => cx.list(list).to_vec(),
        NodeKind::Hash(list) => cx
            .list(list)
            .iter()
            .filter_map(|&pair_id| {
                let NodeKind::Pair { value, .. } = *cx.kind(pair_id) else {
                    return None;
                };
                Some(value)
            })
            .collect(),
        _ => return Vec::new(),
    };
    let mut by_src: HashMap<String, Vec<NodeId>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for &v in &values {
        let src = cx.raw_source(cx.range(v)).to_owned();
        if !by_src.contains_key(&src) {
            order.push(src.clone());
        }
        by_src.entry(src).or_default().push(v);
    }
    let mut out = Vec::new();
    for k in order {
        let group = &by_src[&k];
        if group.len() > 1 {
            out.extend_from_slice(&group[1..]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::EnumUniqueness;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_hash_duplicate() {
        test::<EnumUniqueness>().expect_offense(indoc! {r#"
            enum :status, { active: 0, archived: 0 }
                                                 ^ Duplicate value `0` found in `status` enum declaration.
        "#});
    }

    #[test]
    fn flags_array_duplicate() {
        test::<EnumUniqueness>().expect_offense(indoc! {r#"
            enum :status, [:active, :archived, :active]
                                               ^^^^^^^ Duplicate value `:active` found in `status` enum declaration.
        "#});
    }

    #[test]
    fn flags_old_hash_duplicate() {
        test::<EnumUniqueness>().expect_offense(indoc! {r#"
            enum status: { active: 0, archived: 0 }
                                                ^ Duplicate value `0` found in `status` enum declaration.
        "#});
    }

    #[test]
    fn flags_old_array_duplicate() {
        test::<EnumUniqueness>().expect_offense(indoc! {r#"
            enum status: [:active, :archived, :active]
                                              ^^^^^^^ Duplicate value `:active` found in `status` enum declaration.
        "#});
    }

    #[test]
    fn does_not_flag_unique_hash() {
        test::<EnumUniqueness>()
            .expect_no_offenses("enum :status, { active: 0, archived: 1 }\n");
    }

    #[test]
    fn does_not_flag_unique_array() {
        test::<EnumUniqueness>()
            .expect_no_offenses("enum :status, [:active, :archived]\n");
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<EnumUniqueness>()
            .expect_no_offenses("foo.enum :status, { active: 0, archived: 0 }\n");
    }

    #[test]
    fn does_not_flag_different_types() {
        test::<EnumUniqueness>()
            .expect_no_offenses("enum :status, { active: 0, archived: \"0\" }\n");
    }
}
murphy_plugin_api::submit_cop!(EnumUniqueness);
