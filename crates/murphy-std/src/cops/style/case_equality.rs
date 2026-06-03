//! `Style/CaseEquality` -- flags explicit use of the case equality operator (`===`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CaseEquality
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Flags `===` operator usage (method `===` on Send nodes).
//!     - Skips regexp receivers unconditionally (handled by Performance/RegexpMatch).
//!     - Skips all-uppercase constant receivers unconditionally (e.g. SOME_CONSTANT).
//!     - AllowOnConstant option: when true, skips all Const receivers.
//!     - AllowOnSelfClass option: when true, skips `self.class` receiver.
//!     - Autocorrects `Array === x` -> `x.is_a?(Array)` (Const receiver with module name).
//!     - Autocorrects `self.class === x` -> `x.is_a?(self.class)`.
//!   Gap (v1 limitation):
//!     - Parenthesized range receivers like `(1..100) === 7` are not autocorrected.
//!       Murphy translates parenthesized expressions to `Unknown` nodes; the source
//!       range of the receiver is preserved in offense output but the autocorrect
//!       `(1..100).include?(7)` is not emitted. The offense is still reported.
//!     - General expression receivers (`foo === bar`, `some_obj === x`) produce an
//!       offense without autocorrect (safe/correct rewrite not always possible).
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `===`.
//!
//! Exemptions (no offense):
//! - Receiver is a regexp literal (`/re/`) -- handled by Performance/RegexpMatch.
//! - Receiver is an all-uppercase constant (no lowercase in name), e.g. `FOO`.
//! - Receiver is any Const when `AllowOnConstant: true`.
//! - Receiver is `self.class` when `AllowOnSelfClass: true`.
//!
//! ## Autocorrect
//!
//! - `SomeClass === x` -> `x.is_a?(SomeClass)` (Const with module name).
//! - `self.class === x` -> `x.is_a?(self.class)`.
//! - Other shapes: offense emitted, no autocorrect (unsafe/unknown rewrite).

use murphy_plugin_api::{cop, CopOptions, Cx, NodeId, NodeKind, OptNodeId};

const MSG: &str = "Avoid the use of the case equality operator `===`.";

#[derive(Default)]
pub struct CaseEquality;

#[derive(CopOptions)]
pub struct CaseEqualityOptions {
    #[option(
        name = "AllowOnConstant",
        default = false,
        description = "When true, ignore `===` when the receiver is a constant."
    )]
    pub allow_on_constant: bool,

    #[option(
        name = "AllowOnSelfClass",
        default = false,
        description = "When true, ignore `===` when the receiver is `self.class`."
    )]
    pub allow_on_self_class: bool,
}

#[cop(
    name = "Style/CaseEquality",
    description = "Avoid explicit use of the case equality operator (`===`).",
    default_severity = "warning",
    default_enabled = true,
    options = CaseEqualityOptions,
)]
impl CaseEquality {
    #[on_node(kind = "send", methods = ["==="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };

        let opts = cx.options_or_default::<CaseEqualityOptions>();

        // Receiver must be present.
        let Some(lhs) = receiver.get() else {
            return;
        };

        // Exactly one argument (rhs operand).
        let arg_list = cx.list(args);
        if arg_list.len() != 1 {
            return;
        }
        let rhs = arg_list[0];

        let lhs_kind = cx.kind(lhs);

        // Skip regexp receivers -- handled by Performance/RegexpMatch.
        if matches!(lhs_kind, NodeKind::Regexp { .. }) {
            return;
        }

        // Const receivers: check module_name? and AllowOnConstant option.
        if let NodeKind::Const { name, .. } = lhs_kind {
            // Skip all-uppercase constants unconditionally (no lowercase letters in name).
            // RuboCop's `!module_name?` check: module_name? returns true iff the name
            // contains at least one lowercase letter.
            let name_str = cx.symbol_str(*name);
            let has_lowercase = name_str.chars().any(|c| c.is_lowercase());
            if !has_lowercase {
                // Screaming-case constant like FOO or SOME_CONSTANT -- skip.
                return;
            }

            // If AllowOnConstant is enabled, skip all constants with module names.
            if opts.allow_on_constant {
                return;
            }
        }

        // self.class receiver: check AllowOnSelfClass option.
        if is_self_class(lhs, cx) && opts.allow_on_self_class {
            return;
        }

        // Emit offense on the `===` selector.
        let offense_range = cx.selector(node);
        cx.emit_offense(offense_range, MSG, None);

        // Emit autocorrect for known shapes.
        let lhs_src = cx.raw_source(cx.range(lhs));
        let rhs_src = cx.raw_source(cx.range(rhs));

        let replacement = if matches!(cx.kind(lhs), NodeKind::Const { .. }) {
            // `Array === x` -> `x.is_a?(Array)`
            Some(format!("{rhs_src}.is_a?({lhs_src})"))
        } else if is_self_class(lhs, cx) {
            // `self.class === x` -> `x.is_a?(self.class)`
            Some(format!("{rhs_src}.is_a?({lhs_src})"))
        } else {
            // Unknown/variable/other receiver -- no autocorrect.
            None
        };

        if let Some(replacement) = replacement {
            cx.emit_edit(cx.range(node), &replacement);
        }
    }
}

/// Returns `true` if `node` is a `self.class` send:
/// `Send { receiver: SelfExpr, method: :class, args: [] }`.
fn is_self_class(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    // Must have no arguments.
    if !cx.list(args).is_empty() {
        return false;
    }
    // Method must be :class.
    if cx.symbol_str(method) != "class" {
        return false;
    }
    // Receiver must be SelfExpr.
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    matches!(cx.kind(recv_id), NodeKind::SelfExpr)
}

// Keep OptNodeId in scope so the `use` is load-bearing.
const _: () = {
    let _ = std::mem::size_of::<OptNodeId>();
};

#[cfg(test)]
mod tests {
    use super::{CaseEquality, CaseEqualityOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases -----

    #[test]
    fn flags_case_equality_with_range() {
        // Parenthesized range -> offense without autocorrect (Unknown node in Murphy).
        test::<CaseEquality>().expect_offense(indoc! {r#"
            (1..100) === 7
                     ^^^ Avoid the use of the case equality operator `===`.
        "#});
    }

    #[test]
    fn flags_case_equality_with_const() {
        test::<CaseEquality>().expect_correction(
            indoc! {r#"
                Array === something
                      ^^^ Avoid the use of the case equality operator `===`.
            "#},
            "something.is_a?(Array)\n",
        );
    }

    #[test]
    fn flags_case_equality_with_self_class() {
        test::<CaseEquality>().expect_correction(
            indoc! {r#"
                self.class === something
                           ^^^ Avoid the use of the case equality operator `===`.
            "#},
            "something.is_a?(self.class)\n",
        );
    }

    #[test]
    fn flags_case_equality_with_variable_receiver() {
        // Variable receiver: offense emitted, no autocorrect.
        test::<CaseEquality>().expect_offense(indoc! {r#"
            foo === bar
                ^^^ Avoid the use of the case equality operator `===`.
        "#});
    }

    #[test]
    fn flags_case_equality_with_namespaced_const() {
        test::<CaseEquality>().expect_correction(
            indoc! {r#"
                Foo::Bar === something
                         ^^^ Avoid the use of the case equality operator `===`.
            "#},
            "something.is_a?(Foo::Bar)\n",
        );
    }

    // ----- Negative cases (no offenses) -----

    #[test]
    fn accepts_regexp_receiver() {
        // Regexp === var: handled by Performance/RegexpMatch, skipped here.
        test::<CaseEquality>().expect_no_offenses("/regexp/ === var\n");
    }

    #[test]
    fn accepts_screaming_case_constant() {
        // All-uppercase constant: module_name? returns false, unconditionally skipped.
        test::<CaseEquality>().expect_no_offenses("FOO === something\n");
    }

    #[test]
    fn accepts_screaming_case_constant_with_underscores() {
        test::<CaseEquality>().expect_no_offenses("SOME_CONSTANT === x\n");
    }

    #[test]
    fn accepts_const_with_allow_on_constant_true() {
        test::<CaseEquality>()
            .with_options(&CaseEqualityOptions {
                allow_on_constant: true,
                allow_on_self_class: false,
            })
            .expect_no_offenses("Array === something\n");
    }

    #[test]
    fn still_flags_const_with_allow_on_constant_false() {
        test::<CaseEquality>()
            .with_options(&CaseEqualityOptions {
                allow_on_constant: false,
                allow_on_self_class: false,
            })
            .expect_offense(indoc! {r#"
                Array === something
                      ^^^ Avoid the use of the case equality operator `===`.
            "#});
    }

    #[test]
    fn accepts_self_class_with_allow_on_self_class_true() {
        test::<CaseEquality>()
            .with_options(&CaseEqualityOptions {
                allow_on_constant: false,
                allow_on_self_class: true,
            })
            .expect_no_offenses("self.class === something\n");
    }

    #[test]
    fn still_flags_self_class_with_allow_on_self_class_false() {
        test::<CaseEquality>()
            .with_options(&CaseEqualityOptions {
                allow_on_constant: false,
                allow_on_self_class: false,
            })
            .expect_offense(indoc! {r#"
                self.class === something
                           ^^^ Avoid the use of the case equality operator `===`.
            "#});
    }

    // ----- Options config JSON -----

    #[test]
    fn config_json_allow_on_constant_true() {
        use murphy_plugin_api::CopOptions;
        let opts =
            CaseEqualityOptions::from_config_json(br#"{"AllowOnConstant": true}"#).expect("valid");
        assert!(opts.allow_on_constant);
        assert!(!opts.allow_on_self_class);
    }

    #[test]
    fn config_json_allow_on_self_class_true() {
        use murphy_plugin_api::CopOptions;
        let opts =
            CaseEqualityOptions::from_config_json(br#"{"AllowOnSelfClass": true}"#).expect("valid");
        assert!(!opts.allow_on_constant);
        assert!(opts.allow_on_self_class);
    }

    // ----- No corrections for shapes without autocorrect -----

    #[test]
    fn no_correction_for_range_receiver() {
        // (1..100) === 7: offense but no autocorrect (Unknown node).
        test::<CaseEquality>().expect_no_corrections("(1..100) === 7\n");
    }

    #[test]
    fn no_correction_for_variable_receiver() {
        test::<CaseEquality>().expect_no_corrections("foo === bar\n");
    }
}

murphy_plugin_api::submit_cop!(CaseEquality);
