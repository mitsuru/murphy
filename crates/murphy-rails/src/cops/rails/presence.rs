//! `Rails/Presence` — prefer `presence` / `presence || x` / `presence&.foo` over ternaries.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Presence
//! upstream_version_checked: 2.35.0
//! version_added: "0.52"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_if` (ternary and if/else forms):
//!   `a.present? ? a : b` / `a.blank? ? b : a` (plus `!`-negated
//!   conditions) → `a.presence || b` (bare `a.presence` when the other
//!   branch is nil), and `a.present? ? a.foo : nil` /
//!   `a.blank? ? nil : a.foo` → `a.presence&.foo`. `elsif` nodes,
//!   `if`/`rescue`/`while` other-branches, and assignment/operator-method
//!   chains are skipped. Multi-statement (`begin`) branches are skipped.
//!   Receiver equality uses source text (upstream uses structural node
//!   equality). The `||`-fallback parenthesization (send/and parents) and
//!   the unparenthesized-call `method(args)` rewrite are mirrored.
//!   Upstream Include gating absent.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Presence;

#[cop(
    name = "Rails/Presence",
    description = "Checks code that can be written more easily using `Object#presence` defined by Active Support.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Presence {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `ignore_if_node?`: skip `elsif`.
    if cx.is_elsif(node) {
        return;
    }
    let Some(cond) = cx.if_condition(node).get() else {
        return;
    };
    let (Some(then_), Some(else_)) = (cx.if_then_branch(node).get(), cx.if_else_branch(node).get()) else {
        return;
    };

    // Upstream tries `redundant_receiver_and_other` first, then `chain`.
    if let Some((receiver_src, other)) = match_other_form(cx, cond, then_, else_) {
        if other_is_ignorable(cx, other) {
            return;
        }
        let replacement = replacement(cx, node, &receiver_src, other);
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{}` instead of `{}`.", compact(&replacement), compact(cx.raw_source(cx.range(node)))),
            None,
        );
        cx.emit_edit(cx.range(node), &replacement);
        return;
    }

    if let Some((receiver_src, chain)) = match_chain_form(cx, cond, then_, else_) {
        if chain_is_ignorable(cx, chain) {
            return;
        }
        let replacement = chain_replacement(&receiver_src, cx, chain);
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{}` instead of `{}`.", compact(&replacement), compact(cx.raw_source(cx.range(node)))),
            None,
        );
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// Condition classes. `BlankCheck` matches `recv.blank?` / `!recv.present?`;
/// `PresentCheck` matches `recv.present?` / `!recv.blank?`.
enum CondKind {
    BlankCheck,
    PresentCheck,
}

fn classify_condition(cx: &Cx<'_>, cond: NodeId) -> Option<(CondKind, NodeId)> {
    // Direct `recv.blank?` / `recv.present?` (explicit receiver required).
    if matches!(*cx.kind(cond), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && cx.call_arguments(cond).is_empty()
    {
        let name = cx.method_name(cond)?;
        let recv = cx.call_receiver(cond).get()?;
        if name == "blank?" {
            return Some((CondKind::BlankCheck, recv));
        }
        if name == "present?" {
            return Some((CondKind::PresentCheck, recv));
        }
    }
    // `!inner` — negation of the opposite check.
    if matches!(*cx.kind(cond), NodeKind::Send { .. })
        && cx.method_name(cond) == Some("!")
        && cx.call_arguments(cond).is_empty()
    {
        let inner = cx.call_receiver(cond).get()?;
        if !matches!(*cx.kind(inner), NodeKind::Send { .. } | NodeKind::Csend { .. })
            || !cx.call_arguments(inner).is_empty()
        {
            return None;
        }
        let name = cx.method_name(inner)?;
        let recv = cx.call_receiver(inner).get()?;
        if name == "present?" {
            return Some((CondKind::BlankCheck, recv));
        }
        if name == "blank?" {
            return Some((CondKind::PresentCheck, recv));
        }
    }
    None
}

fn source_of(cx: &Cx<'_>, node: NodeId) -> String {
    cx.raw_source(cx.range(node)).to_owned()
}

/// Upstream `redundant_receiver_and_other`:
///
/// - blank-check with `(then=other non-begin, else==recv)`, or
/// - present-check with `(then==recv, else=other non-begin)`.
///
/// Returns the receiver source and the other branch.
fn match_other_form(cx: &Cx<'_>, cond: NodeId, then_: NodeId, else_: NodeId) -> Option<(String, NodeId)> {
    let (kind, recv) = classify_condition(cx, cond)?;
    let recv_src = source_of(cx, recv);
    match kind {
        CondKind::BlankCheck => {
            if is_begin(cx, else_) || source_of(cx, else_) != recv_src {
                return None;
            }
            if is_begin(cx, then_) {
                return None;
            }
            Some((recv_src, then_))
        }
        CondKind::PresentCheck => {
            if is_begin(cx, then_) || source_of(cx, then_) != recv_src {
                return None;
            }
            if is_begin(cx, else_) {
                return None;
            }
            Some((recv_src, else_))
        }
    }
}

/// Upstream `redundant_receiver_and_chain`:
///
/// - blank-check with `(then=nil, else=(send recv ...))`, or
/// - present-check with `(then=(send recv ...), else=nil)`.
fn match_chain_form(cx: &Cx<'_>, cond: NodeId, then_: NodeId, else_: NodeId) -> Option<(String, NodeId)> {
    let (kind, recv) = classify_condition(cx, cond)?;
    let recv_src = source_of(cx, recv);
    match kind {
        CondKind::BlankCheck => {
            if !is_nil(cx, then_) {
                return None;
            }
            if !is_send_to(cx, else_, &recv_src) {
                return None;
            }
            Some((recv_src, else_))
        }
        CondKind::PresentCheck => {
            if !is_send_to(cx, then_, &recv_src) {
                return None;
            }
            if !is_nil(cx, else_) {
                return None;
            }
            Some((recv_src, then_))
        }
    }
}

fn is_begin(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(*cx.kind(node), NodeKind::Begin(_) | NodeKind::Kwbegin(_))
}

fn is_nil(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(*cx.kind(node), NodeKind::Nil)
}

/// True if `node` is a `send`/`csend` whose receiver source is `recv_src`.
/// Bare `recv` itself (e.g. `a.present? ? a : nil`) is not a chain.
fn is_send_to(cx: &Cx<'_>, node: NodeId, recv_src: &str) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    source_of(cx, recv) == recv_src
}

/// Upstream `ignore_other_node?`: `if`/`rescue`/`while` branches cannot be
/// inlined behind `||`.
fn other_is_ignorable(cx: &Cx<'_>, other: NodeId) -> bool {
    matches!(
        *cx.kind(other),
        NodeKind::If { .. } | NodeKind::Rescue { .. } | NodeKind::While { .. }
    )
}

/// Upstream `ignore_chain_node?`: assignments and operator-method calls
/// cannot use `&.`.
fn chain_is_ignorable(cx: &Cx<'_>, chain: NodeId) -> bool {
    if !matches!(*cx.kind(chain), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    cx.is_assignment_method(chain) || cx.is_operator_method(chain)
}

/// Upstream `replacement`: `recv.presence`, plus the `|| other` fallback.
fn replacement(cx: &Cx<'_>, node: NodeId, receiver_src: &str, other: NodeId) -> String {
    let or_source = if matches!(*cx.kind(other), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        build_source_for_or_method(cx, other)
    } else if is_nil(cx, other) {
        String::new()
    } else {
        format!(" || {}", source_of(cx, other))
    };
    let replaced = format!("{receiver_src}.presence{or_source}");
    if require_parentheses(cx, node, &or_source) {
        format!("({replaced})")
    } else {
        replaced
    }
}

/// Upstream `require_parentheses?`: a trailing `|| other` needs parens when
/// the `if` sits as a bare argument of a call or under `&&`.
fn require_parentheses(cx: &Cx<'_>, node: NodeId, or_source: &str) -> bool {
    if or_source.is_empty() {
        return false;
    }
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if matches!(*cx.kind(parent), NodeKind::And { .. }) {
        return true;
    }
    if matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && !cx.is_parenthesized(parent)
    {
        return true;
    }
    false
}

/// Upstream `build_source_for_or_method`: parenthesized, `[]`, arithmetic,
/// or argless calls inline as-is; otherwise the call is re-parenthesized
/// as `method(args)`.
fn build_source_for_or_method(cx: &Cx<'_>, other: NodeId) -> String {
    let method = cx.method_name(other).unwrap_or("").to_owned();
    let args = cx.call_arguments(other);
    if cx.is_parenthesized(other) || method == "[]" || is_arithmetic(&method) || args.is_empty() {
        return format!(" || {}", source_of(cx, other));
    }
    // `method_range`: node start through the end of the method name, i.e.
    // the call prefix before its arguments (includes the receiver).
    let prefix = cx
        .raw_source(murphy_plugin_api::Range {
            start: cx.range(other).start,
            end: cx.range(args[0]).start,
        })
        .trim_end()
        .to_owned();
    let arg_sources: Vec<String> = args.iter().map(|&a| source_of(cx, a)).collect();
    format!(" || {}({})", prefix, arg_sources.join(", "))
}

fn is_arithmetic(method: &str) -> bool {
    matches!(method, "+" | "-" | "*" | "/" | "%" | "**")
}

/// Upstream `chain_replacement`: `recv.presence&.method(args?)`.
fn chain_replacement(receiver_src: &str, cx: &Cx<'_>, chain: NodeId) -> String {
    let method = cx.method_name(chain).unwrap_or("").to_owned();
    let args = cx.call_arguments(chain);
    if args.is_empty() {
        format!("{receiver_src}.presence&.{method}")
    } else {
        let arg_sources: Vec<String> = args.iter().map(|&a| source_of(cx, a)).collect();
        format!("{receiver_src}.presence&.{method}({})", arg_sources.join(", "))
    }
}

/// Upstream message rendering collapses newlines (and shortens multi-line
/// non-ternary nodes to `keyword ... end`).
fn compact(src: &str) -> String {
    if src.contains('\n') {
        src.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        src.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::Presence;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_present_ternary_to_presence() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.present? ? a : nil
                ^^^^^^^^^^^^^^^^^^^^ Use `a.presence` instead of `a.present? ? a : nil`.
            "#},
            "a.presence\n",
        );
    }

    #[test]
    fn flags_blank_ternary_to_presence() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.blank? ? nil : a
                ^^^^^^^^^^^^^^^^^^ Use `a.presence` instead of `a.blank? ? nil : a`.
            "#},
            "a.presence\n",
        );
    }

    #[test]
    fn flags_negated_present_to_presence() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                !a.present? ? nil : a
                ^^^^^^^^^^^^^^^^^^^^^ Use `a.presence` instead of `!a.present? ? nil : a`.
            "#},
            "a.presence\n",
        );
    }

    #[test]
    fn flags_present_with_fallback() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.present? ? a : b
                ^^^^^^^^^^^^^^^^^^ Use `a.presence || b` instead of `a.present? ? a : b`.
            "#},
            "a.presence || b\n",
        );
    }

    #[test]
    fn flags_blank_with_fallback() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.blank? ? b : a
                ^^^^^^^^^^^^^^^^ Use `a.presence || b` instead of `a.blank? ? b : a`.
            "#},
            "a.presence || b\n",
        );
    }

    #[test]
    fn flags_chain_to_safe_navigation() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.present? ? a.foo : nil
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `a.presence&.foo` instead of `a.present? ? a.foo : nil`.
            "#},
            "a.presence&.foo\n",
        );
    }

    #[test]
    fn flags_blank_chain_to_safe_navigation() {
        test::<Presence>().expect_correction(
            indoc! {r#"
                a.blank? ? nil : a.foo(1)
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `a.presence&.foo(1)` instead of `a.blank? ? nil : a.foo(1)`.
            "#},
            "a.presence&.foo(1)\n",
        );
    }

    #[test]
    fn allows_unrelated_branches() {
        test::<Presence>().expect_no_offenses("a.present? ? b : c\n");
    }

    #[test]
    fn allows_operator_chain() {
        test::<Presence>().expect_no_offenses("a.present? ? a + 1 : nil\n");
    }

    #[test]
    fn allows_elsif() {
        test::<Presence>().expect_no_offenses(indoc! {r#"
            if x
              foo
            elsif a.present?
              a
            else
              b
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(Presence);
