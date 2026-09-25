//! `Rails/RedundantPresenceValidationOnBelongsTo` — redundant presence on `belongs_to`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantPresenceValidationOnBelongsTo
//! upstream_version_checked: 2.35.0
//! version_added: "2.13"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:validates] with
//!   sym keys plus a trailing hash containing `presence: true` (and no
//!   `strict: true`/const or `if:`/`unless:`), the
//!   only-validation-with-extra-options early return, sibling
//!   `belongs_to` lookup in the enclosing `begin` (by name or matching
//!   `foreign_key:`, `_id` suffix aware) with `optional: true` /
//!   `required: false` exemption, and the four autocorrect branches
//!   (drop validation, drop keys, drop presence pair, extract residual
//!   validation). `minimum_target_rails_version 5.0` is gated.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantPresenceValidationOnBelongsTo;

#[cop(
    name = "Rails/RedundantPresenceValidationOnBelongsTo",
    description = "Checks for redundant presence validation on belongs_to association.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantPresenceValidationOnBelongsTo {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["validates"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(5, 0) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }
    // Last arg must be a hash (options); leading args must all be syms.
    let Some(&options_hash) = args.last() else {
        return;
    };
    if !matches!(*cx.kind(options_hash), NodeKind::Hash(_)) {
        return;
    }
    let mut all_keys: Vec<String> = Vec::new();
    for &a in &args[..args.len() - 1] {
        if let NodeKind::Sym(s) = *cx.kind(a) {
            all_keys.push(cx.symbol_str(s).to_owned());
        } else {
            return;
        }
    }
    if all_keys.is_empty() {
        return;
    }
    let pairs = hash_pairs(cx, options_hash);
    let Some(presence_pair) = find_presence_true(cx, &pairs) else {
        return;
    };
    if has_strict_blocker(cx, &pairs) || has_conditional(cx, &pairs) {
        return;
    }
    // Upstream early return: presence is the only validation but other
    // non-validation options remain (removing it would leave an invalid
    // `validates` with no validation).
    if only_presence_with_extras(cx, &pairs) {
        return;
    }
    let Some(parent) = cx.parent(node).get() else {
        return;
    };
    let siblings = sibling_statements(cx, parent, node);
    if siblings.is_empty() {
        return;
    }
    let keys: Vec<String> = all_keys
        .iter()
        .filter(|k| has_non_optional_belongs_to(cx, &siblings, k))
        .cloned()
        .collect();
    if keys.is_empty() {
        return;
    }
    let display = keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join("/");
    cx.emit_offense(
        cx.range(presence_pair),
        &format!("Remove explicit presence validation for {display}."),
        None,
    );
    emit_correction(cx, node, &all_keys, &keys, options_hash, presence_pair);
}

fn hash_pairs(cx: &Cx<'_>, hash: NodeId) -> Vec<NodeId> {
    let NodeKind::Hash(list) = *cx.kind(hash) else {
        return Vec::new();
    };
    cx.list(list).to_vec()
}

fn pair_key_sym(cx: &Cx<'_>, pair: NodeId) -> Option<String> {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return None;
    };
    if let NodeKind::Sym(s) = *cx.kind(key) {
        Some(cx.symbol_str(s).to_owned())
    } else {
        None
    }
}

fn find_presence_true(cx: &Cx<'_>, pairs: &[NodeId]) -> Option<NodeId> {
    for &p in pairs {
        if pair_key_sym(cx, p).as_deref() != Some("presence") {
            continue;
        }
        let NodeKind::Pair { value, .. } = *cx.kind(p) else {
            continue;
        };
        if matches!(*cx.kind(value), NodeKind::True_) {
            return Some(p);
        }
    }
    None
}

/// `strict: true` or `strict: <const>` suppresses (upstream `$boolean` is
/// `true`, plus `const` for custom strict classes).
fn has_strict_blocker(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    for &p in pairs {
        if pair_key_sym(cx, p).as_deref() != Some("strict") {
            continue;
        }
        let NodeKind::Pair { value, .. } = *cx.kind(p) else {
            continue;
        };
        if matches!(*cx.kind(value), NodeKind::True_ | NodeKind::Const { .. }) {
            return true;
        }
    }
    false
}

fn has_conditional(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    pairs.iter().any(|&p| {
        matches!(
            pair_key_sym(cx, p).as_deref(),
            Some("if") | Some("unless")
        )
    })
}

fn only_presence_with_extras(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    const NON_VALIDATION: &[&str] = &["if", "unless", "on", "allow_blank", "allow_nil", "strict"];
    let mut used_sym_keys: Vec<String> = Vec::new();
    for &p in pairs {
        if let Some(k) = pair_key_sym(cx, p) {
            used_sym_keys.push(k);
        }
    }
    let remaining: Vec<&String> = used_sym_keys
        .iter()
        .filter(|k| !NON_VALIDATION.contains(&k.as_str()) && k.as_str() != "presence")
        .collect();
    remaining.is_empty() && pairs.len() > 1
}

/// Sibling statements in the enclosing `begin` (class body). Falls back to
/// the class/module body when the parent is the class node itself.
fn sibling_statements(cx: &Cx<'_>, parent: NodeId, _node: NodeId) -> Vec<NodeId> {
    match *cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        NodeKind::Class { body, .. } | NodeKind::Module { body, .. } => {
            if let Some(b) = body.get() {
                match *cx.kind(b) {
                    NodeKind::Begin(list) => cx.list(list).to_vec(),
                    _ => vec![b],
                }
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn has_non_optional_belongs_to(cx: &Cx<'_>, siblings: &[NodeId], key: &str) -> bool {
    let Some(target) = belongs_to_for(cx, siblings, key) else {
        return false;
    };
    !is_optional(cx, target)
}

fn belongs_to_for(cx: &Cx<'_>, siblings: &[NodeId], key: &str) -> Option<NodeId> {
    if let Some(base) = key.strip_suffix("_id") {
        // By foreign key: `belongs_to :author, foreign_key: :user_id` or
        // plain `belongs_to :user` (without_fk with the base name).
        for &s in siblings {
            if !is_belongs_to_call(cx, s) {
                continue;
            }
            if belongs_to_without_fk(cx, s, base) || belongs_to_with_matching_fk(cx, s, key) {
                return Some(s);
            }
        }
        None
    } else {
        // By association name, regardless of `foreign_key`.
        for &s in siblings {
            if !is_belongs_to_call(cx, s) {
                continue;
            }
            if belongs_to_first_arg(cx, s).as_deref() == Some(key) {
                return Some(s);
            }
        }
        None
    }
}

fn is_belongs_to_call(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(*cx.kind(id), NodeKind::Send { .. })
        && cx.method_name(id) == Some("belongs_to")
        && cx.call_receiver(id).get().is_none()
}

fn belongs_to_first_arg(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    let args = cx.call_arguments(id);
    let first = *args.first()?;
    if let NodeKind::Sym(s) = *cx.kind(first) {
        Some(cx.symbol_str(s).to_owned())
    } else {
        None
    }
}

/// No hash arg contains a `foreign_key` pair, and the first arg matches.
fn belongs_to_without_fk(cx: &Cx<'_>, id: NodeId, key: &str) -> bool {
    if belongs_to_first_arg(cx, id).as_deref() != Some(key) {
        return false;
    }
    for &a in cx.call_arguments(id) {
        if !matches!(*cx.kind(a), NodeKind::Hash(_)) {
            continue;
        }
        for &p in &hash_pairs(cx, a) {
            if pair_key_sym(cx, p).as_deref() == Some("foreign_key") {
                return false;
            }
        }
    }
    true
}

/// Any hash arg has `foreign_key: <fk>` with a sym value.
fn belongs_to_with_matching_fk(cx: &Cx<'_>, id: NodeId, fk: &str) -> bool {
    for &a in cx.call_arguments(id) {
        if !matches!(*cx.kind(a), NodeKind::Hash(_)) {
            continue;
        }
        for &p in &hash_pairs(cx, a) {
            if pair_key_sym(cx, p).as_deref() != Some("foreign_key") {
                continue;
            }
            let NodeKind::Pair { value, .. } = *cx.kind(p) else {
                continue;
            };
            if let NodeKind::Sym(s) = *cx.kind(value)
                && cx.symbol_str(s) == fk
            {
                return true;
            }
        }
    }
    false
}

fn is_optional(cx: &Cx<'_>, belongs_to: NodeId) -> bool {
    for &a in cx.call_arguments(belongs_to) {
        if !matches!(*cx.kind(a), NodeKind::Hash(_)) {
            continue;
        }
        for &p in &hash_pairs(cx, a) {
            let Some(k) = pair_key_sym(cx, p) else {
                continue;
            };
            let NodeKind::Pair { value, .. } = *cx.kind(p) else {
                continue;
            };
            if k == "optional" && matches!(*cx.kind(value), NodeKind::True_) {
                return true;
            }
            if k == "required" && matches!(*cx.kind(value), NodeKind::False_) {
                return true;
            }
        }
    }
    false
}

fn emit_correction(
    cx: &Cx<'_>,
    node: NodeId,
    all_keys: &[String],
    keys: &[String],
    options_hash: NodeId,
    presence_pair: NodeId,
) {
    let pairs = hash_pairs(cx, options_hash);
    if pairs.len() == 1 {
        // `presence: true` is the only option.
        if keys == all_keys {
            cx.emit_edit(validation_range(cx, node), "");
        } else {
            for k in keys {
                if let Some(key_node) = find_key_arg(cx, node, k) {
                    cx.emit_edit(key_removal_range(cx, key_node), "");
                }
            }
        }
    } else if keys == all_keys {
        cx.emit_edit(removal_range(cx, cx.range(presence_pair)), "");
    } else {
        // Extract residual validation for the redundant keys.
        let residual: Vec<String> = pairs
            .iter()
            .filter(|&&p| pair_key_sym(cx, p).as_deref() != Some("presence"))
            .map(|&p| cx.raw_source(cx.range(p)).to_owned())
            .collect();
        let indent = " ".repeat(indent_of(cx, node) as usize);
        let src = format!(
            "{indent}validates {}{}\n",
            keys.iter().map(|k| format!(":{k}")).collect::<Vec<_>>().join(", "),
            if residual.is_empty() {
                String::new()
            } else {
                format!(", {}", residual.join(", "))
            }
        );
        for k in keys {
            if let Some(key_node) = find_key_arg(cx, node, k) {
                cx.emit_edit(key_removal_range(cx, key_node), "");
            }
        }
        let end = validation_range(cx, node).end;
        cx.emit_edit(Range { start: end, end }, &src);
    }
}

fn find_key_arg(cx: &Cx<'_>, validates: NodeId, key: &str) -> Option<NodeId> {
    for &a in cx.call_arguments(validates) {
        if let NodeKind::Sym(s) = *cx.kind(a)
            && cx.symbol_str(s) == key
        {
            return Some(a);
        }
    }
    None
}

/// Whole-line removal for a `validates` statement (upstream
/// `range_by_whole_lines(..., include_final_newline: true)`).
fn validation_range(cx: &Cx<'_>, node: NodeId) -> Range {
    cx.range_by_whole_lines(cx.range(node), true)
}

fn indent_of(cx: &Cx<'_>, node: NodeId) -> u32 {
    let start = cx.range(node).start as usize;
    let src = cx.source().as_bytes();
    let mut col = 0u32;
    let mut i = start;
    while i > 0 && src[i - 1] != b'\n' {
        i -= 1;
        if src[i] == b' ' || src[i] == b'\t' {
            col += 1;
        } else {
            // Non-blank before start on this line — recompute from line start.
            col = 0;
            let mut j = i;
            while j < start && (src[j] == b' ' || src[j] == b'\t') {
                col += 1;
                j += 1;
            }
            break;
        }
    }
    // Simpler: count spaces from line start.
    let mut line_start = start;
    while line_start > 0 && src[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    let mut indent = 0u32;
    while (line_start + indent as usize) < src.len()
        && (src[line_start + indent as usize] == b' '
            || src[line_start + indent as usize] == b'\t')
    {
        indent += 1;
    }
    let _ = col;
    indent
}

fn key_removal_range(cx: &Cx<'_>, key_node: NodeId) -> Range {
    removal_range(cx, cx.range(key_node))
}

fn removal_range(cx: &Cx<'_>, target: Range) -> Range {
    let full = cx.source().as_bytes();
    let mut start = target.start as usize;
    while start > 0 && (full[start - 1] == b' ' || full[start - 1] == b'\t') {
        start -= 1;
    }
    if start > 0 && full[start - 1] == b',' {
        start -= 1;
        while start > 0 && (full[start - 1] == b' ' || full[start - 1] == b'\t') {
            start -= 1;
        }
        return Range {
            start: start as u32,
            end: target.end,
        };
    }
    let mut end = target.end as usize;
    let mut tmp = end;
    while tmp < full.len() && (full[tmp] == b' ' || full[tmp] == b'\t') {
        tmp += 1;
    }
    if tmp < full.len() && full[tmp] == b',' {
        end = tmp + 1;
        while end < full.len() && (full[end] == b' ' || full[end] == b'\t') {
            end += 1;
        }
        return Range {
            start: target.start,
            end: end as u32,
        };
    }
    Range {
        start: start as u32,
        end: end as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantPresenceValidationOnBelongsTo;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_by_association() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_offense(indoc! {r#"
            class User
              belongs_to :account
              validates :account, presence: true
                                  ^^^^^^^^^^^^^^ Remove explicit presence validation for `account`.
            end
        "#});
    }

    #[test]
    fn flags_by_foreign_key() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_offense(indoc! {r#"
            class User
              belongs_to :author, foreign_key: :user_id
              validates :user_id, presence: true
                                  ^^^^^^^^^^^^^^ Remove explicit presence validation for `user_id`.
            end
        "#});
    }

    #[test]
    fn allows_optional() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_no_offenses(indoc! {r#"
            class User
              belongs_to :account, optional: true
              validates :account, presence: true
            end
        "#});
    }

    #[test]
    fn allows_if_condition() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_no_offenses(indoc! {r#"
            class User
              belongs_to :account
              validates :account, presence: true, if: :active?
            end
        "#});
    }

    #[test]
    fn allows_other_validations_only() {
        // Presence is the only validation but extra non-validation
        // options remain → removing it would leave an invalid validates.
        test::<RedundantPresenceValidationOnBelongsTo>().expect_no_offenses(indoc! {r#"
            class User
              belongs_to :account
              validates :account, presence: true, allow_blank: true
            end
        "#});
    }

    #[test]
    fn corrects_drop_validation() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_correction(
            indoc! {r#"
                class User
                  belongs_to :account
                  validates :account, presence: true
                                      ^^^^^^^^^^^^^^ Remove explicit presence validation for `account`.
                end
            "#},
            "class User\n  belongs_to :account\nend\n",
        );
    }

    #[test]
    fn corrects_drop_presence_pair() {
        test::<RedundantPresenceValidationOnBelongsTo>().expect_correction(
            indoc! {r#"
                class User
                  belongs_to :account
                  validates :account, presence: true, uniqueness: true
                                      ^^^^^^^^^^^^^^ Remove explicit presence validation for `account`.
                end
            "#},
            "class User\n  belongs_to :account\n  validates :account, uniqueness: true\nend\n",
        );
    }

    #[test]
    fn gated_below_rails_50() {
        test::<RedundantPresenceValidationOnBelongsTo>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses(indoc! {r#"
                class User
                  belongs_to :account
                  validates :account, presence: true
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantPresenceValidationOnBelongsTo);
