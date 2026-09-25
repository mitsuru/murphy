//! `Rails/RenderPlainText` — prefer `render plain:` over `render text:`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RenderPlainText
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:render] with a bare
//!   receiver and a single hash argument containing a sym `:text` pair.
//!   `ContentTypeCompatibility` (default true) gates the no-content_type
//!   case; explicit `content_type: 'text/plain'` (str only) always flags.
//!   Whole-node offense with `render plain:` replacement preserving rest
//!   options.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RenderPlainText;

#[derive(CopOptions)]
pub struct RenderPlainTextOptions {
    #[option(
        name = "ContentTypeCompatibility",
        default = true,
        description = "Whether to allow `render text:` without content_type for compatibility."
    )]
    pub content_type_compatibility: bool,
}

#[cop(
    name = "Rails/RenderPlainText",
    description = "Prefer `render plain:` over `render text:`.",
    default_severity = "warning",
    default_enabled = false,
    options = RenderPlainTextOptions,
)]
impl RenderPlainText {
    // Mirrors upstream `RESTRICT_ON_SEND`.
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
    // Upstream pattern `(send nil? :render $(hash ...))` — exactly one hash arg.
    if args.len() != 1 {
        return;
    }
    if !matches!(*cx.kind(args[0]), NodeKind::Hash(_)) {
        return;
    }
    let hash = args[0];
    let NodeKind::Hash(list) = *cx.kind(hash) else {
        return;
    };
    let pairs = cx.list(list);
    let mut text_pair = None;
    for &p in pairs {
        if !matches!(*cx.kind(p), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key, .. } = *cx.kind(p) else {
            continue;
        };
        if matches!(*cx.kind(key), NodeKind::Sym(s) if cx.symbol_str(s) == "text") {
            text_pair = Some(p);
            break;
        }
    }
    let Some(text_pair_id) = text_pair else {
        return;
    };
    let NodeKind::Pair { value: text_value, .. } = *cx.kind(text_pair_id) else {
        return;
    };
    let content_type_pair = find_content_type(cx, pairs);
    if !compatible_content_type(cx, content_type_pair) {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Prefer `render plain:` over `render text:`.",
        None,
    );
    let text_src = cx.raw_source(cx.range(text_value)).to_owned();
    let mut rest = Vec::new();
    for &p in pairs {
        if p == text_pair_id {
            continue;
        }
        if Some(p) == content_type_pair {
            continue;
        }
        rest.push(cx.raw_source(cx.range(p)).to_owned());
    }
    let replacement = if rest.is_empty() {
        format!("render plain: {text_src}")
    } else {
        format!("render plain: {text_src}, {}", rest.join(", "))
    };
    cx.emit_edit(cx.range(node), &replacement);
}

fn find_content_type(cx: &Cx<'_>, pairs: &[NodeId]) -> Option<NodeId> {
    for &p in pairs {
        let NodeKind::Pair { key, .. } = *cx.kind(p) else {
            continue;
        };
        let is_content_type = match *cx.kind(key) {
            NodeKind::Sym(s) if cx.symbol_str(s) == "content_type" => true,
            NodeKind::Str(id) if cx.string_str(id) == "content_type" => true,
            _ => false,
        };
        if is_content_type {
            return Some(p);
        }
    }
    None
}

fn compatible_content_type(cx: &Cx<'_>, pair: Option<NodeId>) -> bool {
    let Some(pair_id) = pair else {
        // Upstream: `nil` => `!cop_config['ContentTypeCompatibility']`.
        let opts = cx.options_or_default::<RenderPlainTextOptions>();
        return !opts.content_type_compatibility;
    };
    let NodeKind::Pair { value, .. } = *cx.kind(pair_id) else {
        return false;
    };
    // Upstream: `value.respond_to?(:value)` then `== 'text/plain'`.
    // Only str values can equal 'text/plain'; const/sym/dstr never match.
    if let NodeKind::Str(id) = *cx.kind(value) {
        return cx.string_str(id) == "text/plain";
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{RenderPlainText, RenderPlainTextOptions};
    use murphy_plugin_api::test_support::{indoc, test, test_with_options};

    #[test]
    fn flags_text_with_plain_content_type_default() {
        test::<RenderPlainText>().expect_correction(
            indoc! {r#"
                render text: 'Ruby!', content_type: 'text/plain'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `render plain:` over `render text:`.
            "#},
            "render plain: 'Ruby!'\n",
        );
    }

    #[test]
    fn allows_text_with_html_content_type() {
        test::<RenderPlainText>()
            .expect_no_offenses("render text: 'Ruby!', content_type: 'text/html'\n");
    }

    #[test]
    fn allows_text_with_const_content_type() {
        test::<RenderPlainText>()
            .expect_no_offenses("render text: 'Ruby!', content_type: Foo\n");
    }

    #[test]
    fn allows_plain() {
        test::<RenderPlainText>().expect_no_offenses("render plain: 'Ruby!'\n");
    }

    #[test]
    fn allows_bare_text_with_default_compatibility() {
        test::<RenderPlainText>().expect_no_offenses("render text: 'Ruby!'\n");
    }

    #[test]
    fn flags_bare_text_when_compatibility_false() {
        let opts = RenderPlainTextOptions {
            content_type_compatibility: false,
        };
        test_with_options::<RenderPlainText>(&opts).expect_correction(
            indoc! {r#"
                render text: 'Ruby!'
                ^^^^^^^^^^^^^^^^^^^^ Prefer `render plain:` over `render text:`.
            "#},
            "render plain: 'Ruby!'\n",
        );
    }

    #[test]
    fn flags_with_plain_content_type_when_compatibility_false() {
        let opts = RenderPlainTextOptions {
            content_type_compatibility: false,
        };
        test_with_options::<RenderPlainText>(&opts).expect_correction(
            indoc! {r#"
                render text: 'Ruby!', content_type: 'text/plain'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `render plain:` over `render text:`.
            "#},
            "render plain: 'Ruby!'\n",
        );
    }

    #[test]
    fn preserves_rest_options() {
        test::<RenderPlainText>().expect_correction(
            indoc! {r#"
                render text: 'x', content_type: 'text/plain', status: 200
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `render plain:` over `render text:`.
            "#},
            "render plain: 'x', status: 200\n",
        );
    }

    #[test]
    fn allows_receiver() {
        test::<RenderPlainText>().expect_no_offenses("obj.render text: 'x', content_type: 'text/plain'\n");
    }

    #[test]
    fn allows_str_text_key() {
        test::<RenderPlainText>()
            .expect_no_offenses("render 'text' => 'x', content_type: 'text/plain'\n");
    }
}
murphy_plugin_api::submit_cop!(RenderPlainText);
