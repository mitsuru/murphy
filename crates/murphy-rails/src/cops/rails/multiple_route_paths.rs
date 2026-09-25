//! `Rails/MultipleRoutePaths` — use separate routes for multiple paths.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/MultipleRoutePaths
//! upstream_version_checked: 2.35.0
//! version_added: "2.29"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND HTTP_METHODS
//!   [get post put patch delete], within_routes? (ancestor Block/Numblock/
//!   Itblock whose call is `draw` on a `routes` receiver), route-path
//!   counting that rejects Array/Hash args (Kwsplat-wrapped options arrive
//!   as Hash so `**options` is preserved as `rest`), whole-send offense,
//!   and the split-into-multiple-routes autocorrect with column-based
//!   indentation. Upstream Include (config/routes.rb) absent.
//! ```
//!
//! Checks for mapping a route with multiple paths, which is deprecated
//! and will be removed in Rails 8.1.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleRoutePaths;

#[cop(
    name = "Rails/MultipleRoutePaths",
    description = "Use separate routes instead of combining multiple route paths in a single route.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MultipleRoutePaths {
    #[on_node(kind = "send", methods = ["get", "post", "put", "patch", "delete"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !HTTP_METHODS.contains(&method.as_str()) {
        return;
    }
    if !within_routes(node, cx) {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    // Upstream rejects Array/Hash args; Kwsplat alone (if ever unwrapped)
    // is also an option carrier, not a path.
    let mut paths: Vec<NodeId> = Vec::new();
    for &a in &args {
        match *cx.kind(a) {
            NodeKind::Array(_) | NodeKind::Hash(_) => continue,
            NodeKind::Kwsplat(_) => continue,
            _ => paths.push(a),
        }
    }
    if paths.len() < 2 {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Use separate routes instead of combining multiple route paths in a single route.",
        None,
    );
    let correction = build_correction(cx, node, &method, &paths);
    cx.emit_edit(cx.range(node), &correction);
}

fn build_correction(cx: &Cx<'_>, node: NodeId, method: &str, paths: &[NodeId]) -> String {
    let node_range = cx.range(node);
    let last_path = *paths.last().unwrap();
    let last_end = cx.range(last_path).end;
    let rest_range = murphy_plugin_api::Range {
        start: last_end,
        end: node_range.end,
    };
    let rest = cx.raw_source(rest_range).to_owned();
    let indent = indentation_of(cx, node_range.start);
    let mut lines: Vec<String> = Vec::with_capacity(paths.len());
    for &p in paths {
        let src = cx.raw_source(cx.range(p));
        lines.push(format!("{method} {src}{rest}"));
    }
    lines.join(&format!("\n{indent}"))
}

fn indentation_of(cx: &Cx<'_>, offset: u32) -> String {
    let bytes = cx.source().as_bytes();
    let off = offset as usize;
    let mut line_start = off;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    let mut indent = String::new();
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        indent.push(bytes[i] as char);
        i += 1;
    }
    indent
}

/// Upstream `within_routes?` — ancestor block whose call is
/// `(send (send _ :routes) :draw)`.
fn within_routes(node: NodeId, cx: &Cx<'_>) -> bool {
    for anc in cx.ancestors(node) {
        let call = match *cx.kind(anc) {
            NodeKind::Block { call, .. } => Some(call),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
            _ => None,
        };
        let Some(call_id) = call else {
            continue;
        };
        if cx.method_name(call_id) != Some("draw") {
            continue;
        }
        let Some(recv) = cx.call_receiver(call_id).get() else {
            continue;
        };
        if cx.method_name(recv) != Some("routes") {
            continue;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::MultipleRoutePaths;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiple_strings() {
        test::<MultipleRoutePaths>().expect_correction(
            indoc! {r#"
                Rails.application.routes.draw do
                  get '/users', '/other_path/users', '/another_path/users'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use separate routes instead of combining multiple route paths in a single route.
                end
            "#},
            "Rails.application.routes.draw do\n  get '/users'\n  get '/other_path/users'\n  get '/another_path/users'\nend\n",
        );
    }

    #[test]
    fn flags_multiple_with_option() {
        test::<MultipleRoutePaths>().expect_correction(
            indoc! {r#"
                Rails.application.routes.draw do
                  get '/users', '/other_path/users', '/another_path/users', to: 'users#index'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use separate routes instead of combining multiple route paths in a single route.
                end
            "#},
            "Rails.application.routes.draw do\n  get '/users', to: 'users#index'\n  get '/other_path/users', to: 'users#index'\n  get '/another_path/users', to: 'users#index'\nend\n",
        );
    }

    #[test]
    fn flags_multiple_with_kwsplat() {
        test::<MultipleRoutePaths>().expect_correction(
            indoc! {r#"
                Rails.application.routes.draw do
                  get '/users', '/other_path/users', '/another_path/users', **options
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use separate routes instead of combining multiple route paths in a single route.
                end
            "#},
            "Rails.application.routes.draw do\n  get '/users', **options\n  get '/other_path/users', **options\n  get '/another_path/users', **options\nend\n",
        );
    }

    #[test]
    fn flags_multiple_symbols() {
        test::<MultipleRoutePaths>().expect_correction(
            indoc! {r#"
                Rails.application.routes.draw do
                  get :resend, :generate_new_password
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use separate routes instead of combining multiple route paths in a single route.
                end
            "#},
            "Rails.application.routes.draw do\n  get :resend\n  get :generate_new_password\nend\n",
        );
    }

    #[test]
    fn allows_single_path() {
        test::<MultipleRoutePaths>().expect_no_offenses(indoc! {r#"
            Rails.application.routes.draw do
              get '/users'
              get '/other_path/users'
              get '/another_path/users'
            end
        "#});
    }

    #[test]
    fn allows_single_with_option() {
        test::<MultipleRoutePaths>().expect_no_offenses(indoc! {r#"
            Rails.application.routes.draw do
              get '/users', to: 'users#index'
              get '/other_path/users', to: 'users#index'
              get '/another_path/users', to: 'users#index'
            end
        "#});
    }

    #[test]
    fn allows_array_literal() {
        test::<MultipleRoutePaths>().expect_no_offenses(indoc! {r#"
            Rails.application.routes.draw do
              get '/other_path/users', []
            end
        "#});
    }

    #[test]
    fn allows_no_arguments() {
        test::<MultipleRoutePaths>().expect_no_offenses(indoc! {r#"
            Rails.application.routes.draw do
              get
            end
        "#});
    }

    #[test]
    fn allows_outside_routes() {
        test::<MultipleRoutePaths>()
            .expect_no_offenses("get '/users', '/other_path/users', '/another_path/users'\n");
    }
}
murphy_plugin_api::submit_cop!(MultipleRoutePaths);
