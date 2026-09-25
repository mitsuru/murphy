//! `Rails/UnusedRenderContent` — flag body content with non-content status.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/UnusedRenderContent
//! upstream_version_checked: 2.35.0
//! version_added: "2.21"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND [:render]:
//!   bare `render` only. Two shapes: single hash arg containing both a
//!   non-content `status:` pair and a BODY_OPTIONS pair (offense on the body
//!   pair), or a positional Str/Sym plus a hash containing a non-content
//!   `status:` (offense on the positional). Non-content = Int 100,101,102,103,
//!   204,205,304 or Sym continue/switching_protocols/processing/early_hints/
//!   no_content/reset_content/not_modified (Rack::Utils::SYMBOL_TO_STATUS_CODE).
//!   No autocorrect. Disabled upstream by default (`Enabled: pending`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnusedRenderContent;

const MSG: &str =
    "Do not specify body content for a response with a non-content status code";

#[cop(
    name = "Rails/UnusedRenderContent",
    description = "Do not specify body content for a response with a non-content status code.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl UnusedRenderContent {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[render]`.
    #[on_node(kind = "send", methods = ["render"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() == 1 {
        // `(hash <#non_content_status? $(pair (sym BODY_OPTIONS) _) ...>)`
        let hash = args[0];
        if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
            return;
        }
        let pairs = cx.hash_pairs(hash);
        if !pairs.iter().any(|&p| is_non_content_status_pair(cx, p)) {
            return;
        }
        if let Some(body) = pairs.iter().copied().find(|&p| is_body_option_pair(cx, p)) {
            cx.emit_offense(cx.range(body), MSG, None);
        }
        return;
    }
    if args.len() == 2 {
        // `$({str sym} _) (hash <#non_content_status? ...>)`
        let first = args[0];
        let second = args[1];
        let is_str_sym = matches!(*cx.kind(first), NodeKind::Str(_) | NodeKind::Sym(_));
        if !is_str_sym {
            return;
        }
        if !matches!(*cx.kind(second), NodeKind::Hash(_)) {
            return;
        }
        let pairs = cx.hash_pairs(second);
        if !pairs.iter().any(|&p| is_non_content_status_pair(cx, p)) {
            return;
        }
        cx.emit_offense(cx.range(first), MSG, None);
    }
}

fn is_non_content_status_pair(cx: &Cx<'_>, pair: NodeId) -> bool {
    if !matches!(*cx.kind(pair), NodeKind::Pair { .. }) {
        return false;
    }
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    if !matches!(*cx.kind(key), NodeKind::Sym(s) if cx.symbol_str(s) == "status") {
        return false;
    }
    match *cx.kind(value) {
        NodeKind::Sym(s) => matches!(
            cx.symbol_str(s),
            "continue"
                | "switching_protocols"
                | "processing"
                | "early_hints"
                | "no_content"
                | "reset_content"
                | "not_modified"
        ),
        NodeKind::Int(n) => matches!(n, 100 | 101 | 102 | 103 | 204 | 205 | 304),
        _ => false,
    }
}

fn is_body_option_pair(cx: &Cx<'_>, pair: NodeId) -> bool {
    if !matches!(*cx.kind(pair), NodeKind::Pair { .. }) {
        return false;
    }
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return false;
    };
    if let NodeKind::Sym(s) = *cx.kind(key) {
        matches!(
            cx.symbol_str(s),
            "action"
                | "body"
                | "content_type"
                | "file"
                | "html"
                | "inline"
                | "json"
                | "js"
                | "layout"
                | "plain"
                | "raw"
                | "template"
                | "text"
                | "xml"
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::UnusedRenderContent;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn no_offense_body_with_ok_status() {
        test::<UnusedRenderContent>().expect_no_offenses("render status: :ok, plain: 'Ruby!'\n");
    }

    #[test]
    fn no_offense_no_body_with_non_content_status() {
        test::<UnusedRenderContent>().expect_no_offenses("render status: :no_content\n");
    }

    #[test]
    fn flags_positional_string_with_continue() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render 'foo', status: :continue
                   ^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_positional_symbol_multiline() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render(
              :foo,
              ^^^^ Do not specify body content for a response with a non-content status code
              status: :switching_protocols
            )
        "#});
    }

    #[test]
    fn flags_action_option_last() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: :processing, action: :foo
                                        ^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_body_option_first() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render body: 'foo', status: :early_hints
                   ^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_content_type_between_others() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: :no_content, content_type: 'foo', another: 'option'
                                        ^^^^^^^^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_file_option() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: :reset_content, file: 'foo'
                                           ^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_html_option() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: :not_modified, html: 'foo'
                                          ^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_inline_with_int_100() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 100, inline: 'foo'
                                ^^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_json_with_int_101() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 101, json: 'foo'
                                ^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_js_with_int_102() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 102, js: 'foo'
                                ^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_layout_with_int_103() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 103, layout: 'foo'
                                ^^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_plain_with_int_204() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 204, plain: 'foo'
                                ^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_raw_with_int_205() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 205, raw: 'foo'
                                ^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_template_with_int_304() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 304, template: 'foo'
                                ^^^^^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_text_with_int_304() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 304, text: 'foo'
                                ^^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn flags_xml_with_int_304() {
        test::<UnusedRenderContent>().expect_offense(indoc! {r#"
            render status: 304, xml: 'foo'
                                ^^^^^^^^^^ Do not specify body content for a response with a non-content status code
        "#});
    }

    #[test]
    fn no_offense_with_receiver() {
        test::<UnusedRenderContent>().expect_no_offenses("foo.render status: :continue, plain: 'x'\n");
    }
}

murphy_plugin_api::submit_cop!(UnusedRenderContent);
