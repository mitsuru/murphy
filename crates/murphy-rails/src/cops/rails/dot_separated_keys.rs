//! `Rails/DotSeparatedKeys` — enforce dot-separated locale keys over `:scope`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DotSeparatedKeys
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `translate`/`t` with bare or `I18n`
//!   (`I18n`/`::I18n`) receiver, `${sym str}` key plus trailing hash
//!   containing a `(pair (sym :scope) (array|sym))` pair whose scopes are
//!   all `basic_literal?`. Pair-only offense range; autocorrect removes the
//!   scope pair (preceding comma) and rewrites the key to the squeezed
//!   single-quoted dotted form (`'a.b.key'`). `scope:` as a string does not
//!   flag (upstream `${array sym}` only).
//! ```
//!
//! ## Matched shapes
//!
//! - `I18n.t :record_invalid, scope: [:activerecord, :errors, :messages]`
//!   → `I18n.t 'activerecord.errors.messages.record_invalid'`.
//! - `I18n.t :title, scope: :invitation` → `I18n.t 'invitation.title'`.
//!
//! `I18n.t :title, scope: "invitation"` (string scope) does not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use the dot-separated keys instead of specifying the `:scope` option.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DotSeparatedKeys;

#[cop(
    name = "Rails/DotSeparatedKeys",
    description = "Enforces the use of dot-separated keys instead of `:scope` options in `I18n` translation methods.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DotSeparatedKeys {
    #[on_node(kind = "send", methods = ["translate", "t"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        let method_name = cx.symbol_str(method);
        if method_name != "translate" && method_name != "t" {
            return;
        }
        // Receiver: bare (`t`) or `I18n` / `::I18n`.
        if let Some(recv) = receiver.get()
            && cx.const_name(recv).as_deref() != Some("I18n")
        {
            return;
        }
        let args = cx.call_arguments(node);
        // Upstream pattern is exactly key + options hash.
        if args.len() != 2 {
            return;
        }
        let key_id = args[0];
        if !matches!(*cx.kind(key_id), NodeKind::Sym(_) | NodeKind::Str(_)) {
            return;
        }
        let hash_id = args[1];
        let NodeKind::Hash(pairs) = *cx.kind(hash_id) else {
            return;
        };
        let mut scope_pair = None;
        let mut scope_value = None;
        for &pair_id in cx.list(pairs) {
            let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                continue;
            };
            let NodeKind::Sym(k) = *cx.kind(key) else {
                continue;
            };
            if cx.symbol_str(k) != "scope" {
                continue;
            }
            // Upstream `${array sym}` — string scopes do not match.
            if !matches!(
                *cx.kind(value),
                NodeKind::Array(_) | NodeKind::Sym(_)
            ) {
                return;
            }
            scope_pair = Some(pair_id);
            scope_value = Some(value);
            break;
        }
        let (Some(pair_id), Some(value_id)) = (scope_pair, scope_value) else {
            return;
        };
        let scope_elems: Vec<NodeId> = match *cx.kind(value_id) {
            NodeKind::Array(list) => cx.list(list).to_vec(),
            NodeKind::Sym(_) => vec![value_id],
            _ => return,
        };
        // `should_convert_scope?`: all scopes `basic_literal?`.
        if !scope_elems.iter().all(|&e| cx.is_basic_literal(e)) {
            return;
        }
        cx.emit_offense(cx.range(pair_id), MSG, None);
        // Autocorrect: remove scope pair (preceding comma) + rewrite key.
        let remove_range = scope_removal_range(cx, pair_id);
        cx.emit_edit(remove_range, "");
        let new_key = dotted_key(cx, key_id, &scope_elems);
        cx.emit_edit(cx.range(key_id), &new_key);
    }
}

/// Removal range for the scope pair: pair range extended left through spaces
/// to include the preceding `,` (arg separator) when present, else through a
/// trailing comma. Mirrors RuboCop's `range_with_surrounding_space` +
/// `range_with_surrounding_comma` (left side).
fn scope_removal_range(cx: &Cx<'_>, pair_id: NodeId) -> Range {
    let pair = cx.range(pair_id);
    let src = cx.source().as_bytes();
    let mut start = pair.start as usize;
    // Eat spaces/tabs left (not newlines — single-line fast path; newlines
    // handled by the comma scan below which also eats spaces).
    while start > 0 && (src[start - 1] == b' ' || src[start - 1] == b'\t') {
        start -= 1;
    }
    // Preceding comma (implicit-hash arg separator or explicit-hash pair
    // separator) → include it plus spaces before it.
    if start > 0 && src[start - 1] == b',' {
        start -= 1;
        while start > 0 && (src[start - 1] == b' ' || src[start - 1] == b'\t') {
            start -= 1;
        }
        return Range {
            start: start as u32,
            end: pair.end,
        };
    }
    // No preceding comma (e.g. `{scope: ..., default: ...}` first pair):
    // remove trailing comma instead to keep the hash valid.
    let mut end = pair.end as usize;
    let mut tmp = end;
    while tmp < src.len() && (src[tmp] == b' ' || src[tmp] == b'\t') {
        tmp += 1;
    }
    if tmp < src.len() && src[tmp] == b',' {
        end = tmp + 1;
    }
    Range {
        start: pair.start,
        end: end as u32,
    }
}

/// `'scope1.scope2.key'` squeezed (consecutive `.` collapsed).
fn dotted_key(cx: &Cx<'_>, key_id: NodeId, scopes: &[NodeId]) -> String {
    let mut parts: Vec<String> = scopes.iter().map(|&s| literal_value(cx, s)).collect();
    parts.push(literal_value(cx, key_id));
    let joined = parts.join(".");
    // `String#squeeze('.')` — collapse `..` runs.
    let mut squeezed = String::with_capacity(joined.len() + 2);
    let mut prev_dot = false;
    for ch in joined.chars() {
        if ch == '.' {
            if !prev_dot {
                squeezed.push(ch);
            }
            prev_dot = true;
        } else {
            squeezed.push(ch);
            prev_dot = false;
        }
    }
    format!("'{squeezed}'")
}

fn literal_value(cx: &Cx<'_>, id: NodeId) -> String {
    match *cx.kind(id) {
        NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        _ => cx.raw_source(cx.range(id)).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::DotSeparatedKeys;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_array_scope() {
        test::<DotSeparatedKeys>().expect_offense(indoc! {r#"
            I18n.t :record_invalid, scope: [:activerecord, :errors, :messages]
                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
        "#});
    }

    #[test]
    fn flags_sym_scope() {
        test::<DotSeparatedKeys>().expect_offense(indoc! {r#"
            I18n.t :title, scope: :invitation
                           ^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
        "#});
    }

    #[test]
    fn flags_bare_t() {
        test::<DotSeparatedKeys>().expect_offense(indoc! {r#"
            t :title, scope: :invitation
                      ^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
        "#});
    }

    #[test]
    fn flags_translate() {
        test::<DotSeparatedKeys>().expect_offense(indoc! {r#"
            I18n.translate :title, scope: :invitation
                                   ^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
        "#});
    }

    #[test]
    fn does_not_flag_string_scope() {
        test::<DotSeparatedKeys>()
            .expect_no_offenses("I18n.t :title, scope: \"invitation\"\n");
    }

    #[test]
    fn does_not_flag_without_scope() {
        test::<DotSeparatedKeys>().expect_no_offenses("I18n.t :title\n");
    }

    #[test]
    fn does_not_flag_other_receiver() {
        test::<DotSeparatedKeys>()
            .expect_no_offenses("Foo.t :title, scope: :invitation\n");
    }

    #[test]
    fn does_not_flag_non_literal_scope() {
        test::<DotSeparatedKeys>()
            .expect_no_offenses("I18n.t :title, scope: [foo, :bar]\n");
    }

    #[test]
    fn corrects_array_scope() {
        test::<DotSeparatedKeys>()
            .expect_correction(
                indoc! {r#"
                    I18n.t :record_invalid, scope: [:activerecord, :errors, :messages]
                                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
                "#},
                "I18n.t 'activerecord.errors.messages.record_invalid'\n",
            )
            .expect_no_offenses("I18n.t 'activerecord.errors.messages.record_invalid'\n");
    }

    #[test]
    fn corrects_sym_scope() {
        test::<DotSeparatedKeys>()
            .expect_correction(
                indoc! {r#"
                    I18n.t :title, scope: :invitation
                                   ^^^^^^^^^^^^^^^^^^ Use the dot-separated keys instead of specifying the `:scope` option.
                "#},
                "I18n.t 'invitation.title'\n",
            )
            .expect_no_offenses("I18n.t 'invitation.title'\n");
    }
}
murphy_plugin_api::submit_cop!(DotSeparatedKeys);
