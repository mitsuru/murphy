//! `Rails/RenderInline` — prefer template over inline rendering.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RenderInline
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:render] with a bare
//!   receiver and a hash argument containing an `inline` pair (sym `:inline`
//!   or str `"inline"`, any value). Whole-node offense, no autocorrect.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RenderInline;

#[cop(
    name = "Rails/RenderInline",
    description = "Prefer using a template over inline rendering.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RenderInline {
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
    let mut found = false;
    for &a in args {
        if !matches!(*cx.kind(a), NodeKind::Hash(_)) {
            continue;
        }
        if has_inline_pair(cx, a) {
            found = true;
            break;
        }
    }
    if !found {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Prefer using a template over inline rendering.",
        None,
    );
}

fn has_inline_pair(cx: &Cx<'_>, hash: NodeId) -> bool {
    let NodeKind::Hash(list) = *cx.kind(hash) else {
        return false;
    };
    for &pair in cx.list(list) {
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            continue;
        };
        match *cx.kind(key) {
            NodeKind::Sym(s) if cx.symbol_str(s) == "inline" => return true,
            NodeKind::Str(id) if cx.string_str(id) == "inline" => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::RenderInline;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_inline_sym_key() {
        test::<RenderInline>().expect_offense(indoc! {r#"
            render status: 200, inline: 'inline template'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using a template over inline rendering.
        "#});
    }

    #[test]
    fn flags_inline_str_key() {
        test::<RenderInline>().expect_offense(indoc! {r#"
            render status: 200, 'inline' => 'inline template'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer using a template over inline rendering.
        "#});
    }

    #[test]
    fn allows_template_render() {
        test::<RenderInline>().expect_no_offenses("render :index\n");
    }

    #[test]
    fn allows_other_options() {
        test::<RenderInline>()
            .expect_no_offenses("render json: users, serializer: UserSerializer\n");
    }

    #[test]
    fn allows_variable_key() {
        test::<RenderInline>().expect_no_offenses(indoc! {r#"
            serializer = users.respond_to?(:each) ? :each_serializer : :serializer
            render json: users, serializer => UserSerializer
        "#});
    }

    #[test]
    fn allows_receiver() {
        test::<RenderInline>()
            .expect_no_offenses("obj.render inline: 'x'\n");
    }
}
murphy_plugin_api::submit_cop!(RenderInline);
