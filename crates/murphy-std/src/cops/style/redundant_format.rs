//! `Style/RedundantFormat` — flags `format`/`sprintf` calls that pass a
//! single string, interpolated string, or constant argument (with no
//! additional format arguments), since the call is redundant.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantFormat
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - format/sprintf with a single str, dstr, or const argument and
//!       no additional arguments. Receiver must be nil, Kernel, or ::Kernel.
//!     - Autocorrect: replace the whole call with the single argument's source.
//!   Gaps:
//!     - "Inlinable literal arguments": format('%s %s', 'foo', 'bar') -> 'foo bar'.
//!       This path requires running Ruby's format() at lint time to build the
//!       replacement string safely (width/precision/flag handling, %% escapes,
//!       positional arguments, named-key arguments). Deferring to a follow-up
//!       issue to avoid incorrect autocorrects.
//!   Safety:
//!     - Autocorrect is unsafe because format() returns an unfrozen string,
//!       while the literal replacement may be frozen (with frozen_string_literal: true).
//!       Downstream `String#<<` calls could raise FrozenError after the correction.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantFormat;

const MSG: &str = "Use `%s` directly instead of `%s`.";

#[cop(
    name = "Style/RedundantFormat",
    description = "Checks for calls to `format` or `sprintf` that are redundant.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantFormat {
    #[on_node(kind = "send", methods = ["format", "sprintf"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Receiver must be nil, Kernel, or ::Kernel.
    if !is_nil_or_kernel_receiver(cx.call_receiver(node), cx) {
        return;
    }

    let method_name = cx.method_name(node).unwrap_or("format");

    // Must have exactly one argument.
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    let arg = args[0];

    // Argument must be str, dstr, or const.
    match cx.kind(arg) {
        NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Const { .. } => {}
        _ => return,
    }

    let arg_src = cx.raw_source(cx.range(arg));
    let msg = MSG
        .replacen("%s", arg_src, 1)
        .replacen("%s", method_name, 1);

    cx.emit_offense(cx.range(node), &msg, None);

    // Autocorrect: replace the whole call with the argument source.
    cx.emit_edit(cx.range(node), arg_src);
}

/// Returns true if the receiver is nil (implicit), `Kernel`, or `::Kernel`.
fn is_nil_or_kernel_receiver(receiver: OptNodeId, cx: &Cx<'_>) -> bool {
    match receiver.get() {
        None => true, // nil receiver (implicit call)
        Some(recv) => {
            // Kernel or ::Kernel
            matches!(cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(*name) == "Kernel")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantFormat;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_format_with_multiple_args() {
        // format with multiple arguments is not handled (partial impl).
        test::<RedundantFormat>().expect_no_offenses(r#"format('%s', 'hello')"#);
    }

    #[test]
    fn no_offense_format_with_no_args() {
        test::<RedundantFormat>().expect_no_offenses(r#"format()"#);
    }

    #[test]
    fn no_offense_arbitrary_receiver() {
        // format on an arbitrary receiver should not be flagged.
        test::<RedundantFormat>().expect_no_offenses(r#"obj.format('hello')"#);
    }

    #[test]
    fn no_offense_format_with_integer_arg() {
        test::<RedundantFormat>().expect_no_offenses("format(42)");
    }

    // --- Offense cases: format ---

    #[test]
    fn flags_format_with_single_string() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            format('the quick brown fox jumps over the lazy dog.')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `'the quick brown fox jumps over the lazy dog.'` directly instead of `format`.
        "#});
    }

    #[test]
    fn flags_sprintf_with_single_string() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            sprintf('the quick brown fox jumps over the lazy dog.')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `'the quick brown fox jumps over the lazy dog.'` directly instead of `sprintf`.
        "#});
    }

    #[test]
    fn flags_format_with_constant() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            format(MESSAGE)
            ^^^^^^^^^^^^^^^ Use `MESSAGE` directly instead of `format`.
        "#});
    }

    #[test]
    fn flags_sprintf_with_constant() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            sprintf(MESSAGE)
            ^^^^^^^^^^^^^^^^ Use `MESSAGE` directly instead of `sprintf`.
        "#});
    }

    #[test]
    fn flags_format_with_interpolated_string() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            format("hello #{name}")
            ^^^^^^^^^^^^^^^^^^^^^^^ Use `"hello #{name}"` directly instead of `format`.
        "#});
    }

    #[test]
    fn flags_kernel_format() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            Kernel.format('hello')
            ^^^^^^^^^^^^^^^^^^^^^^ Use `'hello'` directly instead of `format`.
        "#});
    }

    #[test]
    fn flags_kernel_cbase_format() {
        test::<RedundantFormat>().expect_offense(indoc! {r#"
            Kernel.sprintf('hello')
            ^^^^^^^^^^^^^^^^^^^^^^^ Use `'hello'` directly instead of `sprintf`.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_format_single_string() {
        test::<RedundantFormat>().expect_correction(
            indoc! {r#"
                format('the quick brown fox jumps over the lazy dog.')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `'the quick brown fox jumps over the lazy dog.'` directly instead of `format`.
            "#},
            "'the quick brown fox jumps over the lazy dog.'\n",
        );
    }

    #[test]
    fn corrects_sprintf_single_string() {
        test::<RedundantFormat>().expect_correction(
            indoc! {r#"
                sprintf('the quick brown fox jumps over the lazy dog.')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `'the quick brown fox jumps over the lazy dog.'` directly instead of `sprintf`.
            "#},
            "'the quick brown fox jumps over the lazy dog.'\n",
        );
    }

    #[test]
    fn corrects_format_constant() {
        test::<RedundantFormat>().expect_correction(
            indoc! {r#"
                format(MESSAGE)
                ^^^^^^^^^^^^^^^ Use `MESSAGE` directly instead of `format`.
            "#},
            "MESSAGE\n",
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantFormat);
