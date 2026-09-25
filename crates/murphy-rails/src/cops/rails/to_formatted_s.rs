//! `Rails/ToFormattedS` — consistent `to_fs` / `to_formatted_s`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ToFormattedS
//! upstream_version_checked: 2.35.0
//! version_added: "2.15"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`/`on_csend`
//!   (`RESTRICT_ON_SEND = %i[to_formatted_s to_fs]`) behind
//!   `minimum_target_rails_version 7.0` (unset means newest): flags the
//!   non-`EnforcedStyle` spelling on the selector and rewrites it to
//!   the style (`to_fs` default). Disabled upstream by default
//!   (`Enabled: pending`).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ToFormattedS;

#[derive(CopOptions)]
pub struct ToFormattedSOptions {
    #[option(
        name = "EnforcedStyle",
        default = "to_fs",
        description = "Whether to enforce `to_fs` or `to_formatted_s`."
    )]
    pub enforced_style: ToFormattedSStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ToFormattedSStyle {
    #[option(value = "to_fs")]
    ToFs,
    #[option(value = "to_formatted_s")]
    ToFormattedS,
}

impl ToFormattedSStyle {
    fn as_str(self) -> &'static str {
        match self {
            ToFormattedSStyle::ToFs => "to_fs",
            ToFormattedSStyle::ToFormattedS => "to_formatted_s",
        }
    }
}

#[cop(
    name = "Rails/ToFormattedS",
    description = "Checks for consistent uses of `to_fs` or `to_formatted_s`.",
    default_severity = "warning",
    default_enabled = false,
    options = ToFormattedSOptions,
)]
impl ToFormattedS {
    #[on_node(kind = "send", methods = ["to_formatted_s", "to_fs"])]
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
    // Upstream `minimum_target_rails_version 7.0` — unset means newest.
    if !cx.rails_version_at_least(7, 0) {
        return;
    }
    let Some(method) = cx.method_name(node) else {
        return;
    };
    if method != "to_formatted_s" && method != "to_fs" {
        return;
    }
    let opts = cx.options_or_default::<ToFormattedSOptions>();
    let prefer = opts.enforced_style.as_str();
    if method == prefer {
        return;
    }
    cx.emit_offense(cx.loc(node).name, &format!("Use `{prefer}` instead."), None);
    cx.emit_edit(cx.loc(node).name, prefer);
}

#[cfg(test)]
mod tests {
    use super::{ToFormattedS, ToFormattedSOptions, ToFormattedSStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_formatted_s_by_default() {
        test::<ToFormattedS>().expect_offense(indoc! {r#"
            time.to_formatted_s(:db)
                 ^^^^^^^^^^^^^^ Use `to_fs` instead.
        "#});
    }

    #[test]
    fn allows_to_fs_by_default() {
        test::<ToFormattedS>().expect_no_offenses("time.to_fs(:db)\n");
    }

    #[test]
    fn flags_to_fs_when_style_is_to_formatted_s() {
        let opts = ToFormattedSOptions {
            enforced_style: ToFormattedSStyle::ToFormattedS,
        };
        test::<ToFormattedS>().with_options(&opts).expect_offense(indoc! {r#"
            time.to_fs(:db)
                 ^^^^^ Use `to_formatted_s` instead.
        "#});
    }

    #[test]
    fn allows_to_formatted_s_when_style_is_to_formatted_s() {
        let opts = ToFormattedSOptions {
            enforced_style: ToFormattedSStyle::ToFormattedS,
        };
        test::<ToFormattedS>()
            .with_options(&opts)
            .expect_no_offenses("time.to_formatted_s(:db)\n");
    }

    #[test]
    fn does_not_flag_below_rails_70() {
        test::<ToFormattedS>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses("time.to_formatted_s(:db)\n");
    }

    #[test]
    fn fires_at_rails_70() {
        test::<ToFormattedS>()
            .with_target_rails_version(7, 0)
            .expect_offense(indoc! {r#"
                time.to_formatted_s(:db)
                     ^^^^^^^^^^^^^^ Use `to_fs` instead.
            "#});
    }

    #[test]
    fn corrects_to_to_fs() {
        test::<ToFormattedS>()
            .expect_correction(
                indoc! {r#"
                    time.to_formatted_s(:db)
                         ^^^^^^^^^^^^^^ Use `to_fs` instead.
                "#},
                "time.to_fs(:db)\n",
            )
            .expect_no_offenses("time.to_fs(:db)\n");
    }

    #[test]
    fn corrects_safe_navigation() {
        test::<ToFormattedS>()
            .expect_correction(
                indoc! {r#"
                    time&.to_formatted_s(:db)
                          ^^^^^^^^^^^^^^ Use `to_fs` instead.
                "#},
                "time&.to_fs(:db)\n",
            )
            .expect_no_offenses("time&.to_fs(:db)\n");
    }
}
murphy_plugin_api::submit_cop!(ToFormattedS);
