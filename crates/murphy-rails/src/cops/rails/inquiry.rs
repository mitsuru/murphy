//! `Rails/Inquiry` — flag Active Support `inquiry` on String/Array literals.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Inquiry
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:inquiry] gating,
//!   zero-arg gate, receiver must be Str or Array literal, offense on the
//!   `inquiry` selector. Csend (`&.inquiry`) matches like upstream
//!   `alias on_csend on_send`. No autocorrect upstream.
//! ```
//!
//! Checks that Active Support's `inquiry` method is not used.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Inquiry;

#[cop(
    name = "Rails/Inquiry",
    description = "Prefer Ruby's comparison operators over Active Support's `Array#inquiry` and `String#inquiry`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl Inquiry {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[inquiry]`.
    #[on_node(kind = "send", methods = ["inquiry"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("inquiry") {
        return;
    }
    let (receiver_opt, args_list) = match *cx.kind(node) {
        NodeKind::Send { receiver, args, .. } => (receiver.get(), args),
        NodeKind::Csend { receiver, args, .. } => (Some(receiver), args),
        _ => return,
    };
    // Upstream `node.arguments.empty?`.
    if !cx.list(args_list).is_empty() {
        return;
    }
    let Some(recv) = receiver_opt else {
        return;
    };
    if !matches!(*cx.kind(recv), NodeKind::Str(_) | NodeKind::Array(_)) {
        return;
    }
    cx.emit_offense(
        cx.loc(node).name,
        "Prefer Ruby's comparison operators over Active Support's `inquiry`.",
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::Inquiry;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_inquiry() {
        test::<Inquiry>().expect_offense(indoc! {r#"
            'two'.inquiry
                  ^^^^^^^ Prefer Ruby's comparison operators over Active Support's `inquiry`.
        "#});
    }

    #[test]
    fn flags_string_csend_inquiry() {
        test::<Inquiry>().expect_offense(indoc! {r#"
            'two'&.inquiry
                   ^^^^^^^ Prefer Ruby's comparison operators over Active Support's `inquiry`.
        "#});
    }

    #[test]
    fn flags_array_inquiry() {
        test::<Inquiry>().expect_offense(indoc! {r#"
            [foo, bar].inquiry
                       ^^^^^^^ Prefer Ruby's comparison operators over Active Support's `inquiry`.
        "#});
    }

    #[test]
    fn does_not_flag_bare_inquiry() {
        test::<Inquiry>().expect_no_offenses("inquiry\n");
    }

    #[test]
    fn does_not_flag_variable_receiver() {
        test::<Inquiry>().expect_no_offenses("foo.inquiry\n");
    }

    #[test]
    fn does_not_flag_with_arguments() {
        test::<Inquiry>().expect_no_offenses("'foo'.inquiry(bar)\n");
    }

    #[test]
    fn does_not_flag_dstr_receiver() {
        test::<Inquiry>().expect_no_offenses("\"#{foo}\".inquiry\n");
    }

    #[test]
    fn does_not_flag_int_receiver() {
        test::<Inquiry>().expect_no_offenses("1.inquiry\n");
    }
}
murphy_plugin_api::submit_cop!(Inquiry);
