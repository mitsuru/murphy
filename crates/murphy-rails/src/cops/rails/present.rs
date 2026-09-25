//! `Rails/Present` — prefer `present?` over `!blank?`, `!nil? && !empty?`, and `unless blank?`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Present
//! upstream_version_checked: 2.35.0
//! version_added: "0.48"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 with default config (`NotNilAndNotEmpty`,
//!   `NotBlank`, `UnlessBlank` all true; the three flags are also exposed
//!   as cop options): `!foo.blank?` → `foo.present?` (on_send `!`),
//!   `!foo.nil? && !foo.empty?` (plus `!!foo`, `foo != nil`, and bare-truthy
//!   left sides) → `foo.present?` (on_and; on_or carries the same matcher,
//!   which is unreachable in practice as upstream matches an `and` shape),
//!   and `unless foo.blank?` → `if foo.present?` (on_if, modifier and block
//!   forms). Receiver equality uses source text (upstream uses structural
//!   node equality). `unless` with an `else` branch is skipped, matching the
//!   default-enabled `Style/UnlessElse` interplay (Murphy cannot read another
//!   cop's config). Upstream Include gating absent.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Present;

/// Upstream default-true feature flags, also exposed as cop options.
#[derive(CopOptions)]
pub struct PresentOptions {
    #[option(
        name = "NotNilAndNotEmpty",
        default = true,
        description = "Convert `!nil? && !empty?` usages to `present?`."
    )]
    pub not_nil_and_not_empty: bool,
    #[option(
        name = "NotBlank",
        default = true,
        description = "Convert `!blank?` usages to `present?`."
    )]
    pub not_blank: bool,
    #[option(
        name = "UnlessBlank",
        default = true,
        description = "Convert `unless blank?` usages to `if present?`."
    )]
    pub unless_blank: bool,
}

#[cop(
    name = "Rails/Present",
    description = "Enforces use of `present?`.",
    default_severity = "warning",
    default_enabled = true,
    options = PresentOptions,
)]
impl Present {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[!]`.
    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.options_or_default::<PresentOptions>().not_blank {
            return;
        }
        let Some(recv_src) = not_blank_receiver(cx, node) else {
            return;
        };
        let prefer = present_for(recv_src.as_deref());
        let current = cx.raw_source(cx.range(node));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &prefer);
    }

    // Mirrors upstream `on_and` (NotNilAndNotEmpty).
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.options_or_default::<PresentOptions>().not_nil_and_not_empty {
            return;
        }
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some(var_src) = exists_receiver(cx, lhs) else {
            return;
        };
        let Some(empty_src) = not_empty_receiver(cx, rhs) else {
            return;
        };
        if var_src != empty_src {
            return;
        }
        let prefer = format!("{var_src}.present?");
        let current = cx.raw_source(cx.range(node));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &prefer);
    }

    // Mirrors upstream `on_or` (same matcher; unreachable in practice
    // because the pattern is an `and` shape, kept for parity).
    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.options_or_default::<PresentOptions>().not_nil_and_not_empty {
            return;
        }
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some(var_src) = exists_receiver(cx, lhs) else {
            return;
        };
        let Some(empty_src) = not_empty_receiver(cx, rhs) else {
            return;
        };
        if var_src != empty_src {
            return;
        }
        let prefer = format!("{var_src}.present?");
        let current = cx.raw_source(cx.range(node));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &prefer);
    }

    // Mirrors upstream `on_if` (UnlessBlank).
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.options_or_default::<PresentOptions>().unless_blank {
            return;
        }
        if cx.is_elsif(node) || !cx.is_unless(node) {
            return;
        }
        // Matches the default-enabled `Style/UnlessElse` interplay:
        // `unless ... else` is left to that cop.
        if cx.is_else(node) {
            return;
        }
        let Some(cond) = cx.if_condition(node).get() else {
            return;
        };
        if !is_zero_arg_call(cx, cond, "blank?") {
            return;
        }
        let recv_src = call_receiver_source(cx, cond);
        let prefer = present_for(recv_src.as_deref());
        let range = unless_condition_range(cx, node, cond);
        let current = cx.raw_source(range);
        cx.emit_offense(
            range,
            &format!("Use `if {prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.if_keyword_loc(node), "if");
        cx.emit_edit(cx.range(cond), &prefer);
    }
}

/// `(send (send $_ :blank?) :!)` — returns the blank? receiver source
/// (`None` for a bare `!blank?`).
fn not_blank_receiver(cx: &Cx<'_>, node: NodeId) -> Option<Option<String>> {
    if cx.method_name(node) != Some("!") || !cx.call_arguments(node).is_empty() {
        return None;
    }
    let inner = cx.call_receiver(node).get()?;
    if !is_zero_arg_call(cx, inner, "blank?") {
        return None;
    }
    Some(call_receiver_source(cx, inner))
}

/// Left side of `exists_and_not_empty?`: `!x.nil?`, `!!x`, `x != nil`,
/// or a bare truthy `x`. Returns the checked receiver's source.
fn exists_receiver(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    // `(send $_ :!= nil)` — `x != nil`.
    if matches!(*cx.kind(node), NodeKind::Send { .. })
        && cx.method_name(node) == Some("!=")
    {
        let args = cx.call_arguments(node);
        if args.len() == 1 && matches!(*cx.kind(args[0]), NodeKind::Nil) {
            return cx.call_receiver(node).get().map(|r| cx.raw_source(cx.range(r)).to_owned());
        }
        return None;
    }
    // `!`-sends: `!x.nil?` or `!!x`.
    if matches!(*cx.kind(node), NodeKind::Send { .. }) && cx.method_name(node) == Some("!") && cx.call_arguments(node).is_empty() {
        let inner = cx.call_receiver(node).get()?;
        if is_zero_arg_call(cx, inner, "nil?") {
            return call_receiver_source(cx, inner);
        }
        // `(send (send $_ :!) :!)` — `!!x`.
        if matches!(*cx.kind(inner), NodeKind::Send { .. })
            && cx.method_name(inner) == Some("!")
            && cx.call_arguments(inner).is_empty()
        {
            return cx.call_receiver(inner).get().map(|r| cx.raw_source(cx.range(r)).to_owned());
        }
        return None;
    }
    // Bare `$_` — truthiness check.
    Some(cx.raw_source(cx.range(node)).to_owned())
}

/// `(send (send $_ :empty?) :!)` — `!x.empty?`. Returns the receiver source.
fn not_empty_receiver(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    if cx.method_name(node) != Some("!") || !cx.call_arguments(node).is_empty() {
        return None;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    let inner = cx.call_receiver(node).get()?;
    if !is_zero_arg_call(cx, inner, "empty?") {
        return None;
    }
    call_receiver_source(cx, inner)
}

/// True if `node` is a zero-argument `Send`/`Csend` named `name`.
fn is_zero_arg_call(cx: &Cx<'_>, node: NodeId, name: &str) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    if cx.method_name(node) != Some(name) {
        return false;
    }
    cx.call_arguments(node).is_empty()
}

/// Source of a call's receiver (`None` for bare calls).
fn call_receiver_source(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    cx.call_receiver(node).get().map(|r| cx.raw_source(cx.range(r)).to_owned())
}

fn present_for(recv_src: Option<&str>) -> String {
    match recv_src {
        Some(src) => format!("{src}.present?"),
        None => "present?".to_owned(),
    }
}

/// Upstream `unless_condition`: modifier form covers keyword-to-end,
/// block form covers start-to-condition-end.
fn unless_condition_range(cx: &Cx<'_>, node: NodeId, cond: NodeId) -> murphy_plugin_api::Range {
    if cx.is_modifier_form(node) {
        murphy_plugin_api::Range {
            start: cx.if_keyword_loc(node).start,
            end: cx.range(node).end,
        }
    } else {
        murphy_plugin_api::Range {
            start: cx.range(node).start,
            end: cx.range(cond).end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Present;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_not_blank() {
        test::<Present>().expect_correction(
            indoc! {r#"
                !foo.blank?
                ^^^^^^^^^^^ Use `foo.present?` instead of `!foo.blank?`.
            "#},
            "foo.present?\n",
        );
    }

    #[test]
    fn flags_not_nil_and_not_empty() {
        test::<Present>().expect_correction(
            indoc! {r#"
                !foo.nil? && !foo.empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `foo.present?` instead of `!foo.nil? && !foo.empty?`.
            "#},
            "foo.present?\n",
        );
    }

    #[test]
    fn flags_not_equal_nil_and_not_empty() {
        test::<Present>().expect_correction(
            indoc! {r#"
                foo != nil && !foo.empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `foo.present?` instead of `foo != nil && !foo.empty?`.
            "#},
            "foo.present?\n",
        );
    }

    #[test]
    fn flags_unless_blank_modifier() {
        test::<Present>().expect_correction(
            indoc! {r#"
                something unless foo.blank?
                          ^^^^^^^^^^^^^^^^^ Use `if foo.present?` instead of `unless foo.blank?`.
            "#},
            "something if foo.present?\n",
        );
    }

    #[test]
    fn flags_unless_blank_block() {
        test::<Present>().expect_correction(
            indoc! {r#"
                unless foo.blank?
                ^^^^^^^^^^^^^^^^^ Use `if foo.present?` instead of `unless foo.blank?`.
                  something
                end
            "#},
            "if foo.present?\n  something\nend\n",
        );
    }

    #[test]
    fn allows_mismatched_receivers() {
        test::<Present>().expect_no_offenses("!foo.nil? && !bar.empty?\n");
    }

    #[test]
    fn allows_unless_else() {
        test::<Present>().expect_no_offenses(indoc! {r#"
            unless foo.blank?
              something
            else
              other
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(Present);
