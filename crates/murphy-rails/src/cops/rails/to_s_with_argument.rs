//! `Rails/ToSWithArgument` — use `to_formatted_s` instead of `to_s(arg)`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ToSWithArgument
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_csend`
//!   (`RESTRICT_ON_SEND = %i[to_s]`) behind
//!   `minimum_target_rails_version 7.0` (unset means newest):
//!   flags `to_s` whose first argument is a Sym in the upstream
//!   `EXTENDED_FORMAT_TYPES` set and rewrites the selector to
//!   `to_formatted_s`. Other first-arg shapes (str, no args) never
//!   flag. Disabled upstream by default (`Enabled: pending`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ToSWithArgument;

const EXTENDED_FORMAT_TYPES: &[&str] = &[
    "currency",
    "db",
    "delimited",
    "human",
    "human_size",
    "inspect",
    "iso8601",
    "long",
    "long_ordinal",
    "nsec",
    "number",
    "percentage",
    "phone",
    "rfc822",
    "rounded",
    "short",
    "time",
    "usec",
];

#[cop(
    name = "Rails/ToSWithArgument",
    description = "Identifies passing any argument to `#to_s`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ToSWithArgument {
    #[on_node(kind = "send", methods = ["to_s"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(node) != Some("to_s") {
        return;
    }
    // Upstream `minimum_target_rails_version 7.0` — unset means newest.
    if !cx.rails_version_at_least(7, 0) {
        return;
    }
    if !rails_extended_to_s(cx, node) {
        return;
    }
    cx.emit_offense(cx.loc(node).name, "Use `to_formatted_s` instead.", None);
    cx.emit_edit(cx.loc(node).name, "to_formatted_s");
}

/// Upstream `rails_extended_to_s?`: first arg is a Sym in the set.
fn rails_extended_to_s(cx: &Cx<'_>, node: NodeId) -> bool {
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return false;
    };
    let NodeKind::Sym(sym) = *cx.kind(first) else {
        return false;
    };
    EXTENDED_FORMAT_TYPES.contains(&cx.symbol_str(sym))
}

#[cfg(test)]
mod tests {
    use super::ToSWithArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_s_with_delimited() {
        test::<ToSWithArgument>().expect_offense(indoc! {r#"
            obj.to_s(:delimited)
                ^^^^ Use `to_formatted_s` instead.
        "#});
    }

    #[test]
    fn flags_to_s_with_db() {
        test::<ToSWithArgument>().expect_offense(indoc! {r#"
            obj.to_s(:db)
                ^^^^ Use `to_formatted_s` instead.
        "#});
    }

    #[test]
    fn does_not_flag_bare_to_s() {
        test::<ToSWithArgument>().expect_no_offenses("obj.to_s\n");
    }

    #[test]
    fn does_not_flag_unknown_format() {
        test::<ToSWithArgument>().expect_no_offenses("obj.to_s(:unknown)\n");
    }

    #[test]
    fn does_not_flag_string_argument() {
        test::<ToSWithArgument>().expect_no_offenses("obj.to_s('db')\n");
    }

    #[test]
    fn does_not_flag_below_rails_70() {
        test::<ToSWithArgument>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses("obj.to_s(:delimited)\n");
    }

    #[test]
    fn fires_at_rails_70() {
        test::<ToSWithArgument>()
            .with_target_rails_version(7, 0)
            .expect_offense(indoc! {r#"
                obj.to_s(:delimited)
                    ^^^^ Use `to_formatted_s` instead.
            "#});
    }

    #[test]
    fn corrects_to_to_formatted_s() {
        test::<ToSWithArgument>()
            .expect_correction(
                indoc! {r#"
                    obj.to_s(:delimited)
                        ^^^^ Use `to_formatted_s` instead.
                "#},
                "obj.to_formatted_s(:delimited)\n",
            )
            .expect_no_offenses("obj.to_formatted_s(:delimited)\n");
    }
}
murphy_plugin_api::submit_cop!(ToSWithArgument);
