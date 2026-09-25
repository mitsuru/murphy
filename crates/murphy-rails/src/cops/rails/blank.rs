//! `Rails/Blank` — flag `nil? || empty?` and `!present?` in favour of `blank?`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Blank
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `NilOrEmpty` (on_or) and `NotPresent`
//!   (on_send `!`) with default-true behaviour: the `NilOrEmpty`,
//!   `NotPresent`, and `UnlessPresent` config flags are not plumbed, so the
//!   cop always fires on the implemented shapes. `UnlessPresent` (on_if
//!   `unless present?` → `if blank?`, including the Style/UnlessElse
//!   interaction) is not implemented in v1. Receiver equality uses source
//!   text (upstream uses structural node equality). The `def blank?`
//!   exclusion walks up through `begin` wrappers (upstream checks only the
//!   direct parent).
//! ```
//!
//! ## Matched shapes
//!
//! - `Or` node: left side is a nil check (`foo.nil?`, `!foo`, `foo == nil`,
//!   `nil == foo`) and right side is an `empty?` check (`foo.empty?`, or
//!   doubly-negated `!!foo.empty?`), with source-equal receivers:
//!   `foo.nil? || foo.empty?` → `foo.blank?`.
//! - `Send` `!` whose receiver is a zero-arg `present?` send:
//!   `!foo.present?` → `foo.blank?` (skipped inside `def blank?`).
//!
//! ## Autocorrect
//!
//! Replace the whole node with `<receiver source>.blank?` (bare `blank?`
//! when the receiver has no source, e.g. `nil == foo` variants still carry
//! the right-hand receiver so this only triggers for receiver-less `!`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Blank;

#[cop(
    name = "Rails/Blank",
    description = "Use `blank?` instead of `nil? || empty?`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Blank {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[!]` for the NotPresent branch.
    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(inner) = receiver.get() else {
            return;
        };
        // `(send (send $_ :present?) :!)` — zero-arg `present?` receiver.
        let Some(present_recv) = as_zero_arg_send(cx, inner, "present?") else {
            return;
        };
        // Skip `!present?` in the body of `def blank?`.
        if inside_def_blank(cx, node) {
            return;
        }
        let prefer = blank_for(present_recv);
        let current = cx.raw_source(cx.range(node));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &prefer);
    }

    // Mirrors upstream `on_or` (NilOrEmpty).
    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some(var_src) = nil_check_receiver(cx, lhs) else {
            return;
        };
        let Some(empty_src) = empty_check_receiver(cx, rhs) else {
            return;
        };
        if var_src != empty_src {
            return;
        }
        let prefer = format!("{var_src}.blank?");
        let current = cx.raw_source(cx.range(node));
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{prefer}` instead of `{current}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &prefer);
    }
}

/// If `node` is a zero-arg `Send` with `method == name`, return its receiver
/// source (None for a bare call).
fn as_zero_arg_send(cx: &Cx<'_>, node: NodeId, name: &str) -> Option<Option<String>> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != name {
        return None;
    }
    if !cx.call_arguments(node).is_empty() {
        return None;
    }
    Some(receiver.get().map(|r| cx.raw_source(cx.range(r)).to_owned()))
}

/// Left side of `nil_or_empty?`: `!x`, `x.nil?`, `x == nil`, or `nil == x`.
/// Returns the checked receiver's source.
fn nil_check_receiver(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return None;
    };
    let name = cx.symbol_str(method);
    let args = cx.call_arguments(node);
    match name {
        // `(send $_ :!)` / `(send $_ :nil?)` — receiver must be present.
        "!" | "nil?" => {
            if !args.is_empty() {
                return None;
            }
            let recv = receiver.get()?;
            Some(cx.raw_source(cx.range(recv)).to_owned())
        }
        // `(send $_ :== nil)` — `x == nil`; `(send nil :== $_)` — `nil == x`.
        // Note `nil == x` carries the `nil` literal as its receiver node
        // (mirroring upstream, which matches it via the `nil` pattern).
        "==" => {
            if args.len() != 1 {
                return None;
            }
            match receiver.get() {
                Some(recv) if !matches!(*cx.kind(recv), NodeKind::Nil) => {
                    if !matches!(*cx.kind(args[0]), NodeKind::Nil) {
                        return None;
                    }
                    Some(cx.raw_source(cx.range(recv)).to_owned())
                }
                _ => Some(cx.raw_source(cx.range(args[0])).to_owned()),
            }
        }
        _ => None,
    }
}

/// Right side of `nil_or_empty?`: `x.empty?` or `!!x.empty?`.
/// Returns the checked receiver's source.
fn empty_check_receiver(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    // `(send $_ :empty?)`.
    if let Some(recv_src) = as_zero_arg_send(cx, node, "empty?") {
        return recv_src;
    }
    // `(send (send (send $_ :empty?) :!) :!)` — doubly-negated form.
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "!" || !cx.call_arguments(node).is_empty() {
        return None;
    };
    let inner_not = receiver.get()?;
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        ..
    } = *cx.kind(inner_not)
    else {
        return None;
    };
    if cx.symbol_str(inner_method) != "!" || !cx.call_arguments(inner_not).is_empty() {
        return None;
    }
    let inner = inner_recv.get()?;
    as_zero_arg_send(cx, inner, "empty?")?
}

/// `<receiver>.blank?`, or bare `blank?` when the receiver has no source.
fn blank_for(recv: Option<String>) -> String {
    match recv {
        Some(src) => format!("{src}.blank?"),
        None => "blank?".to_owned(),
    }
}

/// Upstream `defining_blank?` (`(def :blank? (args) ...)`): skip `!present?`
/// in the body of a `blank?` definition. Walks up through `begin` wrappers.
fn inside_def_blank(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut current = cx.parent(node).get();
    while let Some(id) = current {
        match *cx.kind(id) {
            NodeKind::Begin(_) => {
                current = cx.parent(id).get();
            }
            NodeKind::Def { name, .. } => {
                return cx.symbol_str(name) == "blank?";
            }
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::Blank;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_nil_or_empty() {
        test::<Blank>().expect_offense(indoc! {r#"
            foo.nil? || foo.empty?
            ^^^^^^^^^^^^^^^^^^^^^^ Use `foo.blank?` instead of `foo.nil? || foo.empty?`.
        "#});
    }

    #[test]
    fn flags_eq_nil_or_empty() {
        test::<Blank>().expect_offense(indoc! {r#"
            foo == nil || foo.empty?
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `foo.blank?` instead of `foo == nil || foo.empty?`.
        "#});
    }

    #[test]
    fn flags_nil_eq_or_empty() {
        test::<Blank>().expect_offense(indoc! {r#"
            nil == foo || foo.empty?
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `foo.blank?` instead of `nil == foo || foo.empty?`.
        "#});
    }

    #[test]
    fn flags_bang_or_empty() {
        test::<Blank>().expect_offense(indoc! {r#"
            !foo || foo.empty?
            ^^^^^^^^^^^^^^^^^^ Use `foo.blank?` instead of `!foo || foo.empty?`.
        "#});
    }

    #[test]
    fn flags_not_present() {
        test::<Blank>().expect_offense(indoc! {r#"
            !foo.present?
            ^^^^^^^^^^^^^ Use `foo.blank?` instead of `!foo.present?`.
        "#});
    }

    #[test]
    fn does_not_flag_mismatched_receivers() {
        test::<Blank>().expect_no_offenses("foo.nil? || bar.empty?\n");
    }

    #[test]
    fn does_not_flag_bare_present_negation() {
        test::<Blank>().expect_no_offenses("!foo\n");
    }

    #[test]
    fn does_not_flag_inside_def_blank() {
        test::<Blank>().expect_no_offenses("def blank?\n  !present?\nend\n");
    }

    #[test]
    fn corrects_nil_or_empty() {
        test::<Blank>()
            .expect_correction(
                indoc! {r#"
                    foo.nil? || foo.empty?
                    ^^^^^^^^^^^^^^^^^^^^^^ Use `foo.blank?` instead of `foo.nil? || foo.empty?`.
                "#},
                "foo.blank?\n",
            )
            .expect_no_offenses("foo.blank?\n");
    }

    #[test]
    fn corrects_not_present() {
        test::<Blank>()
            .expect_correction(
                indoc! {r#"
                    !foo.present?
                    ^^^^^^^^^^^^^ Use `foo.blank?` instead of `!foo.present?`.
                "#},
                "foo.blank?\n",
            )
            .expect_no_offenses("foo.blank?\n");
    }
}
murphy_plugin_api::submit_cop!(Blank);
