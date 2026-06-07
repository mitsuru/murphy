//! `Lint/UselessDefined` — Checks for calls to `defined?` with strings or symbols as the argument.
//!
//! Such calls will always return `"expression"`, so the check is useless.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessDefined
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: string, interpolated string, symbol,
//!   interpolated symbol arguments flagged; constant, method ref, chained call accepted.
//! ```
//!
//! ## Matched shapes
//!
//! - `defined?("string")` — string literal argument.
//! - `defined?(:symbol)` — symbol literal argument.
//! - `defined?("interpolated #{x}")` — interpolated string argument.
//! - `defined?(:"interpolated #{x}")` — interpolated symbol argument.
//!
//! ## No autocorrect
//!
//! There is no safe mechanical rewrite: the correct fix depends on intent
//! (switch to `const_defined?`, `method_defined?`, etc.).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

fn msg(type_name: &str) -> String {
    format!("Calling `defined?` with a {type_name} argument will always return a truthy value.")
}

fn arg_type_name(kind: &NodeKind) -> Option<&'static str> {
    match kind {
        NodeKind::Str(_) | NodeKind::Dstr(_) => Some("string"),
        NodeKind::Sym(_) | NodeKind::Dsym(_) => Some("symbol"),
        _ => None,
    }
}

#[derive(Default)]
pub struct UselessDefined;

#[cop(
    name = "Lint/UselessDefined",
    description = "Checks for calls to `defined?` with strings or symbols.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UselessDefined {
    #[on_node(kind = "defined")]
    fn check_defined(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Defined(arg) = *cx.kind(node) else {
            return;
        };
        let arg_kind = cx.kind(arg);
        let Some(type_name) = arg_type_name(arg_kind) else {
            return;
        };
        let msg = msg(type_name);
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

#[cfg(test)]
mod tests {
    use super::UselessDefined;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_defined_with_string() {
        test::<UselessDefined>().expect_offense(indoc! {r#"
            defined?("FooBar")
            ^^^^^^^^^^^^^^^^^^ Calling `defined?` with a string argument will always return a truthy value.
        "#});
    }

    #[test]
    fn flags_defined_with_interpolated_string() {
        test::<UselessDefined>().expect_offense(indoc! {r#"
            defined?("Foo#{bar}")
            ^^^^^^^^^^^^^^^^^^^^^ Calling `defined?` with a string argument will always return a truthy value.
        "#});
    }

    #[test]
    fn flags_defined_with_symbol() {
        test::<UselessDefined>().expect_offense(indoc! {r#"
            defined?(:FooBar)
            ^^^^^^^^^^^^^^^^^ Calling `defined?` with a symbol argument will always return a truthy value.
        "#});
    }

    #[test]
    fn flags_defined_with_interpolated_symbol() {
        test::<UselessDefined>().expect_offense(indoc! {r#"
            defined?(:"Foo#{bar}")
            ^^^^^^^^^^^^^^^^^^^^^^ Calling `defined?` with a symbol argument will always return a truthy value.
        "#});
    }

    #[test]
    fn accepts_defined_with_constant() {
        test::<UselessDefined>().expect_no_offenses(indoc! {"
            defined?(FooBar)
        "});
    }

    #[test]
    fn accepts_defined_with_method() {
        test::<UselessDefined>().expect_no_offenses(indoc! {"
            defined?(foo_bar)
        "});
    }
}

murphy_plugin_api::submit_cop!(UselessDefined);
