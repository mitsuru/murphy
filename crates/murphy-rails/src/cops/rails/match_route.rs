//! `Rails/MatchRoute` — use specific HTTP method instead of `match`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/MatchRoute
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:match] with bare
//!   receiver, within_routes? (ancestor Block/Numblock/Itblock whose call
//!   is `draw` on a `routes` receiver), path/hash argument shapes
//!   (single path, path+hash, single hash shorthand), via extraction
//!   (missing → get, sym/str single, single-element array; variables and
//!   multi-element arrays suppress), HTTP_METHODS gating, whole-send
//!   offense, and via-stripping replacement. Upstream Include
//!   (`**/config/routes.rb`, `**/config/routes/**/*.rb`) is enforced via
//!   the murphy-rails pack default.yml (engine `cop_applies_to_file`
//!   gate, verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! Identifies places where defining routes with `match` can be replaced
//! with a specific HTTP method.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MatchRoute;

#[cop(
    name = "Rails/MatchRoute",
    description = "Don't use `match` to define routes; use a specific HTTP method.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MatchRoute {
    #[on_node(kind = "send", methods = ["match"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("match") {
        return;
    }
    // Upstream `(send nil? :match ...)` — bare call only.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    if !within_routes(node, cx) {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    // Shapes: [path] / [path, hash] / [hash-shorthand].
    let (path_node, hash_opt) = match args.as_slice() {
        [path] => {
            if matches!(*cx.kind(*path), NodeKind::Hash(_)) {
                (*path, Some(*path))
            } else {
                (*path, None)
            }
        }
        [path, hash] => {
            if !matches!(*cx.kind(*hash), NodeKind::Hash(_)) {
                return;
            }
            (*path, Some(*hash))
        }
        _ => return,
    };
    let is_shorthand =
        matches!(*cx.kind(path_node), NodeKind::Hash(_)) && hash_opt == Some(path_node);
    match hash_opt {
        None => emit(cx, node, path_node, None, "get", false),
        Some(hash) => {
            let http = if has_via_pair(cx, hash) {
                match single_http_method(cx, hash) {
                    Some(m) => m,
                    None => return,
                }
            } else {
                "get".to_owned()
            };
            emit(cx, node, path_node, hash_opt, &http, is_shorthand);
        }
    }
}

fn emit(
    cx: &Cx<'_>,
    node: NodeId,
    path_node: NodeId,
    hash_opt: Option<NodeId>,
    http: &str,
    is_shorthand: bool,
) {
    let msg = format!("Use `{http}` instead of `match` to define a route.");
    cx.emit_offense(cx.range(node), &msg, None);
    let replacement = build_replacement(cx, path_node, hash_opt, http, is_shorthand);
    cx.emit_edit(cx.range(node), &replacement);
}

fn build_replacement(
    cx: &Cx<'_>,
    path_node: NodeId,
    hash_opt: Option<NodeId>,
    http: &str,
    is_shorthand: bool,
) -> String {
    if is_shorthand {
        let hash = hash_opt.unwrap();
        let rest = pairs_without_via(cx, hash);
        let joined = rest
            .iter()
            .map(|&p| cx.raw_source(cx.range(p)).to_owned())
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{http} {joined}");
    }
    match hash_opt {
        None => {
            let path_src = cx.raw_source(cx.range(path_node));
            format!("{http} {path_src}")
        }
        Some(hash) => {
            let rest = pairs_without_via(cx, hash);
            let path_src = cx.raw_source(cx.range(path_node));
            if rest.is_empty() {
                format!("{http} {path_src}")
            } else {
                let joined = rest
                    .iter()
                    .map(|&p| cx.raw_source(cx.range(p)).to_owned())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{http} {path_src}, {joined}")
            }
        }
    }
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

fn hash_pairs(cx: &Cx<'_>, hash: NodeId) -> Vec<NodeId> {
    let NodeKind::Hash(list) = *cx.kind(hash) else {
        return Vec::new();
    };
    cx.list(list).to_vec()
}

fn via_pair_id(cx: &Cx<'_>, hash: NodeId) -> Option<NodeId> {
    for &p in &hash_pairs(cx, hash) {
        let NodeKind::Pair { key, .. } = *cx.kind(p) else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(key)
            && cx.symbol_str(sym) == "via"
        {
            return Some(p);
        }
    }
    None
}

fn has_via_pair(cx: &Cx<'_>, hash: NodeId) -> bool {
    via_pair_id(cx, hash).is_some()
}

fn via_value_strings(cx: &Cx<'_>, hash: NodeId) -> Option<Option<Vec<String>>> {
    // Returns None when no via pair (caller defaults to get),
    // Some(None) when via is non-literal (suppress),
    // Some(Some(values)) otherwise.
    let pair = via_pair_id(cx, hash)?;
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return Some(None);
    };
    match *cx.kind(value) {
        NodeKind::Sym(sym) => Some(Some(vec![cx.symbol_str(sym).to_owned()])),
        NodeKind::Str(sid) => Some(Some(vec![cx.string_str(sid).to_owned()])),
        NodeKind::Array(list) => {
            let mut out = Vec::new();
            for &e in cx.list(list) {
                match *cx.kind(e) {
                    NodeKind::Sym(sym) => out.push(cx.symbol_str(sym).to_owned()),
                    NodeKind::Str(sid) => out.push(cx.string_str(sid).to_owned()),
                    _ => return Some(None),
                }
            }
            Some(Some(out))
        }
        _ => Some(None),
    }
}

fn single_http_method(cx: &Cx<'_>, hash: NodeId) -> Option<String> {
    match via_value_strings(cx, hash) {
        Some(Some(v)) if v.len() == 1 && HTTP_METHODS.contains(&v[0].to_ascii_lowercase().as_str()) => {
            Some(v[0].to_ascii_lowercase())
        }
        _ => None,
    }
}

fn pairs_without_via(cx: &Cx<'_>, hash: NodeId) -> Vec<NodeId> {
    let via = via_pair_id(cx, hash);
    hash_pairs(cx, hash)
        .into_iter()
        .filter(|&p| Some(p) != via)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::MatchRoute;
    use murphy_plugin_api::test_support::{indoc, test};

    fn routes(src: &str) -> String {
        format!("routes.draw do\n{src}\nend\n")
    }

    #[test]
    fn flags_bare_match() {
        test::<MatchRoute>().expect_correction(
            indoc! {r#"
                routes.draw do
                  match ':controller/:action/:id'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `get` instead of `match` to define a route.
                end
            "#},
            "routes.draw do\n  get ':controller/:action/:id'\nend\n",
        );
    }

    #[test]
    fn flags_single_via() {
        test::<MatchRoute>().expect_correction(
            indoc! {r#"
                routes.draw do
                  match 'photos/:id', to: 'photos#show', via: :get
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `get` instead of `match` to define a route.
                end
            "#},
            "routes.draw do\n  get 'photos/:id', to: 'photos#show'\nend\n",
        );
    }

    #[test]
    fn flags_single_via_array() {
        test::<MatchRoute>().expect_correction(
            indoc! {r#"
                routes.draw do
                  match 'photos/:id', to: 'photos#show', via: [:get]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `get` instead of `match` to define a route.
                end
            "#},
            "routes.draw do\n  get 'photos/:id', to: 'photos#show'\nend\n",
        );
    }

    #[test]
    fn flags_hash_shorthand() {
        test::<MatchRoute>().expect_correction(
            indoc! {r#"
                routes.draw do
                  match 'photos/:id' => 'photos#show', via: :get
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `get` instead of `match` to define a route.
                end
            "#},
            "routes.draw do\n  get 'photos/:id' => 'photos#show'\nend\n",
        );
    }

    #[test]
    fn flags_interpolation_with_put() {
        test::<MatchRoute>().expect_correction(
            indoc! {r##"
                routes.draw do
                  match "#{resource}/:action/:id", via: [:put]
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `put` instead of `match` to define a route.
                end
            "##},
            "routes.draw do\n  put \"#{resource}/:action/:id\"\nend\n",
        );
    }

    #[test]
    fn allows_outside_routes() {
        test::<MatchRoute>()
            .expect_no_offenses("match 'photos/:id', to: 'photos#show', via: :get\n");
    }

    #[test]
    fn allows_via_all() {
        test::<MatchRoute>().expect_no_offenses(&routes(
            "  match 'photos/:id', to: 'photos#show', via: :all",
        ));
    }

    #[test]
    fn allows_multiple_verbs() {
        test::<MatchRoute>().expect_no_offenses(&routes(
            "  match 'photos/:id', to: 'photos#update', via: [:put, :patch]",
        ));
    }

    #[test]
    fn allows_get() {
        test::<MatchRoute>()
            .expect_no_offenses(&routes("  get 'photos/:id', to: 'photos#show'"));
    }

    #[test]
    fn allows_variable_via() {
        test::<MatchRoute>().expect_no_offenses(&routes(
            "  match ':controller/:action/:id', via: method",
        ));
    }

    #[test]
    fn allows_receiver_match() {
        test::<MatchRoute>()
            .expect_no_offenses(&routes("  foo.match 'photos/:id', to: 'x', via: :get"));
    }
}
murphy_plugin_api::submit_cop!(MatchRoute);
