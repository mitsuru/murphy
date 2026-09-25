//! `Rails/EnumSyntax` — flag enums written with keyword arguments.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnumSyntax
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `minimum_target_ruby_version 3.0` and
//!   `minimum_target_rails_version 7.0` gating (unset means newest/default
//!   3.1), old-syntax `(send nil? :enum (hash $...))` keyword-arg rewrite to
//!   positional (`enum :k, v[, opts]`) with `_`-prefix stripping, unless
//!   `multiple_enum_definitions?` (2+ non-option keys, offense without
//!   autocorrect), plus new-syntax `(send nil? :enum $_ ${array hash} $_)`
//!   `_`-option (`_prefix` etc.) key rewrite. Include path gating
//!   (`**/app/models/**/*.rb`, `**/lib/**/*.rb`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate,
//!   verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! - `enum status: { active: 0 }` → `enum :status, { active: 0 }`.
//! - `enum status: { active: 0 }, _prefix: true` → `enum :status, { active: 0 }, prefix: true`.
//! - `enum :status, { active: 0 }, _prefix: true` → `enum :status, { active: 0 }, prefix: true` (key only).
//!
//! `enum :status, { active: 0 }` does not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, RubyVersion, cop};

const MSG_TMPL: &str =
    "Enum defined with keyword arguments in `%ENUM%` enum declaration. Use positional arguments instead.";
const MSG_OPTIONS_TMPL: &str =
    "Enum defined with deprecated options in `%ENUM%` enum declaration. Remove the `_` prefix.";

const OPTION_NAMES: &[&str] = &["prefix", "suffix", "scopes", "default", "instance_methods"];
const UNDERSCORED: &[&str] = &["_prefix", "_suffix", "_scopes", "_default", "_instance_methods"];

fn msg(enum_name: &str) -> String {
    MSG_TMPL.replace("%ENUM%", enum_name)
}

fn msg_options(enum_name: &str) -> String {
    MSG_OPTIONS_TMPL.replace("%ENUM%", enum_name)
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnumSyntax;

#[cop(
    name = "Rails/EnumSyntax",
    description = "Use positional arguments over keyword arguments when defining enums.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "3.0",
    options = NoOptions,
)]
impl EnumSyntax {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[enum]`.
    #[on_node(kind = "send", methods = ["enum"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `minimum_target_rails_version 7.0` + `minimum_target_ruby_version 3.0`.
    // Unset Rails means newest (fires); unset Ruby resolves to default 3.1 (fires).
    if !cx.rails_version_at_least(7, 0) {
        return;
    }
    let ruby_ok = cx
        .target_ruby_version()
        .is_none_or(|v| v >= RubyVersion::new(3, 0));
    if !ruby_ok {
        return;
    }
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return;
    };
    if receiver.get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    check_keyword_args(node, args, cx);
    check_enum_options(node, args, cx);
}

/// Old syntax: `enum k: v, ...` — single hash arg.
fn check_keyword_args(node: NodeId, args: &[NodeId], cx: &Cx<'_>) {
    if args.len() != 1 {
        return;
    }
    let hash_id = args[0];
    let NodeKind::Hash(list) = *cx.kind(hash_id) else {
        return;
    };
    let pairs = cx.list(list).to_vec();
    if pairs.is_empty() {
        return;
    }
    let rest: Vec<NodeId> = if pairs.len() > 1 {
        pairs[1..].to_vec()
    } else {
        Vec::new()
    };
    let multiple = multiple_enum_definitions(cx, hash_id);
    for &pair_id in &pairs {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        if is_option_key(cx, pair_id) {
            continue;
        }
        let enum_val = enum_name_value(cx, key);
        cx.emit_offense(cx.range(value), &msg(&enum_val), None);
        if multiple {
            continue;
        }
        let preferred =
            format!("enum {}, {}{}", enum_name(cx, key), cx.raw_source(cx.range(value)), correct_options(cx, &rest));
        cx.emit_edit(cx.range(node), &preferred);
    }
}

/// New syntax with options: `enum :k, <array|hash>, <opts>` — exactly 3 args.
fn check_enum_options(node: NodeId, args: &[NodeId], cx: &Cx<'_>) {
    let _ = node;
    if args.len() != 3 {
        return;
    }
    let key = args[0];
    let values = args[1];
    if !(matches!(*cx.kind(values), NodeKind::Array(_))
        || matches!(*cx.kind(values), NodeKind::Hash(_)))
    {
        return;
    }
    let opts_id = args[2];
    let NodeKind::Hash(list) = *cx.kind(opts_id) else {
        return;
    };
    let enum_val = enum_name_value(cx, key);
    for &pair_id in cx.list(list) {
        let NodeKind::Pair { key: opt_key, .. } = *cx.kind(pair_id) else {
            continue;
        };
        if !is_option_key(cx, pair_id) {
            continue;
        }
        cx.emit_offense(cx.range(opt_key), &msg_options(&enum_val), None);
        let src = cx.raw_source(cx.range(opt_key)).to_owned();
        let fixed = src.strip_prefix('_').unwrap_or(&src).to_owned();
        cx.emit_edit(cx.range(opt_key), &fixed);
    }
}

fn is_option_key(cx: &Cx<'_>, pair_id: NodeId) -> bool {
    let NodeKind::Pair { key, .. } = *cx.kind(pair_id) else {
        return false;
    };
    let src = match *cx.kind(key) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    };
    UNDERSCORED.contains(&src.as_str())
}

fn multiple_enum_definitions(cx: &Cx<'_>, hash_id: NodeId) -> bool {
    let NodeKind::Hash(list) = *cx.kind(hash_id) else {
        return false;
    };
    let mut non_option = 0;
    for &pair_id in cx.list(list) {
        let NodeKind::Pair { key, .. } = *cx.kind(pair_id) else {
            continue;
        };
        let raw = match *cx.kind(key) {
            NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
            _ => cx.raw_source(cx.range(key)).to_owned(),
        };
        let stripped = raw.strip_prefix('_').unwrap_or(&raw);
        if OPTION_NAMES.contains(&stripped) {
            continue;
        }
        non_option += 1;
        if non_option >= 2 {
            return true;
        }
    }
    false
}

fn enum_name_value(cx: &Cx<'_>, key: NodeId) -> String {
    match *cx.kind(key) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    }
}

fn enum_name(cx: &Cx<'_>, key: NodeId) -> String {
    match *cx.kind(key) {
        NodeKind::Str(sid) => format!("{:?}", cx.string_str(sid)),
        NodeKind::Sym(s) => format!(":{}", cx.symbol_str(s)),
        _ => cx.raw_source(cx.range(key)).to_owned(),
    }
}

fn correct_options(cx: &Cx<'_>, options: &[NodeId]) -> String {
    if options.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    for &pair_id in options {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        let raw = match *cx.kind(key) {
            NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
            _ => cx.raw_source(cx.range(key)).to_owned(),
        };
        let name = raw.strip_prefix('_').unwrap_or(&raw).to_owned();
        let vsrc = cx.raw_source(cx.range(value)).to_owned();
        parts.push(format!("{name}: {vsrc}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(", {}", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::EnumSyntax;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_keyword() {
        test::<EnumSyntax>().expect_offense(indoc! {r#"
            enum status: { active: 0, archived: 1 }
                         ^^^^^^^^^^^^^^^^^^^^^^^^^^ Enum defined with keyword arguments in `status` enum declaration. Use positional arguments instead.
        "#});
    }

    #[test]
    fn corrects_keyword() {
        test::<EnumSyntax>()
            .expect_correction(
                indoc! {r#"
                    enum status: { active: 0, archived: 1 }
                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^ Enum defined with keyword arguments in `status` enum declaration. Use positional arguments instead.
                "#},
                "enum :status, { active: 0, archived: 1 }\n",
            )
            .expect_no_offenses("enum :status, { active: 0, archived: 1 }\n");
    }

    #[test]
    fn corrects_keyword_with_underscore_option() {
        test::<EnumSyntax>()
            .expect_correction(
                indoc! {r#"
                    enum status: { active: 0 }, _prefix: true
                                 ^^^^^^^^^^^^^ Enum defined with keyword arguments in `status` enum declaration. Use positional arguments instead.
                "#},
                "enum :status, { active: 0 }, prefix: true\n",
            );
    }

    #[test]
    fn flags_new_option_underscore() {
        test::<EnumSyntax>().expect_offense(indoc! {r#"
            enum :status, { active: 0 }, _prefix: true
                                         ^^^^^^^^ Enum defined with deprecated options in `status` enum declaration. Remove the `_` prefix.
        "#});
    }

    #[test]
    fn corrects_new_option_underscore() {
        test::<EnumSyntax>()
            .expect_correction(
                indoc! {r#"
                    enum :status, { active: 0 }, _prefix: true
                                                 ^^^^^^^^ Enum defined with deprecated options in `status` enum declaration. Remove the `_` prefix.
                "#},
                "enum :status, { active: 0 }, prefix: true\n",
            )
            .expect_no_offenses("enum :status, { active: 0 }, prefix: true\n");
    }

    #[test]
    fn does_not_flag_positional() {
        test::<EnumSyntax>()
            .expect_no_offenses("enum :status, { active: 0, archived: 1 }\n");
    }

    #[test]
    fn does_not_flag_positional_with_option() {
        test::<EnumSyntax>()
            .expect_no_offenses("enum :status, { active: 0 }, prefix: true\n");
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<EnumSyntax>()
            .expect_no_offenses("foo.enum status: { active: 0 }\n");
    }

    #[test]
    fn multiple_definitions_no_autocorrect() {
        // Two enums: both flag, but no correction (upstream skips when multiple).
        test::<EnumSyntax>().expect_offense(indoc! {r#"
            enum status: { active: 0 }, other: { a: 1 }
                         ^^^^^^^^^^^^^ Enum defined with keyword arguments in `status` enum declaration. Use positional arguments instead.
                                               ^^^^^^^^ Enum defined with keyword arguments in `other` enum declaration. Use positional arguments instead.
        "#});
    }

    #[test]
    fn gated_below_rails_7() {
        test::<EnumSyntax>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses("enum status: { active: 0 }\n");
    }

    #[test]
    fn gated_below_ruby_3() {
        test::<EnumSyntax>()
            .with_target_ruby_version(2, 7)
            .expect_no_offenses("enum status: { active: 0 }\n");
    }
}
murphy_plugin_api::submit_cop!(EnumSyntax);
