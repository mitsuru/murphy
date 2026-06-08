//! `Lint/UriEscapeUnescape` — Checks for calls to `URI.escape`, `URI.unescape`,
//! `URI.encode`, and `URI.decode` which are obsolete.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UriEscapeUnescape
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags all `URI.escape`, `URI.unescape`, `URI.encode`, `URI.decode` calls
//!   (including `::URI` cbase form). No autocorrect because the correct
//!   replacement depends on the specific use case and is not safe to automate.
//! ```
//!
//! ## Matched shapes
//!
//! - `URI.escape(...)` / `URI.encode(...)` — escape-family calls
//! - `URI.unescape(...)` / `URI.decode(...)` — unescape-family calls
//! - `::URI.escape(...)` etc. — cbase form
//!
//! ## No autocorrect
//!
//! The correct replacement (`CGI.escape`, `URI.encode_www_form`,
//! `URI.encode_www_form_component`, `URI.decode_www_form`, etc.)
//! depends on the specific use case and is not safe to automate.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const OBSOLETE_METHODS: &[&str] = &["escape", "unescape", "encode", "decode"];

const ESCAPE_REPLACEMENTS: &str =
    "`CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component`";

const UNESCAPE_REPLACEMENTS: &str =
    "`CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component`";

fn message(method: &str) -> String {
    let replacements = if method == "escape" || method == "encode" {
        ESCAPE_REPLACEMENTS
    } else {
        UNESCAPE_REPLACEMENTS
    };
    format!(
        "`URI.{method}` method is obsolete and should not be used. Instead, use {replacements} depending on your specific use case."
    )
}

#[derive(Default)]
pub struct UriEscapeUnescape;

#[cop(
    name = "Lint/UriEscapeUnescape",
    description = "Checks for calls to `URI.escape`, `URI.unescape`, `URI.encode`, and `URI.decode` which are obsolete.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UriEscapeUnescape {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else { return; };
        let method_str = cx.symbol_str(method);
        if !OBSOLETE_METHODS.contains(&method_str) {
            return;
        }
        let Some(receiver_id) = receiver.get() else { return; };
        let NodeKind::Const { scope, name } = *cx.kind(receiver_id) else { return; };
        if !scope.is_none() {
            return;
        }
        if cx.symbol_str(name) != "URI" {
            return;
        }
        cx.emit_offense(cx.range(node), &message(method_str), None);
    }
}

#[cfg(test)]
mod tests {
    use super::UriEscapeUnescape;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_uri_escape() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            URI.escape('http://example.com')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `URI.escape` method is obsolete and should not be used. Instead, use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_uri_escape_with_two_args() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            URI.escape('@?@!', '!?')
            ^^^^^^^^^^^^^^^^^^^^^^^^ `URI.escape` method is obsolete and should not be used. Instead, use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_cbase_uri_escape() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            ::URI.escape('http://example.com')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `URI.escape` method is obsolete and should not be used. Instead, use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_uri_encode() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            URI.encode('http://example.com')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `URI.encode` method is obsolete and should not be used. Instead, use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_cbase_uri_encode() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            ::URI.encode('http://example.com')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `URI.encode` method is obsolete and should not be used. Instead, use `CGI.escape`, `URI.encode_www_form` or `URI.encode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_uri_unescape() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            URI.unescape(enc_uri)
            ^^^^^^^^^^^^^^^^^^^^^ `URI.unescape` method is obsolete and should not be used. Instead, use `CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_cbase_uri_unescape() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            ::URI.unescape(enc_uri)
            ^^^^^^^^^^^^^^^^^^^^^^^ `URI.unescape` method is obsolete and should not be used. Instead, use `CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_uri_decode() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            URI.decode(enc_uri)
            ^^^^^^^^^^^^^^^^^^^ `URI.decode` method is obsolete and should not be used. Instead, use `CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component` depending on your specific use case.
        "#});
    }

    #[test]
    fn flags_cbase_uri_decode() {
        test::<UriEscapeUnescape>().expect_offense(indoc! {r#"
            ::URI.decode(enc_uri)
            ^^^^^^^^^^^^^^^^^^^^^ `URI.decode` method is obsolete and should not be used. Instead, use `CGI.unescape`, `URI.decode_www_form` or `URI.decode_www_form_component` depending on your specific use case.
        "#});
    }
}

murphy_plugin_api::submit_cop!(UriEscapeUnescape);
