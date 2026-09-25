//! `Rails/CompactBlank` — flag `reject(&:blank?)` / `select(&:present?)` (and block forms) in favour of `compact_blank`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/CompactBlank
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [reject delete_if select filter keep_if] gating, block-pass and single /
//!   two-argument block shapes with polarity (reject/delete_if ↔ blank?,
//!   select/filter/keep_if ↔ present?), the selector-to-block-end offense
//!   range, and `compact_blank` / `compact_blank!` (destructive) autocorrect.
//!   Not gated: `minimum_target_rails_version 6.1` and the Ruby 2.6 `filter`
//!   guard — Murphy fires regardless of configured versions. Only `block`
//!   nodes match (upstream `block` pattern excludes numblock/itblock).
//! ```
//!
//! ## Matched shapes (Send node)
//!
//! `Send` with method `reject` / `delete_if` / `select` / `filter` /
//! `keep_if`, in either form:
//!
//! - block-pass: `collection.reject(&:blank?)` → `collection.compact_blank`
//! - block: `collection.reject { |x| x.blank? }` → `collection.compact_blank`
//! - hash block: `collection.reject { |_k, v| v.blank? }` → `collection.compact_blank`
//!
//! Destructive `delete_if` / `keep_if` rewrite to `compact_blank!` instead.
//! The block body must be a bare `blank?` / `present?` send on an `lvar`
//! matching the single block argument (or the second of two, for hashes).
//!
//! ## Offense range and autocorrect
//!
//! Offense spans the selector through the end of the parent block when the
//! send is a block call (`reject { }`), else through the end of the send
//! (`reject(&:blank?)`); autocorrect replaces that range with the preferred
//! method name.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct CompactBlank;

#[cop(
    name = "Rails/CompactBlank",
    description = "Use `compact_blank` instead of `reject(&:blank?)`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl CompactBlank {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(
        kind = "send",
        methods = ["reject", "delete_if", "select", "filter", "keep_if"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(method) = cx.method_name(node) else {
            return;
        };
        // Polarity gate: reject/delete_if ↔ blank?, select/filter/keep_if ↔ present?.
        let blank_side = matches!(method, "reject" | "delete_if");
        let wanted = if blank_side { "blank?" } else { "present?" };
        if !is_bad_method(cx, node, wanted) {
            return;
        }
        let preferred = if matches!(method, "delete_if" | "keep_if") {
            "compact_blank!"
        } else {
            "compact_blank"
        };
        cx.emit_offense(
            offense_range(cx, node),
            &format!("Use `{preferred}` instead."),
            None,
        );
        cx.emit_edit(offense_range(cx, node), preferred);
    }
}

/// Mirrors upstream `bad_method?`: a block-pass `(&:blank?/:present?)` with
/// matching polarity, or a parent `block` whose body is an `lvar`-receiver
/// `blank?`/`present?` send over the single (or hash-value) block argument.
fn is_bad_method(cx: &Cx<'_>, node: NodeId, wanted: &str) -> bool {
    if has_block_pass(cx, node, wanted) {
        return true;
    }
    let Some(block) = cx.block_node(node).get() else {
        return false;
    };
    let NodeKind::Block { args, body, .. } = *cx.kind(block) else {
        return false;
    };
    let Some(body) = body.get() else {
        return false;
    };
    // Body must be exactly `(send (lvar _) :blank?/:present?)` with no args.
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(body)
    else {
        return false;
    };
    if cx.symbol_str(method) != wanted {
        return false;
    }
    if !cx.call_arguments(body).is_empty() {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    if !matches!(*cx.kind(recv), NodeKind::Lvar(_)) {
        return false;
    }
    let recv_src = cx.raw_source(cx.range(recv));
    // Block args: one (`|x|`) matches the receiver; two (`|_k, v|`, hash
    // form) matches on the second.
    let NodeKind::Args(params) = *cx.kind(args) else {
        return false;
    };
    let params = cx.list(params);
    match params.len() {
        1 => cx.raw_source(cx.range(params[0])) == recv_src,
        2 => cx.raw_source(cx.range(params[1])) == recv_src,
        _ => false,
    }
}

/// `collection.reject(&:blank?)` — the last argument is a `BlockPass` over
/// the polarity-matching predicate symbol.
fn has_block_pass(cx: &Cx<'_>, node: NodeId, wanted: &str) -> bool {
    let args = cx.call_arguments(node);
    let Some(&last) = args.last() else {
        return false;
    };
    let NodeKind::BlockPass(inner) = *cx.kind(last) else {
        return false;
    };
    let Some(sym_node) = inner.get() else {
        return false;
    };
    if !matches!(*cx.kind(sym_node), NodeKind::Sym(_)) {
        return false;
    }
    let NodeKind::Sym(sym) = *cx.kind(sym_node) else {
        return false;
    };
    cx.symbol_str(sym) == wanted
}

/// Selector through parent-block end when the send heads a block, else
/// through the send end. Mirrors upstream `offense_range`.
fn offense_range(cx: &Cx<'_>, node: NodeId) -> Range {
    let start = cx.loc(node).name.start;
    let end = match cx.block_node(node).get() {
        Some(block) => cx.range(block).end,
        None => cx.range(node).end,
    };
    Range { start, end }
}

#[cfg(test)]
mod tests {
    use super::CompactBlank;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_reject_block_pass() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.reject(&:blank?)
                       ^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn flags_select_block_pass() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.select(&:present?)
                       ^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn flags_filter_block_pass() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.filter(&:present?)
                       ^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn flags_delete_if_block_pass() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.delete_if(&:blank?)
                       ^^^^^^^^^^^^^^^^^^^ Use `compact_blank!` instead.
        "#});
    }

    #[test]
    fn flags_keep_if_block_pass() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.keep_if(&:present?)
                       ^^^^^^^^^^^^^^^^^^^ Use `compact_blank!` instead.
        "#});
    }

    #[test]
    fn flags_reject_block() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.reject { |x| x.blank? }
                       ^^^^^^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn flags_reject_hash_block() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.reject { |_k, v| v.blank? }
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn flags_select_block() {
        test::<CompactBlank>().expect_offense(indoc! {r#"
            collection.select { |x| x.present? }
                       ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
        "#});
    }

    #[test]
    fn does_not_flag_wrong_polarity() {
        // `reject(&:present?)` keeps present elements — not compact_blank.
        test::<CompactBlank>().expect_no_offenses("collection.reject(&:present?)\n");
        test::<CompactBlank>().expect_no_offenses("collection.select(&:blank?)\n");
    }

    #[test]
    fn does_not_flag_mismatched_block_arg() {
        test::<CompactBlank>()
            .expect_no_offenses("collection.reject { |x| y.blank? }\n");
    }

    #[test]
    fn does_not_flag_other_methods() {
        test::<CompactBlank>().expect_no_offenses("collection.map(&:blank?)\n");
    }

    #[test]
    fn corrects_reject_block_pass() {
        test::<CompactBlank>()
            .expect_correction(
                indoc! {r#"
                    collection.reject(&:blank?)
                               ^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
                "#},
                "collection.compact_blank\n",
            )
            .expect_no_offenses("collection.compact_blank\n");
    }

    #[test]
    fn corrects_delete_if_block_pass() {
        test::<CompactBlank>()
            .expect_correction(
                indoc! {r#"
                    collection.delete_if(&:blank?)
                               ^^^^^^^^^^^^^^^^^^^ Use `compact_blank!` instead.
                "#},
                "collection.compact_blank!\n",
            )
            .expect_no_offenses("collection.compact_blank!\n");
    }

    #[test]
    fn corrects_reject_block() {
        test::<CompactBlank>()
            .expect_correction(
                indoc! {r#"
                    collection.reject { |x| x.blank? }
                               ^^^^^^^^^^^^^^^^^^^^^^^ Use `compact_blank` instead.
                "#},
                "collection.compact_blank\n",
            )
            .expect_no_offenses("collection.compact_blank\n");
    }
}
murphy_plugin_api::submit_cop!(CompactBlank);
