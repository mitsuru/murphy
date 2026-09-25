//! `Rails/OutputSafety` — tagging a string as html safe may be a security risk.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/OutputSafety
//! upstream_version_checked: 2.35.0
//! version_added: "0.41"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [html_safe raw safe_concat] (send + csend), non-interpolated-string
//!   suppression (Str receiver, or Dstr whose parts are all Str),
//!   I18n descendant suppression (bare t/translate/l/localize or
//!   I18n/::I18n receiver anywhere inside the call), html_safe (receiver
//!   required, zero args), raw (no receiver, exactly one arg), safe_concat
//!   (any receiver, exactly one arg). Offense is the selector only; no
//!   autocorrect.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct OutputSafety;

#[cop(
    name = "Rails/OutputSafety",
    description = "The use of `html_safe` or `raw` may be a security risk.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl OutputSafety {
    #[on_node(kind = "send", methods = ["html_safe", "raw", "safe_concat"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "html_safe" && method != "raw" && method != "safe_concat" {
        return;
    }
    if non_interpolated_string(cx, node) {
        return;
    }
    if contains_i18n_call(cx, node) {
        return;
    }
    let looks_like = match method.as_str() {
        "html_safe" => {
            // receiver required, no args
            cx.call_receiver(node).get().is_some() && cx.call_arguments(node).is_empty()
        }
        "raw" => {
            // no receiver (command), exactly one arg
            cx.call_receiver(node).get().is_none() && cx.call_arguments(node).len() == 1
        }
        "safe_concat" => cx.call_arguments(node).len() == 1,
        _ => false,
    };
    if !looks_like {
        return;
    }
    cx.emit_offense(
        cx.loc(node).name,
        "Tagging a string as html safe may be a security risk.",
        None,
    );
}

fn non_interpolated_string(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    match *cx.kind(recv) {
        NodeKind::Str(_) => true,
        NodeKind::Dstr(list) => {
            // `receiver.dstr_type? && receiver.children.all?(&:str_type?)`
            // Heredoc static multiline is Str? Dynamic has non-Str parts.
            cx.list(list).iter().all(|&p| matches!(*cx.kind(p), NodeKind::Str(_)))
        }
        _ => false,
    }
}

fn contains_i18n_call(cx: &Cx<'_>, node: NodeId) -> bool {
    // `def_node_search :i18n_method?` — any descendant (including self)
    // matching `(send {nil? (const {nil? cbase} :I18n)} {:t :translate :l :localize} ...)`
    for desc in cx.descendants(node) {
        if is_i18n_call(cx, desc) {
            return true;
        }
    }
    false
}

fn is_i18n_call(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    let m = match cx.method_name(id) {
        Some(m) => m,
        None => return false,
    };
    if m != "t" && m != "translate" && m != "l" && m != "localize" {
        return false;
    }
    let recv_opt = cx.call_receiver(id).get();
    match recv_opt {
        None => true,
        Some(r) => {
            // `(const {nil? cbase} :I18n)` — bare I18n or ::I18n
            cx.const_name(r).as_deref() == Some("I18n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputSafety;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_safe_concat() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            foo.safe_concat('bar')
                ^^^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn flags_safe_concat_csend() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            foo&.safe_concat('bar')
                 ^^^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn allows_static_string_html_safe() {
        test::<OutputSafety>().expect_no_offenses("\"foo\".html_safe\n");
    }

    #[test]
    fn flags_dynamic_string_html_safe() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            "foo#{1}".html_safe
                      ^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn flags_variable_html_safe() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            foo.html_safe
                ^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn allows_html_safe_with_args() {
        test::<OutputSafety>().expect_no_offenses("foo.html_safe(one)\n");
    }

    #[test]
    fn allows_bare_html_safe() {
        test::<OutputSafety>().expect_no_offenses("html_safe\n");
    }

    #[test]
    fn flags_html_safe_csend() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            foo&.html_safe
                 ^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn flags_raw_with_var() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            raw(foo)
            ^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn flags_raw_with_literal() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            raw("foo")
            ^^^ Tagging a string as html safe may be a security risk.
        "#});
    }

    #[test]
    fn allows_raw_with_receiver() {
        test::<OutputSafety>().expect_no_offenses("foo.raw(foo)\n");
    }

    #[test]
    fn allows_raw_without_args() {
        test::<OutputSafety>().expect_no_offenses("raw\n");
    }

    #[test]
    fn allows_raw_with_two_args() {
        test::<OutputSafety>().expect_no_offenses("raw(one, two)\n");
    }

    #[test]
    fn allows_i18n_html_safe() {
        test::<OutputSafety>().expect_no_offenses("I18n.t('foo.bar.baz', scope: [:x, :y, :z]).html_safe\n");
    }

    #[test]
    fn allows_bare_t_html_safe() {
        test::<OutputSafety>().expect_no_offenses("t('foo.bar.baz').html_safe\n");
    }

    #[test]
    fn allows_i18n_l_html_safe() {
        test::<OutputSafety>().expect_no_offenses("I18n.l(Time.now, locale: :de).html_safe\n");
    }

    #[test]
    fn flags_safe_concat_inside_safe_join() {
        test::<OutputSafety>().expect_offense(indoc! {r#"
            safe_join([i18n_text.safe_concat(i18n_text)])
                                 ^^^^^^^^^^^ Tagging a string as html safe may be a security risk.
        "#});
    }
}
murphy_plugin_api::submit_cop!(OutputSafety);
