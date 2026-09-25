//! `Rails/HttpStatus` — enforce symbolic or numeric HTTP status values.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HttpStatus
//! upstream_version_checked: 2.35.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RESTRICT_ON_SEND (render/redirect_to/head/assert_response/
//!   assert_redirected_to) is mirrored by the methods dispatch filter.
//!   Receiver must be None (bare calls only). render/redirect_to/
//!   assert_redirected_to inspect the status: pair of any Hash argument;
//!   head/assert_response inspect the bare first argument (Int/Sym only,
//!   matching upstream `${int sym}`). Symbolic style flags Int and numeric
//!   Str ('200' -> :ok); custom codes (e.g. 550) are ignored. Numeric style
//!   flags known symbols to their codes; :error/:success/:missing/:redirect
//!   and unknown symbols (e.g. :ng) are ignored. Offense range is the status
//!   value node only; autocorrect replaces it with the preferred style.
//! ```
//!
//! Enforces use of symbolic (`:ok`) or numeric (`200`) HTTP status values.
//! Default style is `symbolic`, matching RuboCop.
//!
//! ## Matched shapes (Send node, bare receiver only)
//!
//! - `render` / `redirect_to`: any `Hash` argument containing a
//!   `status:` pair whose value is `Int` / `Sym` / `Str`.
//!   e.g. `render :foo, status: 200`, `render status: :ok, json: x`.
//! - `assert_redirected_to`: same hash scan (allows trailing message arg,
//!   e.g. `assert_redirected_to '/p', { status: 301 }, 'msg'`).
//! - `head` / `assert_response`: bare first argument that is `Int` / `Sym`.
//!   e.g. `head 200`, `assert_response :ok`. `Str` is intentionally not
//!   matched here, mirroring upstream `${int sym}`.
//!
//! ## Implementation
//!
//! Dispatch is gated by `methods = [...]` (the five RESTRICT_ON_SEND names)
//! plus a bare-receiver check. Status extraction scans `Hash`/`Pair` nodes
//! via `cx.kind` + `cx.list` — no text matching. The Rack
//! `SYMBOL_TO_STATUS_CODE` table (derived from `HTTP_STATUS_CODES`) is
//! embedded as two `match` helpers below.
//!
//! ## Autocorrect
//!
//! Replace the status value node range with the preferred spelling:
//! `200` -> `:ok` (symbolic) or `:not_found` -> `404` (numeric).
//! For numeric strings (`status: '200'`) the whole quoted literal is
//! replaced, matching RuboCop.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, NodeList, cop};

// --- Rack status table (from rack/utils.rb HTTP_STATUS_CODES) ----------------
//
// Symbol = message.downcase with spaces/hyphens -> underscores.
// e.g. 200 OK -> :ok, 404 Not Found -> :not_found,
// 301 Moved Permanently -> :moved_permanently.

/// Map a numeric code to its symbolic name (without colon), or `None` for
/// custom/unknown codes (e.g. 550).
fn symbol_for_code(code: i64) -> Option<&'static str> {
    match code {
        100 => Some("continue"),
        101 => Some("switching_protocols"),
        102 => Some("processing"),
        103 => Some("early_hints"),
        200 => Some("ok"),
        201 => Some("created"),
        202 => Some("accepted"),
        203 => Some("non_authoritative_information"),
        204 => Some("no_content"),
        205 => Some("reset_content"),
        206 => Some("partial_content"),
        207 => Some("multi_status"),
        208 => Some("already_reported"),
        226 => Some("im_used"),
        300 => Some("multiple_choices"),
        301 => Some("moved_permanently"),
        302 => Some("found"),
        303 => Some("see_other"),
        304 => Some("not_modified"),
        305 => Some("use_proxy"),
        307 => Some("temporary_redirect"),
        308 => Some("permanent_redirect"),
        400 => Some("bad_request"),
        401 => Some("unauthorized"),
        402 => Some("payment_required"),
        403 => Some("forbidden"),
        404 => Some("not_found"),
        405 => Some("method_not_allowed"),
        406 => Some("not_acceptable"),
        407 => Some("proxy_authentication_required"),
        408 => Some("request_timeout"),
        409 => Some("conflict"),
        410 => Some("gone"),
        411 => Some("length_required"),
        412 => Some("precondition_failed"),
        413 => Some("content_too_large"),
        414 => Some("uri_too_long"),
        415 => Some("unsupported_media_type"),
        416 => Some("range_not_satisfiable"),
        417 => Some("expectation_failed"),
        421 => Some("misdirected_request"),
        422 => Some("unprocessable_content"),
        423 => Some("locked"),
        424 => Some("failed_dependency"),
        425 => Some("too_early"),
        426 => Some("upgrade_required"),
        428 => Some("precondition_required"),
        429 => Some("too_many_requests"),
        431 => Some("request_header_fields_too_large"),
        451 => Some("unavailable_for_legal_reasons"),
        500 => Some("internal_server_error"),
        501 => Some("not_implemented"),
        502 => Some("bad_gateway"),
        503 => Some("service_unavailable"),
        504 => Some("gateway_timeout"),
        505 => Some("http_version_not_supported"),
        506 => Some("variant_also_negotiates"),
        507 => Some("insufficient_storage"),
        508 => Some("loop_detected"),
        511 => Some("network_authentication_required"),
        _ => None,
    }
}

/// Map a symbolic name (without colon) to its numeric code, or `None` for
/// unknown symbols. Obsolete Rack aliases (`payload_too_large`,
/// `unprocessable_entity`, …) intentionally return `None` — upstream treats
/// them as unknown (no offense in numeric style).
fn code_for_symbol(sym: &str) -> Option<i64> {
    match sym {
        "continue" => Some(100),
        "switching_protocols" => Some(101),
        "processing" => Some(102),
        "early_hints" => Some(103),
        "ok" => Some(200),
        "created" => Some(201),
        "accepted" => Some(202),
        "non_authoritative_information" => Some(203),
        "no_content" => Some(204),
        "reset_content" => Some(205),
        "partial_content" => Some(206),
        "multi_status" => Some(207),
        "already_reported" => Some(208),
        "im_used" => Some(226),
        "multiple_choices" => Some(300),
        "moved_permanently" => Some(301),
        "found" => Some(302),
        "see_other" => Some(303),
        "not_modified" => Some(304),
        "use_proxy" => Some(305),
        "temporary_redirect" => Some(307),
        "permanent_redirect" => Some(308),
        "bad_request" => Some(400),
        "unauthorized" => Some(401),
        "payment_required" => Some(402),
        "forbidden" => Some(403),
        "not_found" => Some(404),
        "method_not_allowed" => Some(405),
        "not_acceptable" => Some(406),
        "proxy_authentication_required" => Some(407),
        "request_timeout" => Some(408),
        "conflict" => Some(409),
        "gone" => Some(410),
        "length_required" => Some(411),
        "precondition_failed" => Some(412),
        "content_too_large" => Some(413),
        "uri_too_long" => Some(414),
        "unsupported_media_type" => Some(415),
        "range_not_satisfiable" => Some(416),
        "expectation_failed" => Some(417),
        "misdirected_request" => Some(421),
        "unprocessable_content" => Some(422),
        "locked" => Some(423),
        "failed_dependency" => Some(424),
        "too_early" => Some(425),
        "upgrade_required" => Some(426),
        "precondition_required" => Some(428),
        "too_many_requests" => Some(429),
        "request_header_fields_too_large" => Some(431),
        "unavailable_for_legal_reasons" => Some(451),
        "internal_server_error" => Some(500),
        "not_implemented" => Some(501),
        "bad_gateway" => Some(502),
        "service_unavailable" => Some(503),
        "gateway_timeout" => Some(504),
        "http_version_not_supported" => Some(505),
        "variant_also_negotiates" => Some(506),
        "insufficient_storage" => Some(507),
        "loop_detected" => Some(508),
        "network_authentication_required" => Some(511),
        _ => None,
    }
}

/// `assert_response` shorthands permitted in numeric style (upstream
/// `NumericStyleChecker::PERMITTED_STATUS`). These never flag even though
/// they are symbols.
fn is_permitted_symbol(sym: &str) -> bool {
    matches!(sym, "error" | "success" | "missing" | "redirect")
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HttpStatus;

#[derive(CopOptions)]
pub struct HttpStatusOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symbolic",
        description = "Whether to enforce symbolic (`:ok`) or numeric (`200`) HTTP status values."
    )]
    pub enforced_style: HttpStatusStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatusStyle {
    #[option(value = "symbolic")]
    Symbolic,
    #[option(value = "numeric")]
    Numeric,
}

#[cop(
    name = "Rails/HttpStatus",
    description = "Enforces use of symbolic or numeric value to define HTTP status.",
    default_severity = "warning",
    default_enabled = true,
    options = HttpStatusOptions,
)]
impl HttpStatus {
    // Mirrors upstream `RESTRICT_ON_SEND` — dispatch only on the five
    // status-taking methods. The bare-receiver gate lives in the body.
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
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        // Upstream patterns all require `nil?` receiver (bare calls).
        if receiver.get().is_some() {
            return;
        }
        let opts = cx.options_or_default::<HttpStatusOptions>();
        let method_name = cx.symbol_str(method);
        let Some(status_node) = (match method_name {
            "render" | "redirect_to" | "assert_redirected_to" => {
                find_status_in_args(cx, args)
            }
            "head" | "assert_response" => first_arg_if_int_or_sym(cx, args),
            _ => None,
        }) else {
            return;
        };
        match opts.enforced_style {
            HttpStatusStyle::Symbolic => check_symbolic(status_node, cx),
            HttpStatusStyle::Numeric => check_numeric(status_node, cx),
        }
    }
}

/// Scan call arguments for a `Hash` containing a `status:` pair whose value
/// is `Int` / `Sym` / `Str`. Returns the value node (the offense target).
/// Only top-level pairs are considered — a nested `json: { status: 200 }`
/// payload does not match.
fn find_status_in_args(cx: &Cx<'_>, args: NodeList) -> Option<NodeId> {
    for &arg in cx.list(args) {
        let NodeKind::Hash(pairs) = *cx.kind(arg) else {
            continue;
        };
        for &pair_id in cx.list(pairs) {
            let NodeKind::Pair { key, value } = cx.kind(pair_id) else {
                continue;
            };
            let NodeKind::Sym(key_sym) = *cx.kind(*key) else {
                continue;
            };
            if cx.symbol_str(key_sym) != "status" {
                continue;
            }
            // Only Int/Sym/Str values are status candidates (upstream
            // `status_code` matcher). Other shapes (e.g. `status: x`)
            // are left alone.
            if matches!(
                *cx.kind(*value),
                NodeKind::Int(_) | NodeKind::Sym(_) | NodeKind::Str(_)
            ) {
                return Some(*value);
            }
        }
    }
    None
}

/// Bare first argument when it is `Int` / `Sym` (upstream `${int sym}` for
/// `head` / `assert_response`). `Str` is excluded to match upstream.
fn first_arg_if_int_or_sym(cx: &Cx<'_>, args: NodeList) -> Option<NodeId> {
    let &first = cx.list(args).first()?;
    if matches!(*cx.kind(first), NodeKind::Int(_) | NodeKind::Sym(_)) {
        Some(first)
    } else {
        None
    }
}

fn check_symbolic(status_node: NodeId, cx: &Cx<'_>) {
    match *cx.kind(status_node) {
        NodeKind::Sym(_) => {
            // Already symbolic — the preferred style.
        }
        NodeKind::Int(code) => {
            let Some(sym) = symbol_for_code(code) else {
                return; // custom code (e.g. 550) — ignored upstream.
            };
            cx.emit_offense(
                cx.range(status_node),
                &format!("Prefer `:{sym}` over `{code}` to define HTTP status code."),
                None,
            );
            cx.emit_edit(cx.range(status_node), &format!(":{sym}"));
        }
        NodeKind::Str(id) => {
            // Numeric strings ('200') flag like their Int form upstream.
            // Non-numeric or unmapped strings are ignored (conservative:
            // avoids synthesising `nil` corrections for values like 'ok').
            let raw = cx.string_str(id);
            let trimmed = raw.trim();
            let Ok(code) = trimmed.parse::<i64>() else {
                return;
            };
            let Some(sym) = symbol_for_code(code) else {
                return;
            };
            cx.emit_offense(
                cx.range(status_node),
                &format!("Prefer `:{sym}` over `{trimmed}` to define HTTP status code."),
                None,
            );
            cx.emit_edit(cx.range(status_node), &format!(":{sym}"));
        }
        _ => {}
    }
}

fn check_numeric(status_node: NodeId, cx: &Cx<'_>) {
    match *cx.kind(status_node) {
        NodeKind::Int(_) => {
            // Already numeric — the preferred style.
        }
        NodeKind::Sym(sym) => {
            let name = cx.symbol_str(sym);
            if is_permitted_symbol(name) {
                return;
            }
            let Some(code) = code_for_symbol(name) else {
                return; // unknown symbol (e.g. :ng) — ignored upstream.
            };
            cx.emit_offense(
                cx.range(status_node),
                &format!("Prefer `{code}` over `:{name}` to define HTTP status code."),
                None,
            );
            cx.emit_edit(cx.range(status_node), &code.to_string());
        }
        // Str never flags in numeric style upstream (the checker looks up
        // the symbol table with the string value and gets nil).
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{HttpStatus, HttpStatusOptions, HttpStatusStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn numeric_style() -> HttpStatusOptions {
        HttpStatusOptions {
            enforced_style: HttpStatusStyle::Numeric,
        }
    }

    // === symbolic style (default): numeric -> symbolic ===

    #[test]
    fn flags_render_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                render :foo, status: 200
                                     ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_render_json_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                render json: { foo: 'bar' }, status: 404
                                                     ^^^ Prefer `:not_found` over `404` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_render_with_status_first_in_hash() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                render status: 404, json: { foo: 'bar' }
                               ^^^ Prefer `:not_found` over `404` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_render_plain_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                render plain: 'foo/bar', status: 304
                                                 ^^^ Prefer `:not_modified` over `304` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_redirect_to_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                redirect_to root_url, status: 301
                                              ^^^ Prefer `:moved_permanently` over `301` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_head_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                head 200
                     ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_head_with_numeric_status_and_location() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                head 200, location: 'accounts'
                     ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_assert_response_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                assert_response 200
                                ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_assert_response_with_numeric_status_and_message() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                assert_response 404, 'message'
                                ^^^ Prefer `:not_found` over `404` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_assert_redirected_to_with_numeric_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                assert_redirected_to '/some/path', status: 301
                                                           ^^^ Prefer `:moved_permanently` over `301` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_assert_redirected_to_with_braced_hash_and_message() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                assert_redirected_to '/some/path', { status: 301 }, 'message'
                                                             ^^^ Prefer `:moved_permanently` over `301` to define HTTP status code.
            "#});
    }

    #[test]
    fn flags_numeric_string_status() {
        test::<HttpStatus>().expect_offense(indoc! {r#"
                render :foo, status: '200'
                                     ^^^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#});
    }

    #[test]
    fn corrects_render_numeric_to_symbolic() {
        test::<HttpStatus>().expect_correction(
            indoc! {r#"
                render :foo, status: 200
                                     ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#},
            "render :foo, status: :ok\n",
        );
    }

    #[test]
    fn corrects_head_numeric_to_symbolic() {
        test::<HttpStatus>().expect_correction(
            indoc! {r#"
                head 200
                     ^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#},
            "head :ok\n",
        );
    }

    #[test]
    fn corrects_numeric_string_to_symbolic() {
        test::<HttpStatus>().expect_correction(
            indoc! {r#"
                render :foo, status: '200'
                                     ^^^^^ Prefer `:ok` over `200` to define HTTP status code.
            "#},
            "render :foo, status: :ok\n",
        );
    }

    // === symbolic style: no-offense cases ===

    #[test]
    fn does_not_flag_symbolic_status() {
        test::<HttpStatus>().expect_no_offenses("render :foo, status: :ok\n");
    }

    #[test]
    fn does_not_flag_head_symbolic() {
        test::<HttpStatus>().expect_no_offenses("head :ok\n");
    }

    #[test]
    fn does_not_flag_assert_response_symbolic() {
        test::<HttpStatus>().expect_no_offenses("assert_response :ok\n");
    }

    #[test]
    fn does_not_flag_custom_numeric_code() {
        test::<HttpStatus>().expect_no_offenses("render :foo, status: 550\n");
    }

    #[test]
    fn does_not_flag_head_custom_code() {
        test::<HttpStatus>().expect_no_offenses("head 550\n");
    }

    #[test]
    fn does_not_flag_redirect_without_to() {
        // `redirect` (no _to) is not in RESTRICT_ON_SEND.
        test::<HttpStatus>().expect_no_offenses("get '/foobar', to: redirect('/foobar/baz', status: 301)\n");
    }

    #[test]
    fn does_not_flag_receiver_call() {
        // Bare calls only — an explicit receiver is a different method.
        test::<HttpStatus>().expect_no_offenses("obj.render :foo, status: 200\n");
    }

    #[test]
    fn does_not_flag_render_without_status() {
        test::<HttpStatus>().expect_no_offenses("render :foo\n");
    }

    #[test]
    fn does_not_flag_nested_json_status_key() {
        // `status` inside the json payload is not the status: option.
        test::<HttpStatus>().expect_no_offenses("render json: { status: 200 }\n");
    }

    // === numeric style: symbolic -> numeric ===

    #[test]
    fn numeric_flags_render_symbolic() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_offense(indoc! {r#"
                render :foo, status: :ok
                                     ^^^ Prefer `200` over `:ok` to define HTTP status code.
            "#});
    }

    #[test]
    fn numeric_flags_head_symbolic() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_offense(indoc! {r#"
                head :ok
                     ^^^ Prefer `200` over `:ok` to define HTTP status code.
            "#});
    }

    #[test]
    fn numeric_flags_assert_response_symbolic_with_message() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_offense(indoc! {r#"
                assert_response :not_found, 'message'
                                ^^^^^^^^^^ Prefer `404` over `:not_found` to define HTTP status code.
            "#});
    }

    #[test]
    fn numeric_flags_redirect_to_symbolic() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_offense(indoc! {r#"
                redirect_to root_url, status: :moved_permanently
                                              ^^^^^^^^^^^^^^^^^^ Prefer `301` over `:moved_permanently` to define HTTP status code.
            "#});
    }

    #[test]
    fn numeric_corrects_render_symbolic_to_numeric() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_correction(
                indoc! {r#"
                    render :foo, status: :ok
                                         ^^^ Prefer `200` over `:ok` to define HTTP status code.
                "#},
                "render :foo, status: 200\n",
            );
    }

    #[test]
    fn numeric_does_not_flag_numeric_status() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_no_offenses("render :foo, status: 200\n");
    }

    #[test]
    fn numeric_does_not_flag_permitted_symbols() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_no_offenses("assert_response :success\n");
    }

    #[test]
    fn numeric_does_not_flag_permitted_error_in_render() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_no_offenses("render :foo, status: :error\n");
    }

    #[test]
    fn numeric_does_not_flag_unknown_symbol() {
        test::<HttpStatus>()
            .with_options(&numeric_style())
            .expect_no_offenses("render json: { foo: 'bar' }, status: :ng\n");
    }
}

murphy_plugin_api::submit_cop!(HttpStatus);
