//! `Style/PreferredHashMethods` — prefer shorter or verbose Hash method names.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PreferredHashMethods
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Verbatim port of the `has_key?`/`has_value?`/`key?`/`value?` call head
//!   `(call _ {:has_key? :has_value? :key? :value?} ...)` (murphy-s1yc.6):
//!   `call` = `{send csend}` covers safe-navigation (`h&.has_key?`),
//!   mirroring RuboCop's `alias on_csend on_send`; the `_` receiver binds an
//!   absent (receiverless `has_key?(k)`) or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list (upstream's
//!   `node.arguments.one?` guard and the EnforcedStyle filter apply
//!   separately).
//!   Ports both EnforcedStyle modes:
//!     - short (default): flags `has_key?` and `has_value?`, suggests `key?` and `value?`.
//!     - verbose: flags `key?` and `value?`, suggests `has_key?` and `has_value?`.
//!   Offense range and autocorrect target the selector only (loc.name).
//!   Marked unsafe in RuboCop because the receiver may not actually be a Hash.
//! ```
//!
//! ## Matched shapes
//!
//! - **short** (default): `hash.has_key?(k)` -> `hash.key?(k)`,
//!   `hash.has_value?(v)` -> `hash.value?(v)`
//! - **verbose**: `hash.key?(k)` -> `hash.has_key?(k)`,
//!   `hash.value?(v)` -> `hash.has_value?(v)`
//!
//! ## Autocorrect
//!
//! Surgical rename of the selector (`loc.name`) only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the `has_key?`/`has_value?`/`key?`/`value?` call head
// (murphy-s1yc.6):
// `(call _ {:has_key? :has_value? :key? :value?} ...)` — `call` =
// `{send csend}` covers safe-navigation (`h&.has_key?`); the `_` receiver
// binds an absent (receiverless `has_key?(k)`) or present receiver; trailing
// `...` absorbs any argument list, so the one-arg guard and the EnforcedStyle
// filter below apply separately.
def_node_matcher!(
    preferred_hash_methods_call,
    "(call _ {:has_key? :has_value? :key? :value?} ...)"
);

/// Stateless unit struct.
#[derive(Default)]
pub struct PreferredHashMethods;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PreferredHashMethodsStyle {
    #[default]
    #[option(value = "short")]
    Short,
    #[option(value = "verbose")]
    Verbose,
}

#[derive(CopOptions)]
pub struct PreferredHashMethodsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "short",
        description = "Preferred Hash method name style."
    )]
    pub enforced_style: PreferredHashMethodsStyle,
}

const MSG: &str = "Use `Hash#%preferred%` instead of `Hash#%current%`.";

#[cop(
    name = "Style/PreferredHashMethods",
    description = "Checks use of `has_key?` and `has_value?` Hash methods.",
    default_severity = "warning",
    default_enabled = true,
    options = PreferredHashMethodsOptions,
)]
impl PreferredHashMethods {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ {:has_key? :has_value? :key? :value?} ...)` head:
    // filters to the four Hash-method calls on either `send` or `csend`
    // (safe-navigation), with any receiver (absent or present). Trailing
    // `...` matches any arg list, so the one-arg guard and the EnforcedStyle
    // filter below apply separately.
    if !preferred_hash_methods_call(node, cx) {
        return;
    }
    // Must have exactly one argument (mirrors RuboCop's `node.arguments.one?`).
    let args = match *cx.kind(node) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => args,
        _ => return,
    };
    if cx.list(args).len() != 1 {
        return;
    }

    let opts = cx.options_or_default::<PreferredHashMethodsOptions>();
    let method_name = cx.method_name(node).unwrap_or_default();

    let offending = match opts.enforced_style {
        PreferredHashMethodsStyle::Short => matches!(method_name, "has_key?" | "has_value?"),
        PreferredHashMethodsStyle::Verbose => matches!(method_name, "key?" | "value?"),
    };
    if !offending {
        return;
    }

    let preferred = proper_method_name(method_name, opts.enforced_style);
    let message = MSG
        .replace("%preferred%", preferred)
        .replace("%current%", method_name);

    let selector_range = cx.node(node).loc.name;
    cx.emit_offense(selector_range, &message, None);
    cx.emit_edit(selector_range, preferred);
}

fn proper_method_name(method_name: &str, style: PreferredHashMethodsStyle) -> &'static str {
    match style {
        PreferredHashMethodsStyle::Short => {
            if method_name == "has_key?" {
                "key?"
            } else {
                "value?"
            }
        }
        PreferredHashMethodsStyle::Verbose => {
            if method_name == "key?" {
                "has_key?"
            } else {
                "has_value?"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Default style (short): flag has_key? / has_value? ---

    #[test]
    fn flags_has_key() {
        test::<PreferredHashMethods>().expect_offense(indoc! {r#"
            h.has_key?(:foo)
              ^^^^^^^^ Use `Hash#key?` instead of `Hash#has_key?`.
        "#});
    }

    #[test]
    fn flags_has_value() {
        test::<PreferredHashMethods>().expect_offense(indoc! {r#"
            h.has_value?(42)
              ^^^^^^^^^^ Use `Hash#value?` instead of `Hash#has_value?`.
        "#});
    }

    #[test]
    fn no_offense_for_key_in_short_mode() {
        test::<PreferredHashMethods>().expect_no_offenses("h.key?(:foo)\n");
    }

    #[test]
    fn no_offense_for_value_in_short_mode() {
        test::<PreferredHashMethods>().expect_no_offenses("h.value?(42)\n");
    }

    // --- Guard: exactly one argument ---

    #[test]
    fn no_offense_has_key_no_args() {
        test::<PreferredHashMethods>().expect_no_offenses("h.has_key?\n");
    }

    #[test]
    fn no_offense_has_key_two_args() {
        test::<PreferredHashMethods>().expect_no_offenses("h.has_key?(:a, :b)\n");
    }

    // --- Verbose style: flag key? / value? ---

    #[test]
    fn verbose_flags_key() {
        test::<PreferredHashMethods>()
            .with_options(&PreferredHashMethodsOptions {
                enforced_style: PreferredHashMethodsStyle::Verbose,
            })
            .expect_offense(indoc! {r#"
                h.key?(:foo)
                  ^^^^ Use `Hash#has_key?` instead of `Hash#key?`.
            "#});
    }

    #[test]
    fn verbose_flags_value() {
        test::<PreferredHashMethods>()
            .with_options(&PreferredHashMethodsOptions {
                enforced_style: PreferredHashMethodsStyle::Verbose,
            })
            .expect_offense(indoc! {r#"
                h.value?(42)
                  ^^^^^^ Use `Hash#has_value?` instead of `Hash#value?`.
            "#});
    }

    #[test]
    fn verbose_no_offense_for_has_key() {
        test::<PreferredHashMethods>()
            .with_options(&PreferredHashMethodsOptions {
                enforced_style: PreferredHashMethodsStyle::Verbose,
            })
            .expect_no_offenses("h.has_key?(:foo)\n");
    }

    // --- csend ---

    #[test]
    fn flags_csend_has_key() {
        test::<PreferredHashMethods>().expect_offense(indoc! {r#"
            h&.has_key?(:foo)
               ^^^^^^^^ Use `Hash#key?` instead of `Hash#has_key?`.
        "#});
    }

    #[test]
    fn flags_csend_has_value() {
        test::<PreferredHashMethods>().expect_offense(indoc! {r#"
            h&.has_value?(42)
               ^^^^^^^^^^ Use `Hash#value?` instead of `Hash#has_value?`.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn autocorrects_has_key_to_key() {
        test::<PreferredHashMethods>().expect_correction(
            indoc! {r#"
                h.has_key?(:foo)
                  ^^^^^^^^ Use `Hash#key?` instead of `Hash#has_key?`.
            "#},
            "h.key?(:foo)\n",
        );
    }

    #[test]
    fn autocorrects_has_value_to_value() {
        test::<PreferredHashMethods>().expect_correction(
            indoc! {r#"
                h.has_value?(42)
                  ^^^^^^^^^^ Use `Hash#value?` instead of `Hash#has_value?`.
            "#},
            "h.value?(42)\n",
        );
    }

    // --- Characterization (murphy-s1yc.6): pin the exact node set the
    // hand-rolled send/csend dispatch matches, so the verbatim
    // `(call _ {:has_key? :has_value? :key? :value?} ...)` port can be proven
    // byte-identical. `call` = `{send csend}` covers safe-navigation; `_`
    // binds the absent (receiverless) slot per if9y; trailing `...` absorbs
    // any arg list (the one-arg guard below applies separately, mirroring
    // upstream's `node.arguments.one?`).

    #[test]
    fn s1yc6_flags_bare_has_key_corrects_to_key() {
        // Bare `has_key?(:foo)` matches: `_` binds the nil-filled receiver.
        test::<PreferredHashMethods>().expect_correction(
            indoc! {r#"
                has_key?(:foo)
                ^^^^^^^^ Use `Hash#key?` instead of `Hash#has_key?`.
            "#},
            "key?(:foo)\n",
        );
    }

    #[test]
    fn s1yc6_accepts_bare_key_in_short_mode() {
        // Bare `key?(:foo)` in short mode: matcher matches, style filter rejects.
        test::<PreferredHashMethods>().expect_no_offenses("key?(:foo)\n");
    }

    #[test]
    fn s1yc6_accepts_bare_has_key_without_args() {
        // No-arg bare `has_key?`: matcher `...` matches zero args,
        // the one-arg guard rejects.
        test::<PreferredHashMethods>().expect_no_offenses("has_key?\n");
    }

    #[test]
    fn s1yc6_accepts_unrelated_method() {
        // `h.foo(:foo)`: matcher rejects non-union method.
        test::<PreferredHashMethods>().expect_no_offenses("h.foo(:foo)\n");
    }

    #[test]
    fn verbose_autocorrects_key_to_has_key() {
        test::<PreferredHashMethods>()
            .with_options(&PreferredHashMethodsOptions {
                enforced_style: PreferredHashMethodsStyle::Verbose,
            })
            .expect_correction(
                indoc! {r#"
                    h.key?(:foo)
                      ^^^^ Use `Hash#has_key?` instead of `Hash#key?`.
                "#},
                "h.has_key?(:foo)\n",
            );
    }
}

murphy_plugin_api::submit_cop!(PreferredHashMethods);
