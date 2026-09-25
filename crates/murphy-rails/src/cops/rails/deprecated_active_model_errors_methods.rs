//! `Rails/DeprecatedActiveModelErrorsMethods` — flag direct hash manipulation of
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DeprecatedActiveModelErrorsMethods
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Send-dispatch port of RuboCop's on_send patterns (root/manipulation,
//!   root assignment, errors keys/values/to_h/to_xml, messages/details
//!   manipulation and assignment). Receiver gating (send/ivar/lvar, bare
//!   errors only in /models/ paths) and TargetRailsVersion gating for
//!   keys/values/to_h/to_xml (>= 6.1) are implemented. Autocorrect covers
//!   <<, clear and keys with the same skip rule for details+<< as upstream.
//!   Upstream is Enabled: pending, Safe: false — Murphy maps pending to
//!   default_enabled = false.
//! ```
//!
//! ActiveModel errors as a hash. These operations were deprecated in
//! Rails 6.1 and removed in Rails 7.
//!
//! ## Matched shapes (Send nodes only)
//!
//! Upstream `on_send` covers five shapes (all `Send`, never `Csend`):
//!
//! - **root manipulation**: `Send(receiver=Send(receiver=errors_send, method="[]"), method in MANIPULATIVE)`
//!   e.g. `user.errors[:name] << 'msg'`, `user.errors[:name].clear`.
//! - **root assignment**: `Send(receiver=errors_send, method="[]=")`
//!   e.g. `user.errors[:name] = []`.
//! - **errors deprecated**: `Send(receiver=errors_send, method in {keys, values, to_h, to_xml}, args=[])`
//!   e.g. `user.errors.keys`. Gated on `TargetRailsVersion >= 6.1`.
//! - **messages/details manipulation**: outer manipulative whose `:[]` receiver
//!   sits on `errors.messages` / `errors.details`, e.g.
//!   `user.errors.messages[:name] << 'msg'`.
//! - **messages/details assignment**: `[]=` on `errors.messages` / `errors.details`,
//!   e.g. `user.errors.messages[:name] = []`.
//!
//! Where `errors_send` is `Send(receiver=RECV, method="errors", args=[])`
//! with `RECV` in `{send, ivar, lvar}` — or bare (`None`) only inside
//! `/models/` paths (mirrors RuboCop's `receiver_matcher` split on
//! `file_path.include?('/models/')`). `messages` / `details` are
//! zero-arg sends on the errors node. `Const` / `cvar` / `gvar` / `self`
//! receivers never match (upstream is equally strict).
//!
//! `MANIPULATIVE_METHODS` (30, upstream `Set`):
//! `<< append clear collect! compact! concat delete delete_at delete_if drop
//! drop_while fill filter! keep_if flatten! insert map! pop prepend push reject!
//! replace reverse! rotate! select! shift shuffle! slice! sort! sort_by! uniq! unshift`.
//!
//! ## Offense range and message
//!
//! Whole outer send (`cx.range(node)`), message
//! `Avoid manipulating ActiveModel errors as hash directly.`
//!
//! ## Autocorrect
//!
//! - `<<` (except `details` + `<<`): `[:key] << val` → `.add(key, val)`.
//! - `clear`: `[:key].clear` → `.delete(key)`.
//! - `keys`: `.keys` → `.attribute_names`.
//! - Everything else (`[]=`, `values`, `to_h`, `to_xml`, other manipulative,
//!   `details` + `<<`): offense only, no edits.
//!
//! The edit range runs from the end of the `errors` send to the end of the
//! outer node (mirrors RuboCop's `offense_range`), so
//! `user.errors.messages[:name] << 'msg'` rewrites to
//! `user.errors.add(:name, 'msg')`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "Avoid manipulating ActiveModel errors as hash directly.";

const MANIPULATIVE_METHODS: &[&str] = &[
    "<<",
    "append",
    "clear",
    "collect!",
    "compact!",
    "concat",
    "delete",
    "delete_at",
    "delete_if",
    "drop",
    "drop_while",
    "fill",
    "filter!",
    "keep_if",
    "flatten!",
    "insert",
    "map!",
    "pop",
    "prepend",
    "push",
    "reject!",
    "replace",
    "reverse!",
    "rotate!",
    "select!",
    "shift",
    "shuffle!",
    "slice!",
    "sort!",
    "sort_by!",
    "uniq!",
    "unshift",
];

const INCOMPATIBLE_METHODS: &[&str] = &["keys", "values", "to_h", "to_xml"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DeprecatedActiveModelErrorsMethods;

#[cop(
    name = "Rails/DeprecatedActiveModelErrorsMethods",
    description = "Avoid manipulating ActiveModel errors as hash directly.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DeprecatedActiveModelErrorsMethods {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        let method_str = cx.symbol_str(method);

        if INCOMPATIBLE_METHODS.contains(&method_str) {
            // `errors.keys` etc. take zero args upstream (no `...` in pattern).
            if !cx.list(args).is_empty() {
                return;
            }
            // Upstream: skip when TargetRailsVersion <= 6.0.
            // Murphy unset means newest, so flag when unset.
            if !cx.rails_version_at_least(6, 1) {
                return;
            }
            let Some(errors_id) = receiver.get() else {
                return;
            };
            if !is_valid_errors_call(errors_id, cx) {
                return;
            }
            cx.emit_offense(cx.range(node), MSG, None);
            if method_str == "keys" {
                emit_keys_correction(node, errors_id, cx);
            }
            return;
        }

        if method_str == "[]=" {
            let Some(recv) = receiver.get() else {
                return;
            };
            // Root: `errors` directly.
            if is_valid_errors_call(recv, cx) {
                cx.emit_offense(cx.range(node), MSG, None);
                return;
            }
            // messages/details: `errors.messages` / `errors.details`.
            if as_messages_details_call(recv, cx).is_some() {
                cx.emit_offense(cx.range(node), MSG, None);
            }
            return;
        }

        if MANIPULATIVE_METHODS.contains(&method_str) {
            let Some(bracket_id) = receiver.get() else {
                return;
            };
            // Bracket must be a `Send :[]` (any args, upstream `...`).
            let NodeKind::Send {
                receiver: bracket_recv,
                method: bracket_method,
                ..
            } = *cx.kind(bracket_id)
            else {
                return;
            };
            if cx.symbol_str(bracket_method) != "[]" {
                return;
            }
            let Some(inner) = bracket_recv.get() else {
                return;
            };
            // Root: `errors[:k].manip`.
            if is_valid_errors_call(inner, cx) {
                cx.emit_offense(cx.range(node), MSG, None);
                emit_manipulative_correction(node, bracket_id, inner, false, cx);
                return;
            }
            // messages/details: `errors.messages[:k].manip`.
            if let Some((errors_id, is_details)) = as_messages_details_call(inner, cx) {
                cx.emit_offense(cx.range(node), MSG, None);
                emit_manipulative_correction(node, bracket_id, errors_id, is_details, cx);
            }
        }
    }
}

fn is_model_file(cx: &Cx<'_>) -> bool {
    cx.file_path().contains("/models/")
}

/// `RECV` gate for the receiver of an `errors` call.
///
/// Outside `/models/`: only `Send`, `Ivar`, `Lvar`.
/// Inside `/models/`: additionally bare (`None`, implicit self).
fn errors_receiver_valid(recv: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(rid) = recv.get() else {
        return is_model_file(cx);
    };
    matches!(
        *cx.kind(rid),
        NodeKind::Send { .. } | NodeKind::Ivar(..) | NodeKind::Lvar(..)
    )
}

/// `Send(receiver=RECV, method="errors", args=[])` with a valid `RECV`.
fn is_valid_errors_call(id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(id)
    else {
        return false;
    };
    if cx.symbol_str(method) != "errors" {
        return false;
    }
    if !cx.list(args).is_empty() {
        return false;
    }
    errors_receiver_valid(receiver, cx)
}

/// `Send(receiver=errors_send, method in {messages, details}, args=[])`.
///
/// Returns `(errors_id, is_details)` on match.
fn as_messages_details_call(id: NodeId, cx: &Cx<'_>) -> Option<(NodeId, bool)> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(id)
    else {
        return None;
    };
    let name = cx.symbol_str(method);
    let is_details = match name {
        "messages" => false,
        "details" => true,
        _ => return None,
    };
    if !cx.list(args).is_empty() {
        return None;
    }
    let errors_id = receiver.get()?;
    if !is_valid_errors_call(errors_id, cx) {
        return None;
    }
    Some((errors_id, is_details))
}

/// `.keys` → `.attribute_names`.
fn emit_keys_correction(outer: NodeId, errors_id: NodeId, cx: &Cx<'_>) {
    let range = Range {
        start: cx.range(errors_id).end,
        end: cx.range(outer).end,
    };
    cx.emit_edit(range, ".attribute_names");
}

/// `<<` → `.add(key, val)`, `clear` → `.delete(key)`.
///
/// `errors_id` is the innermost `errors` send (for range start).
/// `is_details` suppresses the `<<` edit (upstream `skip_autocorrect?`).
fn emit_manipulative_correction(
    outer: NodeId,
    bracket_id: NodeId,
    errors_id: NodeId,
    is_details: bool,
    cx: &Cx<'_>,
) {
    let Some(method) = cx.method_name(outer) else {
        return;
    };
    if method != "<<" && method != "clear" {
        return;
    }
    if is_details && method == "<<" {
        return;
    }
    let Some(key_id) = cx.first_argument(bracket_id).get() else {
        return;
    };
    let key_src = cx.raw_source(cx.range(key_id));
    let replacement = if method == "<<" {
        let Some(val_id) = cx.first_argument(outer).get() else {
            return;
        };
        let val_src = cx.raw_source(cx.range(val_id));
        format!(".add({key_src}, {val_src})")
    } else {
        format!(".delete({key_src})")
    };
    let range = Range {
        start: cx.range(errors_id).end,
        end: cx.range(outer).end,
    };
    cx.emit_edit(range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::DeprecatedActiveModelErrorsMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    const MODEL_PATH: &str = "/foo/app/models/bar.rb";

    // === root manipulation (<<) ===

    #[test]
    fn flags_root_shovel() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_root_shovel_to_add() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.add(:name, 'msg')\n",
        );
    }

    #[test]
    fn flags_root_assignment() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors[:name] = []
                ^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn assignment_has_no_autocorrect() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_corrections("user.errors[:name] = []\n");
    }

    #[test]
    fn flags_root_clear() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors[:name].clear
                ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_root_clear_to_delete() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors[:name].clear
                ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.delete(:name)\n",
        );
    }

    // === errors.keys / values / to_h / to_xml (Rails >= 6.1) ===

    #[test]
    fn flags_errors_keys_on_new_rails() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                user.errors.keys
                ^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn flags_errors_keys_when_version_unset() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.keys
                ^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn does_not_flag_errors_keys_on_old_rails() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 0)
            .expect_no_offenses("user.errors.keys\n");
    }

    #[test]
    fn corrects_errors_keys_to_attribute_names() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors.keys
                ^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.attribute_names\n",
        );
    }

    #[test]
    fn flags_errors_values() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                user.errors.values
                ^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn flags_errors_to_h() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                user.errors.to_h
                ^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn flags_errors_to_xml() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                user.errors.to_xml
                ^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn values_has_no_autocorrect() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_target_rails_version(6, 1)
            .expect_no_corrections("user.errors.values\n");
    }

    // === messages / details ===

    #[test]
    fn flags_messages_shovel() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.messages[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_messages_shovel_to_add() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors.messages[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.add(:name, 'msg')\n",
        );
    }

    #[test]
    fn flags_messages_assignment() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.messages[:name] = []
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn flags_messages_clear() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.messages[:name].clear
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_messages_clear_to_delete() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors.messages[:name].clear
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.delete(:name)\n",
        );
    }

    #[test]
    fn flags_details_shovel_without_autocorrect() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.details[:name] << {}
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn details_shovel_has_no_autocorrect() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_corrections("user.errors.details[:name] << {}\n");
    }

    #[test]
    fn flags_details_assignment() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user.errors.details[:name] = []
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_details_clear_to_delete() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_correction(
            indoc! {r#"
                user.errors.details[:name].clear
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#},
            "user.errors.delete(:name)\n",
        );
    }

    // === receiver gating ===

    #[test]
    fn flags_ivar_receiver() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                @user.errors[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn flags_lvar_receiver() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_offense(indoc! {r#"
                user = create_user
                user.errors.keys
                ^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn does_not_flag_const_receiver() {
        // Upstream receiver_matcher is {send ivar lvar} — Const never matches.
        test::<DeprecatedActiveModelErrorsMethods>().expect_no_offenses("User.errors.keys\n");
    }

    #[test]
    fn does_not_flag_bare_errors_outside_model() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_no_offenses("errors[:name] << 'msg'\n");
    }

    #[test]
    fn flags_bare_errors_in_model_file() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_file_path(MODEL_PATH)
            .expect_offense(indoc! {r#"
                errors[:name] << 'msg'
                ^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    #[test]
    fn corrects_bare_errors_in_model_file() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_file_path(MODEL_PATH)
            .expect_correction(
                indoc! {r#"
                    errors[:name] << 'msg'
                    ^^^^^^^^^^^^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
                "#},
                "errors.add(:name, 'msg')\n",
            );
    }

    #[test]
    fn flags_bare_errors_keys_in_model_file() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_file_path(MODEL_PATH)
            .expect_offense(indoc! {r#"
                errors.keys
                ^^^^^^^^^^^ Avoid manipulating ActiveModel errors as hash directly.
            "#});
    }

    // === non-manipulative / false positives ===

    #[test]
    fn does_not_flag_present_on_bracket() {
        test::<DeprecatedActiveModelErrorsMethods>().expect_no_offenses("errors[:name].present?\n");
    }

    #[test]
    fn does_not_flag_messages_present() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .with_file_path(MODEL_PATH)
            .expect_no_offenses("errors.messages[:name].present?\n");
    }

    #[test]
    fn does_not_flag_errors_add() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_offenses("user.errors.add(:name, 'msg')\n");
    }

    #[test]
    fn does_not_flag_errors_delete() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_offenses("user.errors.delete(:name)\n");
    }

    #[test]
    fn does_not_flag_comment() {
        // Text scan would hit the words; Send dispatch must not.
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_offenses("# user.errors[:name] << 'msg'\nuser.name\n");
    }

    #[test]
    fn does_not_flag_string_literal() {
        test::<DeprecatedActiveModelErrorsMethods>()
            .expect_no_offenses("\"user.errors[:name] << 'msg'\"\n");
    }

    #[test]
    fn does_not_flag_errors_with_args() {
        // `errors(x)` is not the zero-arg reader upstream matches.
        test::<DeprecatedActiveModelErrorsMethods>().expect_no_offenses("user.errors(:x).keys\n");
    }

    #[test]
    fn does_not_flag_keys_with_args() {
        // Upstream errors_deprecated pattern has no `...` — zero args only.
        test::<DeprecatedActiveModelErrorsMethods>().expect_no_offenses("user.errors.keys(:x)\n");
    }
}
murphy_plugin_api::submit_cop!(DeprecatedActiveModelErrorsMethods);
