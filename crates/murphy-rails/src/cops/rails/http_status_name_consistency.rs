//! `Rails/HttpStatusNameConsistency` — use current HTTP status names.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HttpStatusNameConsistency
//! upstream_version_checked: 2.35.0
//! version_added: "2.34"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   (render/redirect_to/head/assert_response/assert_redirected_to) with bare
//!   receivers, `status:` hash-value extraction for render/redirect_to/
//!   assert_redirected_to, bare first-argument extraction for head/
//!   assert_response, and recursive descent into the status value (so ternary
//!   `cond ? :unprocessable_entity : :ok` flags). Only the two Rack 3.1
//!   renames (`unprocessable_entity` → `unprocessable_content`,
//!   `payload_too_large` → `content_too_large`) flag; offense is the Sym node
//!   with symbol replacement. `requires_gem rack >= 3.1.0` is not gated:
//!   Murphy always enforces the new names. Upstream Include
//!   (`**/app/controllers/**/*.rb`) has no file-path infrastructure yet, so
//!   the cop fires in all files (audit tracked by murphy-4gd.1.15).
//! ```
//!
//! Enforces consistency by using the current HTTP status names.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HttpStatusNameConsistency;

fn preferred_for(current: &str) -> Option<&'static str> {
    match current {
        "unprocessable_entity" => Some("unprocessable_content"),
        "payload_too_large" => Some("content_too_large"),
        _ => None,
    }
}

#[cop(
    name = "Rails/HttpStatusNameConsistency",
    description = "Enforces consistency by using the current HTTP status names.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl HttpStatusNameConsistency {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(
        kind = "send",
        methods = [
            "render",
            "redirect_to",
            "head",
            "assert_response",
            "assert_redirected_to"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        // All upstream patterns require a bare (`nil?`) receiver.
        if receiver.get().is_some() {
            return;
        }
        let method_name = cx.symbol_str(method).to_owned();
        match method_name.as_str() {
            "render" | "redirect_to" | "assert_redirected_to" => {
                for value in status_hash_values(cx, node) {
                    check_value_recursive(cx, value);
                }
            }
            "head" | "assert_response" => {
                let args = cx.call_arguments(node);
                let Some(&first) = args.first() else {
                    return;
                };
                check_value_recursive(cx, first);
            }
            _ => {}
        }
    }
}

/// Every `status:` pair value in any `Hash` argument (mirrors upstream
/// `status_hash_value` over the captured `$hash`).
fn status_hash_values(cx: &Cx<'_>, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &arg in cx.call_arguments(node) {
        let NodeKind::Hash(pairs) = *cx.kind(arg) else {
            continue;
        };
        for &pair_id in cx.list(pairs) {
            let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                continue;
            };
            let NodeKind::Sym(key_sym) = *cx.kind(key) else {
                continue;
            };
            if cx.symbol_str(key_sym) != "status" {
                continue;
            }
            out.push(value);
        }
    }
    out
}

/// Upstream `check_status_name_consistency`: offense when the node is a
/// matching Sym, else recurse into children (covers ternary branches).
fn check_value_recursive(cx: &Cx<'_>, node: NodeId) {
    if let NodeKind::Sym(sym) = *cx.kind(node) {
        let current = cx.symbol_str(sym).to_owned();
        if let Some(preferred) = preferred_for(&current) {
            cx.emit_offense(
                cx.range(node),
                &format!("Prefer `:{preferred}` over `:{current}`."),
                None,
            );
            cx.emit_edit(cx.range(node), &format!(":{preferred}"));
        }
        return;
    }
    for child in cx.children(node) {
        check_value_recursive(cx, child);
    }
}

#[cfg(test)]
mod tests {
    use super::HttpStatusNameConsistency;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_render_unprocessable_entity() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            render json: { error: 'Invalid data' }, status: :unprocessable_entity
                                                            ^^^^^^^^^^^^^^^^^^^^^ Prefer `:unprocessable_content` over `:unprocessable_entity`.
        "#});
    }

    #[test]
    fn corrects_render() {
        test::<HttpStatusNameConsistency>().expect_correction(
            indoc! {r#"
                render json: { error: 'Invalid data' }, status: :unprocessable_entity
                                                                ^^^^^^^^^^^^^^^^^^^^^ Prefer `:unprocessable_content` over `:unprocessable_entity`.
            "#},
            "render json: { error: 'Invalid data' }, status: :unprocessable_content\n",
        );
    }

    #[test]
    fn flags_head_payload_too_large() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            head :payload_too_large
                 ^^^^^^^^^^^^^^^^^^ Prefer `:content_too_large` over `:payload_too_large`.
        "#});
    }

    #[test]
    fn flags_redirect_to() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            redirect_to some_path, status: :unprocessable_entity
                                           ^^^^^^^^^^^^^^^^^^^^^ Prefer `:unprocessable_content` over `:unprocessable_entity`.
        "#});
    }

    #[test]
    fn flags_assert_response() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            assert_response :unprocessable_entity
                            ^^^^^^^^^^^^^^^^^^^^^ Prefer `:unprocessable_content` over `:unprocessable_entity`.
        "#});
    }

    #[test]
    fn flags_assert_redirected_to() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            assert_redirected_to some_path, status: :payload_too_large
                                                    ^^^^^^^^^^^^^^^^^^ Prefer `:content_too_large` over `:payload_too_large`.
        "#});
    }

    #[test]
    fn flags_ternary_branch() {
        test::<HttpStatusNameConsistency>().expect_offense(indoc! {r#"
            render json: { error: 'Invalid data' }, status: some_condition ? :unprocessable_entity : :ok
                                                                             ^^^^^^^^^^^^^^^^^^^^^ Prefer `:unprocessable_content` over `:unprocessable_entity`.
        "#});
    }

    #[test]
    fn does_not_flag_preferred() {
        test::<HttpStatusNameConsistency>().expect_no_offenses(
            "render json: { error: 'Invalid data' }, status: :unprocessable_content\n",
        );
        test::<HttpStatusNameConsistency>()
            .expect_no_offenses("head :content_too_large\n");
    }

    #[test]
    fn does_not_flag_hash_key() {
        test::<HttpStatusNameConsistency>()
            .expect_no_offenses("{ unprocessable_entity: 'Invalid data' }\n");
    }

    #[test]
    fn does_not_flag_variable_or_method() {
        test::<HttpStatusNameConsistency>().expect_no_offenses("head status_var\n");
        test::<HttpStatusNameConsistency>().expect_no_offenses("head get_status_code\n");
        test::<HttpStatusNameConsistency>().expect_no_offenses(
            "render json: { error: 'Invalid data' }, status: calculate_status\n",
        );
    }

    #[test]
    fn does_not_flag_receiver_call() {
        test::<HttpStatusNameConsistency>()
            .expect_no_offenses("obj.head :payload_too_large\n");
    }
}
murphy_plugin_api::submit_cop!(HttpStatusNameConsistency);
