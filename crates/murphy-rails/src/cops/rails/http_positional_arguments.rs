//! `Rails/HttpPositionalArguments` — use keyword arguments for http calls
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HttpPositionalArguments
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Send-dispatch port of RuboCop's on_send (RESTRICT_ON_SEND get/post/put/
//!   patch/delete/head, bare receiver only, >= 2 args). needs_conversion?,
//!   routing-block (draw/routes ancestors), Rack::Test::Methods file guard,
//!   TargetRailsVersion >= 5.0 gating, highlight range (second arg to last),
//!   and params/session autocorrect are implemented. Include path gating
//!   (**/spec/**, **/test/**) is not implemented; Murphy flags in all files.
//!   Middle data args (>= 4 call args) are dropped like upstream (first to
//!   params, last to session).
//! ```
//!
//! Rails 5 changed test http helpers (`get`, `post`, `put`, `patch`,
//! `delete`, `head`) from positional `get :new, {user: 1}, {session}`
//! to keyword `get :new, params: {...}, session: {...}`. This cop flags
//! the old positional form and autocorrects it.
//!
//! ## Matched shape (Send nodes only)
//!
//! `Send(receiver=None, method in {get post put patch delete head}, args=[path, data, ...])`
//!
//! - **bare call only** — receiver must be `None` (implicit self). `@user.get`
//!   or `foo.get` never match (mirrors upstream `(send nil? ...)`).
//! - **>= 2 args** — path/action plus at least one data arg. `get :new`
//!   alone never matches (mirrors `(send nil? {verbs} !nil? $_ ...)`).
//! - **needs_conversion?(second arg)** — the second arg decides:
//!   - `ForwardedArgs` / `Unknown` (`...` forwarding) → no offense.
//!   - non-`Hash` (lvar, send, splat, str, nil, ...) → offense.
//!   - `Hash` containing any `Kwsplat` / `Kwnilarg` (`**opts`, `**nil`,
//!     mixed kwargs) → no offense.
//!   - `Hash` of only `Pair`s: no offense if any pair key is a special
//!     keyword (`method params session body flash xhr as headers env to`)
//!     or if the hash is a single `format:` pair; otherwise offense.
//!     Empty `{}` counts as offense (correction drops it).
//!
//! ## Skips
//!
//! - `TargetRailsVersion < 5.0` (unset means newest, flags).
//! - Inside a `draw` / `routes` block ancestor (routing DSL).
//! - Anywhere in a file containing `include Rack::Test::Methods`
//!   (bare or `::`-prefixed; both fold to `Const{scope: None}`).
//!
//! ## Offense range and message
//!
//! From the second arg start to the last arg end
//! (mirrors `highlight_range`), message
//! ``Use keyword arguments instead of positional arguments for http call: `<verb>`.``
//!
//! ## Autocorrect
//!
//! Replace the whole call with `verb action, params: {...}, session: {...}`
//! (parens preserved via `is_parenthesized`), where the first data arg maps
//! to `params:` and the last (when >= 2 data args) maps to `session:`.
//! Empty hashes map to nothing (dropped). Non-hash data maps to
//! `, params: <source>` / `, session: <source>` without braces.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Keywords that already mark the new kwargs style (upstream `KEYWORD_ARGS`).
const KEYWORD_ARGS: &[&str] = &[
    "method", "params", "session", "body", "flash", "xhr", "as", "headers", "env", "to",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HttpPositionalArguments;

#[cop(
    name = "Rails/HttpPositionalArguments",
    description = "Use keyword arguments instead of positional arguments in http method calls.",
    default_enabled = true,
    options = NoOptions,
)]
impl HttpPositionalArguments {
    // Mirrors upstream `RESTRICT_ON_SEND` — dispatch only on the six http verbs.
    #[on_node(kind = "send", methods = ["get", "post", "put", "patch", "delete", "head"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Rails >= 5 only. Unset means newest, so flag when unset.
    if !cx.rails_version_at_least(5, 0) {
        return;
    }

    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return;
    };
    // Bare call only (upstream `nil?` receiver).
    if receiver.get().is_some() {
        return;
    }
    let verb = cx.symbol_str(method).to_owned();

    let args = cx.call_arguments(node);
    // Path plus at least one data arg (upstream `!nil? $_ ...`).
    if args.len() < 2 {
        return;
    }
    if !needs_conversion(args[1], cx) {
        return;
    }

    if in_routing_block(node, cx) {
        return;
    }
    if uses_rack_test_methods(cx) {
        return;
    }

    let range = Range {
        start: cx.range(args[1]).start,
        end: cx.range(args[args.len() - 1]).end,
    };
    let msg = format!(
        "Use keyword arguments instead of positional arguments for http call: `{verb}`."
    );
    cx.emit_offense(range, &msg, None);

    let correction = build_correction(node, args, &verb, cx);
    cx.emit_edit(cx.range(node), &correction);
}

/// Upstream `needs_conversion?` on the second (first data) argument.
fn needs_conversion(data: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(data) {
        // `...` forwarding (`ForwardedArgs`, currently `Unknown` from translate).
        NodeKind::ForwardedArgs | NodeKind::Unknown => return false,
        NodeKind::Hash(_) => {}
        // Non-hash positional payload (lvar, send, splat, ...) needs conversion.
        _ => return true,
    }

    // Hash: `**opts` / `**nil` (any kwsplat) already uses kwargs style.
    let children = cx.children(data);
    if children.iter().any(|&c| {
        matches!(
            *cx.kind(c),
            NodeKind::Kwsplat(_) | NodeKind::Kwnilarg | NodeKind::ForwardedArgs | NodeKind::Unknown
        )
    }) {
        return false;
    }

    let pairs = cx.hash_pairs(data);
    // Empty `{}` needs conversion (correction drops it).
    if pairs.is_empty() {
        return true;
    }
    let single = pairs.len() == 1;
    for &pair in &pairs {
        let Some(key) = cx.pair_key(pair).get() else {
            continue;
        };
        let NodeKind::Sym(sym) = *cx.kind(key) else {
            continue;
        };
        let name = cx.symbol_str(sym);
        if KEYWORD_ARGS.contains(&name) {
            return false;
        }
        if single && name == "format" {
            return false;
        }
    }
    true
}

/// Upstream `in_routing_block?` — any `draw` / `routes` block ancestor.
fn in_routing_block(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        let call = match *cx.kind(ancestor) {
            NodeKind::Block { call, .. } => Some(call),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
            _ => None,
        };
        if let Some(call_id) = call
            && let Some(name) = cx.method_name(call_id)
            && (name == "draw" || name == "routes")
        {
            return true;
        }
    }
    false
}

/// Upstream `use_rack_test_methods?` — file contains `include Rack::Test::Methods`.
fn uses_rack_test_methods(cx: &Cx<'_>) -> bool {
    let root = cx.root();
    if is_rack_test_include(root, cx) {
        return true;
    }
    cx.descendants(root)
        .iter()
        .any(|&id| is_rack_test_include(id, cx))
}

/// `(send nil :include (const (const (const nil :Rack) :Test) :Methods))`.
/// `::Rack` folds to the same `Const{scope: None}` as bare `Rack`.
fn is_rack_test_include(id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    if cx.symbol_str(method) != "include" {
        return false;
    }
    if receiver.get().is_some() {
        return false;
    }
    let args = cx.call_arguments(id);
    if args.len() != 1 {
        return false;
    }
    as_const_chain(args[0], cx) == Some(("Methods", "Test", "Rack"))
}

/// Unwrap `Methods <- Test <- Rack` const chain; innermost scope must be `None`.
fn as_const_chain<'a>(id: NodeId, cx: &'a Cx<'a>) -> Option<(&'a str, &'a str, &'a str)> {
    let NodeKind::Const { scope, name } = *cx.kind(id) else {
        return None;
    };
    if cx.symbol_str(name) != "Methods" {
        return None;
    }
    let test_id = scope.get()?;
    let NodeKind::Const { scope, name } = *cx.kind(test_id) else {
        return None;
    };
    if cx.symbol_str(name) != "Test" {
        return None;
    }
    let rack_id = scope.get()?;
    let NodeKind::Const { scope, name } = *cx.kind(rack_id) else {
        return None;
    };
    if cx.symbol_str(name) != "Rack" {
        return None;
    }
    if scope.get().is_some() {
        return None;
    }
    Some(("Methods", "Test", "Rack"))
}

/// Upstream `correction` — whole-call rewrite preserving parens.
fn build_correction(node: NodeId, args: &[NodeId], verb: &str, cx: &Cx<'_>) -> String {
    let action_src = cx.raw_source(cx.range(args[0]));
    let params = convert_hash_data(args[1], "params", cx);
    let session = if args.len() > 2 {
        convert_hash_data(args[args.len() - 1], "session", cx)
    } else {
        String::new()
    };
    if cx.is_parenthesized(node) {
        format!("{verb}({action_src}{params}{session})")
    } else {
        format!("{verb} {action_src}{params}{session}")
    }
}

/// Upstream `convert_hash_data` — empty hash drops, hash wraps pairs,
/// non-hash passes through without braces.
fn convert_hash_data(data: NodeId, kind: &str, cx: &Cx<'_>) -> String {
    if let NodeKind::Hash(_) = *cx.kind(data) {
        let pairs = cx.hash_pairs(data);
        if pairs.is_empty() {
            // Also covers `{ **nil }`-free empty; kwsplat hashes never reach here.
            // A hash with only non-pair children (defensive) also drops.
            return String::new();
        }
        let joined = pairs
            .iter()
            .map(|&pair| cx.raw_source(cx.range(pair)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(", {kind}: {{ {joined} }}")
    } else {
        let src = cx.raw_source(cx.range(data));
        format!(", {kind}: {src}")
    }
}

#[cfg(test)]
mod tests {
    use super::HttpPositionalArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_get_with_positional_hash() {
        test::<HttpPositionalArguments>().expect_offense(indoc! {r#"
                get :create, user_id: @user.id
                             ^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#});
    }

    #[test]
    fn corrects_get_with_positional_hash() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                get :create, user_id: @user.id
                             ^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#},
            "get :create, params: { user_id: @user.id }\n",
        );
    }

    #[test]
    fn flags_each_verb() {
        for verb in ["post", "put", "patch", "delete", "head"] {
            // Offense range covers only the second arg (`user_id: @user.id`).
            let caret_start = verb.len() + ":create, ".len() + 1;
            let carets = "^".repeat("user_id: @user.id".len());
            let annotated = format!(
                "{verb} :create, user_id: @user.id\n{}{carets} \
                 Use keyword arguments instead of positional arguments for http call: `{verb}`.\n",
                " ".repeat(caret_start),
            );
            test::<HttpPositionalArguments>().expect_offense(&annotated);
        }
    }

    #[test]
    fn corrects_post_with_positional_hash() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                post :create, user_id: @user.id
                              ^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "post :create, params: { user_id: @user.id }\n",
        );
    }

    #[test]
    fn flags_method_call_action() {
        test::<HttpPositionalArguments>().expect_offense(indoc! {r#"
                post user_attrs, id: 1
                                 ^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#});
    }

    #[test]
    fn corrects_method_call_action() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                post user_attrs, id: 1
                                 ^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "post user_attrs, params: { id: 1 }\n",
        );
    }

    #[test]
    fn maintains_parentheses_when_autocorrecting() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                post(:user_attrs, id: 1)
                                  ^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "post(:user_attrs, params: { id: 1 })\n",
        );
    }

    #[test]
    fn maintains_quotes_when_autocorrecting() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                get '/auth/linkedin/callback', id: 1
                                               ^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#},
            "get '/auth/linkedin/callback', params: { id: 1 }\n",
        );
    }

    #[test]
    fn corrects_non_hash_payload_without_braces() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                post :create, confirmation_data
                              ^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "post :create, params: confirmation_data\n",
        );
    }

    #[test]
    fn flags_format_with_other_keys() {
        test::<HttpPositionalArguments>().expect_offense(indoc! {r#"
                post :create, id: 7, format: :rss
                              ^^^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#});
    }

    #[test]
    fn corrects_format_with_other_keys() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                post :create, id: 7, format: :rss
                              ^^^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "post :create, params: { id: 7, format: :rss }\n",
        );
    }

    #[test]
    fn corrects_two_hashes_to_params_and_session() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                get some_path(profile.id), { user_id: @user.id, profile_id: p.id }, 'HTTP_REFERER' => p_url(p.id).to_s
                                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#},
            "get some_path(profile.id), params: { user_id: @user.id, profile_id: p.id }, session: { 'HTTP_REFERER' => p_url(p.id).to_s }\n",
        );
    }

    #[test]
    fn corrects_empty_hash_to_session_only() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                get some_path(profile.id), {}, 'HTTP_REFERER' => p_url(p.id).to_s
                                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#},
            "get some_path(profile.id), session: { 'HTTP_REFERER' => p_url(p.id).to_s }\n",
        );
    }

    #[test]
    fn flags_lvar_payload() {
        test::<HttpPositionalArguments>().expect_correction(
            indoc! {r#"
                params = { id: 1 }
                post user_attrs, params
                                 ^^^^^^ Use keyword arguments instead of positional arguments for http call: `post`.
            "#},
            "params = { id: 1 }\npost user_attrs, params: params\n",
        );
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_single_arg() {
        test::<HttpPositionalArguments>().expect_no_offenses("get :new\n");
    }

    #[test]
    fn does_not_flag_bare_verb() {
        test::<HttpPositionalArguments>().expect_no_offenses("get\n");
    }

    #[test]
    fn does_not_flag_non_http_method() {
        test::<HttpPositionalArguments>().expect_no_offenses("puts :create, user_id: @user.id\n");
    }

    #[test]
    fn does_not_flag_process() {
        test::<HttpPositionalArguments>()
            .expect_no_offenses("process :new, method: :get, params: { user_id: @user.id }\n");
    }

    #[test]
    fn does_not_flag_receiver_call() {
        test::<HttpPositionalArguments>().expect_no_offenses("@user.get.id = ''\n");
    }

    #[test]
    fn does_not_flag_explicit_receiver() {
        test::<HttpPositionalArguments>()
            .expect_no_offenses("foo.get :new, { user_id: 1 }\n");
    }

    #[test]
    fn does_not_flag_keyword_args() {
        for kwargs in [
            "method: :get",
            "params: { user_id: @user.id }",
            "xhr: true",
            "session: { foo: 'bar' }",
            "format: :json",
            "headers: {}",
            "body: \"foo\"",
            "flash: {}",
            "as: :json",
            "env: \"test\"",
            "to: 'admin/admin#test'",
        ] {
            test::<HttpPositionalArguments>()
                .expect_no_offenses(&format!("get :new, {kwargs}\n"));
        }
    }

    #[test]
    fn does_not_flag_format_alone() {
        test::<HttpPositionalArguments>().expect_no_offenses("get :new, format: :json\n");
    }

    #[test]
    fn does_not_flag_kwsplat() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "[{ format: :json }, { format: :html }].each do |args|\n  get :nothing, **args\nend\n",
        );
    }

    #[test]
    fn does_not_flag_anonymous_kwsplat_nil() {
        test::<HttpPositionalArguments>().expect_no_offenses("get :new, **nil\n");
    }

    #[test]
    fn does_not_flag_forwarded_args() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "def perform_request(...)\n  get(:list, ...)\nend\n",
        );
    }

    #[test]
    fn does_not_flag_kwrest_forwarding() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "def perform_request(**options)\n  get(:list, **options)\nend\n",
        );
    }

    #[test]
    fn does_not_flag_in_routes_block() {
        test::<HttpPositionalArguments>().expect_no_offenses("routes do\n  get :list, on: :collection\nend\n");
    }

    #[test]
    fn does_not_flag_in_routes_draw_block() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "Rails.application.routes.draw do\n  get :list, on: :collection\nend\n",
        );
    }

    #[test]
    fn does_not_flag_with_rack_test_methods() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "include Rack::Test::Methods\n\nget :create, user_id: @user.id\n",
        );
    }

    #[test]
    fn does_not_flag_with_cbase_rack_test_methods() {
        test::<HttpPositionalArguments>().expect_no_offenses(
            "include ::Rack::Test::Methods\n\nget :create, user_id: @user.id\n",
        );
    }

    #[test]
    fn rails_42_does_not_flag() {
        test::<HttpPositionalArguments>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("get :create, user_id: @user.id\n");
    }

    #[test]
    fn rails_50_flags() {
        test::<HttpPositionalArguments>()
            .with_target_rails_version(5, 0)
            .expect_offense(indoc! {r#"
                get :create, user_id: @user.id
                             ^^^^^^^^^^^^^^^^^ Use keyword arguments instead of positional arguments for http call: `get`.
            "#});
    }

    #[test]
    fn does_not_flag_string_literal() {
        test::<HttpPositionalArguments>()
            .expect_no_offenses("\"get :new, user_id: 1\"\n");
    }

    #[test]
    fn does_not_flag_comment() {
        test::<HttpPositionalArguments>()
            .expect_no_offenses("# get :new, user_id: 1\n");
    }
}
murphy_plugin_api::submit_cop!(HttpPositionalArguments);
