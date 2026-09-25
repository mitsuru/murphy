//! `Rails/ResponseParsedBody` — prefer `response.parsed_body`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ResponseParsedBody
//! upstream_version_checked: 2.35.0
//! version_added: "2.18"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `JSON.parse(response.body)` (JSON /
//!   ::JSON, zero-arg `response` + zero-arg `body`) always flags on Rails
//!   >= 5.0 (unset means newest); Nokogiri `HTML/HTML4/HTML5` call,
//!   `.parse`, and `::Document.parse` shapes flag only on Rails >= 7.1.
//!   Whole-node offense with `response.parsed_body` replacement. Upstream
//!   Include (spec/controllers, spec/requests, test/controllers,
//!   test/integration) is enforced via the murphy-rails pack default.yml.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ResponseParsedBody;

#[cop(
    name = "Rails/ResponseParsedBody",
    description = "Prefer `response.parsed_body` to custom parsing logic for `response.body`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ResponseParsedBody {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `minimum_target_rails_version 5.0` — unset means newest.
    if !cx.rails_version_at_least(5, 0) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if is_json_parse_response_body(cx, node) || is_nokogiri_offense(cx, node) {
        cx.emit_offense(cx.range(node), "Prefer `response.parsed_body`.", None);
        cx.emit_edit(cx.range(node), "response.parsed_body");
    }
}

fn is_bare_response(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("response") {
        return false;
    }
    if cx.call_receiver(id).get().is_some() {
        return false;
    }
    cx.call_arguments(id).is_empty()
}

fn is_response_body(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("body") {
        return false;
    }
    if !cx.call_arguments(id).is_empty() {
        return false;
    }
    match cx.call_receiver(id).get() {
        Some(recv) => is_bare_response(cx, recv),
        None => false,
    }
}

fn is_json_const(cx: &Cx<'_>, id: NodeId) -> bool {
    cx.const_name(id).as_deref() == Some("JSON")
}

fn is_nokogiri_const(cx: &Cx<'_>, id: NodeId) -> bool {
    cx.const_name(id).as_deref() == Some("Nokogiri")
}

fn is_nokogiri_html_const(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        cx.const_name(id).as_deref(),
        Some("Nokogiri::HTML") | Some("Nokogiri::HTML4") | Some("Nokogiri::HTML5")
    )
}

fn is_json_parse_response_body(cx: &Cx<'_>, node: NodeId) -> bool {
    // `(send #json? :parse #response_body?)` — single response.body arg.
    if cx.method_name(node) != Some("parse") {
        return false;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 || !is_response_body(cx, args[0]) {
        return false;
    }
    match cx.call_receiver(node).get() {
        Some(recv) => is_json_const(cx, recv),
        None => false,
    }
}

fn is_nokogiri_offense(cx: &Cx<'_>, node: NodeId) -> bool {
    // HTML shapes require Rails >= 7.1.
    if !cx.rails_version_at_least(7, 1) {
        return false;
    }
    let method = cx.method_name(node);
    let args = cx.call_arguments(node);
    if args.len() != 1 || !is_response_body(cx, args[0]) {
        return false;
    }
    let recv = match cx.call_receiver(node).get() {
        Some(r) => r,
        None => return false,
    };
    // `Nokogiri::HTML(response.body)` — method HTML/HTML4/HTML5 on Nokogiri.
    if let Some(m) = method
        && matches!(m, "HTML" | "HTML4" | "HTML5")
        && is_nokogiri_const(cx, recv)
    {
        return true;
    }
    // `Nokogiri::HTML.parse(response.body)` — method parse on HTML const.
    if method == Some("parse") && is_nokogiri_html_const(cx, recv) {
        return true;
    }
    // `Nokogiri::HTML::Document.parse(response.body)` — method parse on
    // `Nokogiri::HTML::Document` const.
    if method == Some("parse") {
        match cx.const_name(recv).as_deref() {
            Some("Nokogiri::HTML::Document")
            | Some("Nokogiri::HTML4::Document")
            | Some("Nokogiri::HTML5::Document") => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ResponseParsedBody;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_json_parse() {
        test::<ResponseParsedBody>().expect_correction(
            indoc! {r#"
                expect(JSON.parse(response.body)).to eq('foo' => 'bar')
                       ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
            "#},
            "expect(response.parsed_body).to eq('foo' => 'bar')\n",
        );
    }

    #[test]
    fn flags_cbase_json_parse() {
        test::<ResponseParsedBody>().expect_correction(
            indoc! {r#"
                expect(::JSON.parse(response.body)).to eq('foo' => 'bar')
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
            "#},
            "expect(response.parsed_body).to eq('foo' => 'bar')\n",
        );
    }

    #[test]
    fn allows_parsed_body() {
        test::<ResponseParsedBody>()
            .expect_no_offenses("expect(response.parsed_body).to eq('foo' => 'bar')\n");
    }

    #[test]
    fn allows_below_rails_5() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("expect(JSON.parse(response.body)).to eq('foo' => 'bar')\n");
    }

    #[test]
    fn allows_nokogiri_on_rails_70() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(7, 0)
            .expect_no_offenses("Nokogiri::HTML(response.body)\n");
    }

    #[test]
    fn flags_nokogiri_html_on_rails_71() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(7, 1)
            .expect_correction(
                indoc! {r#"
                    Nokogiri::HTML(response.body)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
                "#},
                "response.parsed_body\n",
            );
    }

    #[test]
    fn flags_nokogiri_html4_on_rails_71() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(7, 1)
            .expect_correction(
                indoc! {r#"
                    Nokogiri::HTML4(response.body)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
                "#},
                "response.parsed_body\n",
            );
    }

    #[test]
    fn flags_nokogiri_parse_on_rails_71() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(7, 1)
            .expect_correction(
                indoc! {r#"
                    Nokogiri::HTML.parse(response.body)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
                "#},
                "response.parsed_body\n",
            );
    }

    #[test]
    fn flags_nokogiri_document_parse_on_rails_71() {
        test::<ResponseParsedBody>()
            .with_target_rails_version(7, 1)
            .expect_correction(
                indoc! {r#"
                    Nokogiri::HTML::Document.parse(response.body)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `response.parsed_body`.
                "#},
                "response.parsed_body\n",
            );
    }

    #[test]
    fn allows_other_parse() {
        test::<ResponseParsedBody>().expect_no_offenses("JSON.parse(other.body)\n");
    }
}
murphy_plugin_api::submit_cop!(ResponseParsedBody);
