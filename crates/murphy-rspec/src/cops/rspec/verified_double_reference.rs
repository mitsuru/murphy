//! `RSpec/VerifiedDoubleReference` — use constant references for verified doubles.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/VerifiedDoubleReference
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `verified_double` (`(send nil? RESTRICT_ON_SEND $str
//!   ...)`): bare-receiver `class_double` / `class_spy` / `instance_double`
//!   / `instance_spy` / `mock_model` / `object_double` / `object_spy` /
//!   `stub_model` whose first arg is a `Str` flags that arg per
//!   `add_offense(string_argument_node)` with `Use a constant class
//!   reference for verified doubles. String references are not verifying
//!   unless the class is loaded.` Non-`Str` first args (`Const`, `Lvar`,
//!   `Ivar`, `Array`, `Sym`, `Dstr`) stay clean. Detection is at parity
//!   (verified vs 3.7.0, including the `Foo::Bar::Baz` and `::Foo::Bar`
//!   string cases); autocorrect (replace the string with its contents) is
//!   not ported in this batch — same convention as `RSpec/BeNil`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with the eight verified-double methods (bare
//! receiver only):
//!
//! - `instance_double('ClassName')` — flagged (the string arg).
//! - `instance_double(ClassName)` — const, clean.
//! - `instance_double(klass)` — lvar, clean.
//! - `instance_double(@sut)` — ivar, clean.
//! - `object_double([])` — array, clean.
//! - `class_double(:Model)` — sym, clean.
//! - `obj.instance_double('X')` — explicit receiver, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces the string with its contents. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct VerifiedDoubleReference;

#[cop(
    name = "RSpec/VerifiedDoubleReference",
    description = "Checks for consistent verified double reference style.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl VerifiedDoubleReference {
    #[on_node(
        kind = "send",
        methods = [
            "class_double",
            "class_spy",
            "instance_double",
            "instance_spy",
            "mock_model",
            "object_double",
            "object_spy",
            "stub_model"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let Some(&first) = cx.list(args).first() else {
            return;
        };
        if !matches!(*cx.kind(first), NodeKind::Str(_)) {
            return;
        }
        cx.emit_offense(
            cx.range(first),
            "Use a constant class reference for verified doubles. String references are not verifying unless the class is loaded.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::VerifiedDoubleReference;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_reference() {
        test::<VerifiedDoubleReference>().expect_offense(indoc! {r#"
                instance_double('ClassName')
                                ^^^^^^^^^^^ Use a constant class reference for verified doubles. String references are not verifying unless the class is loaded.
            "#});
    }

    #[test]
    fn flags_namespaced_string() {
        test::<VerifiedDoubleReference>().expect_offense(indoc! {r#"
                instance_double('Foo::Bar::Baz')
                                ^^^^^^^^^^^^^^^ Use a constant class reference for verified doubles. String references are not verifying unless the class is loaded.
            "#});
    }

    #[test]
    fn does_not_flag_const() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                instance_double(ClassName)
            "#});
    }

    #[test]
    fn does_not_flag_lvar() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                klass = Array
                instance_double(klass)
            "#});
    }

    #[test]
    fn does_not_flag_ivar() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                instance_double(@sut)
            "#});
    }

    #[test]
    fn does_not_flag_array() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                object_double([])
            "#});
    }

    #[test]
    fn does_not_flag_sym() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                class_double(:Model)
            "#});
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<VerifiedDoubleReference>().expect_no_offenses(indoc! {r#"
                obj.instance_double('X')
            "#});
    }

    #[test]
    fn flags_each_verified_double() {
        for method in [
            "class_double",
            "class_spy",
            "instance_double",
            "instance_spy",
            "mock_model",
            "object_double",
            "object_spy",
            "stub_model",
        ] {
            let src = format!("{method}('X')\n");
            let offenses =
                murphy_plugin_api::test_support::run_cop::<VerifiedDoubleReference>(&src);
            assert_eq!(offenses.len(), 1, "method {method} should flag");
        }
    }
}

murphy_plugin_api::submit_cop!(VerifiedDoubleReference);
