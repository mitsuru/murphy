//! `Rails/RedundantAllowNil` — `allow_nil` is redundant when `allow_blank` covers it.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantAllowNil
//! upstream_version_checked: 2.35.0
//! version_added: "0.67"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:validates],
//!   recursive `allow_nil`/`allow_blank` pair search, same-type
//!   (`MSG_SAME`) vs `allow_nil: false` + `allow_blank: true`
//!   (`MSG_ALLOW_NIL_FALSE`) messaging, pair-only offense with
//!   surrounding-comma removal autocorrect. Upstream Include gating
//!   (`**/app/models/**/*.rb`) has no file-path infrastructure in
//!   Murphy yet, so the cop fires in all files.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantAllowNil;

#[cop(
    name = "Rails/RedundantAllowNil",
    description = "Checks Rails model validations for a redundant `allow_nil` when `allow_blank` is present.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantAllowNil {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["validates"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let (Some(allow_nil), Some(allow_blank)) = find_pairs(cx, node) else {
        return;
    };
    let nil_val = pair_value(cx, allow_nil);
    let blank_val = pair_value(cx, allow_blank);
    let (Some(nil_id), Some(blank_id)) = (nil_val, blank_val) else {
        return;
    };
    if same_node_type(cx, nil_id, blank_id) {
        cx.emit_offense(
            cx.range(allow_nil),
            "`allow_nil` is redundant when `allow_blank` has the same value.",
            None,
        );
        emit_remove(cx, allow_nil);
    } else if matches!(*cx.kind(nil_id), NodeKind::False_)
        && matches!(*cx.kind(blank_id), NodeKind::True_)
    {
        cx.emit_offense(
            cx.range(allow_nil),
            "`allow_nil: false` is redundant when `allow_blank` is true.",
            None,
        );
        emit_remove(cx, allow_nil);
    }
}

/// Upstream `find_allow_nil_and_allow_blank`: depth-first search for the
/// two pairs anywhere under the `validates` node.
fn find_pairs(cx: &Cx<'_>, node: NodeId) -> (Option<NodeId>, Option<NodeId>) {
    let mut allow_nil = None;
    let mut allow_blank = None;
    // Direct scan first (common case), then recursive descent.
    search(cx, node, &mut allow_nil, &mut allow_blank);
    (allow_nil, allow_blank)
}

fn search(cx: &Cx<'_>, id: NodeId, nil_out: &mut Option<NodeId>, blank_out: &mut Option<NodeId>) {
    if nil_out.is_some() && blank_out.is_some() {
        return;
    }
    if let NodeKind::Pair { key, .. } = *cx.kind(id)
        && let NodeKind::Sym(s) = *cx.kind(key)
    {
        let name = cx.symbol_str(s);
        if name == "allow_nil" && nil_out.is_none() {
            *nil_out = Some(id);
        } else if name == "allow_blank" && blank_out.is_none() {
            *blank_out = Some(id);
        }
        if nil_out.is_some() && blank_out.is_some() {
            return;
        }
    }
    for child in cx.children(id) {
        search(cx, child, nil_out, blank_out);
        if nil_out.is_some() && blank_out.is_some() {
            return;
        }
    }
}

fn pair_value(cx: &Cx<'_>, pair: NodeId) -> Option<NodeId> {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return None;
    };
    Some(value)
}

/// Upstream `allow_nil_val.type == allow_blank_val.type`: same node-type.
/// RuboCop compares parser-gem types (e.g. both `str`), not values, so
/// `allow_nil: "a", allow_blank: "b"` still reports `MSG_SAME`.
fn same_node_type(cx: &Cx<'_>, a: NodeId, b: NodeId) -> bool {
    std::mem::discriminant(cx.kind(a)) == std::mem::discriminant(cx.kind(b))
}

fn emit_remove(cx: &Cx<'_>, pair: NodeId) {
    cx.emit_edit(removal_range(cx, cx.range(pair)), "");
}

/// Left-biased comma-aware removal (mirrors `range_with_surrounding_space`
/// + `range_with_surrounding_comma` on the left, with leading-pair fallback).
fn removal_range(cx: &Cx<'_>, pair_range: Range) -> Range {
    let full = cx.source().as_bytes();
    let mut start = pair_range.start as usize;
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
            end: pair_range.end,
        };
    }
    let mut end = pair_range.end as usize;
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
            start: pair_range.start,
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
    use super::RedundantAllowNil;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_same_true() {
        test::<RedundantAllowNil>().expect_offense(indoc! {r#"
            validates :x, length: { is: 5 }, allow_nil: true, allow_blank: true
                                             ^^^^^^^^^^^^^^^ `allow_nil` is redundant when `allow_blank` has the same value.
        "#});
    }

    #[test]
    fn flags_same_false() {
        test::<RedundantAllowNil>().expect_offense(indoc! {r#"
            validates :x, length: { is: 5 }, allow_nil: false, allow_blank: false
                                             ^^^^^^^^^^^^^^^^ `allow_nil` is redundant when `allow_blank` has the same value.
        "#});
    }

    #[test]
    fn flags_nil_false_blank_true() {
        test::<RedundantAllowNil>().expect_offense(indoc! {r#"
            validates :x, length: { is: 5 }, allow_nil: false, allow_blank: true
                                             ^^^^^^^^^^^^^^^^ `allow_nil: false` is redundant when `allow_blank` is true.
        "#});
    }

    #[test]
    fn allows_nil_true_blank_false() {
        test::<RedundantAllowNil>().expect_no_offenses(
            "validates :x, length: { is: 5 }, allow_nil: true, allow_blank: false\n",
        );
    }

    #[test]
    fn allows_only_allow_nil() {
        test::<RedundantAllowNil>()
            .expect_no_offenses("validates :x, length: { is: 5 }, allow_nil: true\n");
    }

    #[test]
    fn corrects_same_true() {
        test::<RedundantAllowNil>().expect_correction(
            indoc! {r#"
                validates :x, length: { is: 5 }, allow_nil: true, allow_blank: true
                                                 ^^^^^^^^^^^^^^^ `allow_nil` is redundant when `allow_blank` has the same value.
            "#},
            "validates :x, length: { is: 5 }, allow_blank: true\n",
        );
    }

    #[test]
    fn corrects_nil_false_blank_true() {
        test::<RedundantAllowNil>().expect_correction(
            indoc! {r#"
                validates :x, length: { is: 5 }, allow_nil: false, allow_blank: true
                                                 ^^^^^^^^^^^^^^^^ `allow_nil: false` is redundant when `allow_blank` is true.
            "#},
            "validates :x, length: { is: 5 }, allow_blank: true\n",
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantAllowNil);
