//! `Rails/ShortI18n` — use `t`/`l` instead of `translate`/`localize`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ShortI18n
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `(send {nil? (const {nil? cbase} :I18n)}
//!   {:translate :localize} ...)` with `conservative` (default, only
//!   `I18n.`-receiver calls flag) vs `aggressive` (bare calls flag too).
//!   `I18n` matches `I18n` and `::I18n` via `cx.const_name`. Offense is the
//!   selector; autocorrect replaces it with `t`/`l`.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ShortI18n;

#[derive(CopOptions)]
pub struct ShortI18nOptions {
    #[option(
        name = "EnforcedStyle",
        default = "conservative",
        description = "Whether only I18n.-receiver calls flag (conservative) or all translate/localize calls flag (aggressive)."
    )]
    pub enforced_style: ShortI18nStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ShortI18nStyle {
    #[option(value = "conservative")]
    Conservative,
    #[option(value = "aggressive")]
    Aggressive,
}

#[cop(
    name = "Rails/ShortI18n",
    description = "Use the short form of the I18n methods (`t`/`l`).",
    default_severity = "warning",
    default_enabled = false,
    options = ShortI18nOptions,
)]
impl ShortI18n {
    // Mirrors upstream `RESTRICT_ON_SEND = [:translate, :localize]`.
    #[on_node(kind = "send", methods = ["translate", "localize"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "translate" && method != "localize" {
        return;
    }
    let opts = cx.options_or_default::<ShortI18nOptions>();
    let receiver = cx.call_receiver(node).get();
    match receiver {
        Some(recv) => {
            // Upstream `(const {nil? cbase} :I18n)`: only I18n / ::I18n.
            if cx.const_name(recv).as_deref() != Some("I18n") {
                return;
            }
        }
        None => {
            // Conservative (default) skips bare calls.
            if opts.enforced_style == ShortI18nStyle::Conservative {
                return;
            }
        }
    }
    let good = if method == "translate" { "t" } else { "l" };
    cx.emit_offense(
        cx.selector(node),
        &format!("Use `{good}` instead of `{method}`."),
        None,
    );
    cx.emit_edit(cx.selector(node), good);
}

#[cfg(test)]
mod tests {
    use super::{ShortI18n, ShortI18nOptions, ShortI18nStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_i18n_translate_conservative() {
        test::<ShortI18n>().expect_correction(
            indoc! {r#"
                I18n.translate :key
                     ^^^^^^^^^ Use `t` instead of `translate`.
            "#},
            "I18n.t :key
",
        );
    }

    #[test]
    fn flags_i18n_localize_conservative() {
        test::<ShortI18n>().expect_correction(
            indoc! {r#"
                I18n.localize Time.now
                     ^^^^^^^^ Use `l` instead of `localize`.
            "#},
            "I18n.l Time.now
",
        );
    }

    #[test]
    fn allows_bare_translate_conservative() {
        test::<ShortI18n>().expect_no_offenses("translate :key
");
    }

    #[test]
    fn allows_other_receiver() {
        test::<ShortI18n>().expect_no_offenses("foo.translate :key
");
    }

    #[test]
    fn allows_short_forms() {
        test::<ShortI18n>().expect_no_offenses("I18n.t :key
");
    }

    #[test]
    fn flags_bare_translate_aggressive() {
        let opts = ShortI18nOptions {
            enforced_style: ShortI18nStyle::Aggressive,
        };
        test::<ShortI18n>().with_options(&opts).expect_correction(
            indoc! {r#"
                translate :key
                ^^^^^^^^^ Use `t` instead of `translate`.
            "#},
            "t :key
",
        );
    }

    #[test]
    fn flags_bare_localize_aggressive() {
        let opts = ShortI18nOptions {
            enforced_style: ShortI18nStyle::Aggressive,
        };
        test::<ShortI18n>().with_options(&opts).expect_correction(
            indoc! {r#"
                localize Time.now
                ^^^^^^^^ Use `l` instead of `localize`.
            "#},
            "l Time.now
",
        );
    }
}
murphy_plugin_api::submit_cop!(ShortI18n);
