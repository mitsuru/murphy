//! `Rails/EnumHash` — flag enums written with array syntax.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnumHash
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:enum] gating,
//!   new-syntax `(send nil? :enum $_ ${array} ...)` gated on
//!   `TargetRailsVersion >= 7.0` (unset means newest) plus unconditional
//!   old-syntax `(send nil? :enum (hash $...))` with `(pair $_ $array)`.
//!   Offense on the array node; autocorrect to `{elem => idx}` hash via
//!   `str.dump` / `sym.inspect` / raw source. Include path gating
//!   (`**/app/models/**/*.rb`) is enforced via the murphy-rails pack
//!   default.yml (engine `cop_applies_to_file` gate, verified vs
//!   rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! - `enum :status, [:active, :archived]` → `enum :status, {:active => 0, :archived => 1}`.
//! - `enum status: [:active, :archived]` → `enum status: {:active => 0, :archived => 1}`.
//!
//! `enum :status, { active: 0, archived: 1 }` does not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG_TMPL: &str =
    "Enum defined as an array found in `%ENUM%` enum declaration. Use hash syntax instead.";

fn message(enum_name: &str) -> String {
    MSG_TMPL.replace("%ENUM%", enum_name)
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnumHash;

#[cop(
    name = "Rails/EnumHash",
    description = "Prefer hash syntax over array syntax when defining enums.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EnumHash {
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
    // New syntax: `enum <key>, <array>, ...` — Rails 7.0+.
    if args.len() >= 2 && cx.rails_version_at_least(7, 0) {
        let key = args[0];
        let arr = args[1];
        if matches!(*cx.kind(arr), NodeKind::Array(_)) {
            let name = enum_name(cx, key);
            cx.emit_offense(cx.range(arr), &message(&name), None);
            cx.emit_edit(cx.range(arr), &build_hash(cx, arr));
            // Fall through to old-syntax check? Old syntax requires single
            // hash arg, so no overlap when args.len() >= 2 with array second.
            // Return to avoid double-reporting when first arg is a hash?
            // (e.g. `enum status: [...], other` is not valid upstream shape.)
            // Still check old syntax only when single hash arg below.
            if !(args.len() == 1 && matches!(*cx.kind(args[0]), NodeKind::Hash(_))) {
                // Old-syntax block below will no-op; return early.
                return;
            }
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
    for &pair_id in cx.list(match *cx.kind(hash_id) {
        NodeKind::Hash(l) => l,
        _ => unreachable!(),
    }) {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        if !matches!(*cx.kind(value), NodeKind::Array(_)) {
            continue;
        }
        let name = enum_name(cx, key);
        cx.emit_offense(cx.range(value), &message(&name), None);
        cx.emit_edit(cx.range(value), &build_hash(cx, value));
    }
}

fn enum_name(cx: &Cx<'_>, key: NodeId) -> String {
    match *cx.kind(key) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    }
}

fn elem_source(cx: &Cx<'_>, elem: NodeId) -> String {
    match *cx.kind(elem) {
        NodeKind::Str(sid) => format!("{:?}", cx.string_str(sid)),
        NodeKind::Sym(s) => format!(":{}", cx.symbol_str(s)),
        _ => cx.raw_source(cx.range(elem)).to_owned(),
    }
}

fn build_hash(cx: &Cx<'_>, array_id: NodeId) -> String {
    let NodeKind::Array(list) = *cx.kind(array_id) else {
        return String::new();
    };
    let elems = cx.list(list);
    let parts: Vec<String> = elems
        .iter()
        .enumerate()
        .map(|(i, &e)| format!("{} => {i}", elem_source(cx, e)))
        .collect();
    format!("{{{}}}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::EnumHash;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_new_syntax_array() {
        test::<EnumHash>().expect_offense(indoc! {r#"
            enum :status, [:active, :archived]
                          ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
        "#});
    }

    #[test]
    fn flags_old_syntax_array() {
        test::<EnumHash>().expect_offense(indoc! {r#"
            enum status: [:active, :archived]
                         ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
        "#});
    }

    #[test]
    fn does_not_flag_hash() {
        test::<EnumHash>()
            .expect_no_offenses("enum :status, { active: 0, archived: 1 }\n");
    }

    #[test]
    fn does_not_flag_old_hash() {
        test::<EnumHash>()
            .expect_no_offenses("enum status: { active: 0, archived: 1 }\n");
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<EnumHash>()
            .expect_no_offenses("foo.enum :status, [:active]\n");
    }

    #[test]
    fn new_syntax_gated_below_rails_7() {
        test::<EnumHash>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses("enum :status, [:active, :archived]\n");
    }

    #[test]
    fn fires_at_rails_7() {
        test::<EnumHash>()
            .with_target_rails_version(7, 0)
            .expect_offense(indoc! {r#"
                enum :status, [:active, :archived]
                              ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
            "#});
    }

    #[test]
    fn old_syntax_fires_below_rails_7() {
        // Old syntax is not version-gated upstream.
        test::<EnumHash>()
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                enum status: [:active, :archived]
                             ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
            "#});
    }

    #[test]
    fn corrects_new_syntax() {
        test::<EnumHash>()
            .expect_correction(
                indoc! {r#"
                    enum :status, [:active, :archived]
                                  ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
                "#},
                "enum :status, {:active => 0, :archived => 1}\n",
            )
            .expect_no_offenses("enum :status, {:active => 0, :archived => 1}\n");
    }

    #[test]
    fn corrects_old_syntax() {
        test::<EnumHash>()
            .expect_correction(
                indoc! {r#"
                    enum status: [:active, :archived]
                                 ^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
                "#},
                "enum status: {:active => 0, :archived => 1}\n",
            )
            .expect_no_offenses("enum status: {:active => 0, :archived => 1}\n");
    }

    #[test]
    fn corrects_string_elements() {
        test::<EnumHash>()
            .expect_correction(
                indoc! {r#"
                    enum :status, ["active", "archived"]
                                  ^^^^^^^^^^^^^^^^^^^^^^ Enum defined as an array found in `status` enum declaration. Use hash syntax instead.
                "#},
                "enum :status, {\"active\" => 0, \"archived\" => 1}\n",
            );
    }
}
murphy_plugin_api::submit_cop!(EnumHash);
