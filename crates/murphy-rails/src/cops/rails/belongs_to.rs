//! `Rails/BelongsTo` — flag `belongs_to ..., required: true/false` in favour of `optional:`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/BelongsTo
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:belongs_to] gating,
//!   the `(send _ :belongs_to ... (hash <(pair (sym :required) boolean) ...>))`
//!   shape (any receiver, trailing hash with a `required:` boolean pair),
//!   selector-only offense range, and pair-replacement autocorrect
//!   (`required: false` → `optional: true`, `required: true` →
//!   `optional: false`). `minimum_target_rails_version 5.0` is not gated:
//!   Murphy fires regardless of the configured Rails version.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` with method `belongs_to` (any receiver, any args) whose arguments
//! include a `Hash` containing a `Pair` with key `sym :required` and a
//! boolean (`true`/`false`) value:
//!
//! - `belongs_to :blog, required: false` → `belongs_to :blog, optional: true`
//! - `belongs_to :blog, required: true` → `belongs_to :blog, optional: false`
//!
//! A non-boolean `required:` value (e.g. `required: x`) does not flag —
//! mirrors upstream's `$boolean` capture.
//!
//! ## Autocorrect
//!
//! Replace the `required: <bool>` pair only; the selector, association name,
//! and surrounding options pass through untouched.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BelongsTo;

#[cop(
    name = "Rails/BelongsTo",
    description = "Use `optional: true` instead of `required: false` on `belongs_to`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl BelongsTo {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[belongs_to]`.
    #[on_node(kind = "send", methods = ["belongs_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive: dispatcher guarantees Send.
        if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
            return;
        }
        let Some((pair_id, optional_false)) = find_required_pair(cx, node) else {
            return;
        };
        let (msg, replacement) = if optional_false {
            (
                "You specified `required: true`, in Rails > 5.0 the required option is deprecated and you want to use `optional: false`. In most configurations, this is the default and you can omit this option altogether",
                "optional: false",
            )
        } else {
            (
                "You specified `required: false`, in Rails > 5.0 the required option is deprecated and you want to use `optional: true`.",
                "optional: true",
            )
        };
        // Upstream offense range is `node.loc.selector`.
        cx.emit_offense(cx.loc(node).name, msg, None);
        cx.emit_edit(cx.range(pair_id), replacement);
    }
}

/// Scan call arguments for a `Hash` containing a `required:` pair with a
/// boolean value. Returns the pair node plus whether the value is `true`.
/// Mirrors upstream's `(hash <(pair (sym :required) boolean) ...>)`.
fn find_required_pair(cx: &Cx<'_>, node: NodeId) -> Option<(NodeId, bool)> {
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
            if cx.symbol_str(key_sym) != "required" {
                continue;
            }
            if matches!(*cx.kind(value), NodeKind::True_) {
                return Some((pair_id, true));
            }
            if matches!(*cx.kind(value), NodeKind::False_) {
                return Some((pair_id, false));
            }
            // Non-boolean `required:` value — upstream `$boolean` does not
            // capture; keep scanning (a later pair cannot re-match the same
            // key realistically, but stay permissive).
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::BelongsTo;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_required_false() {
        test::<BelongsTo>().expect_offense(indoc! {r#"
            belongs_to :blog, required: false
            ^^^^^^^^^^ You specified `required: false`, in Rails > 5.0 the required option is deprecated and you want to use `optional: true`.
        "#});
    }

    #[test]
    fn flags_required_true() {
        test::<BelongsTo>().expect_offense(indoc! {r#"
            belongs_to :blog, required: true
            ^^^^^^^^^^ You specified `required: true`, in Rails > 5.0 the required option is deprecated and you want to use `optional: false`. In most configurations, this is the default and you can omit this option altogether
        "#});
    }

    #[test]
    fn flags_required_among_other_options() {
        test::<BelongsTo>().expect_offense(indoc! {r#"
            belongs_to :blog, class_name: "Blog", required: false
            ^^^^^^^^^^ You specified `required: false`, in Rails > 5.0 the required option is deprecated and you want to use `optional: true`.
        "#});
    }

    #[test]
    fn does_not_flag_optional() {
        test::<BelongsTo>().expect_no_offenses("belongs_to :blog, optional: true\n");
    }

    #[test]
    fn does_not_flag_without_options() {
        test::<BelongsTo>().expect_no_offenses("belongs_to :blog\n");
    }

    #[test]
    fn does_not_flag_non_boolean_required() {
        test::<BelongsTo>().expect_no_offenses("belongs_to :blog, required: foo\n");
    }

    #[test]
    fn corrects_required_false() {
        test::<BelongsTo>()
            .expect_correction(
                indoc! {r#"
                    belongs_to :blog, required: false
                    ^^^^^^^^^^ You specified `required: false`, in Rails > 5.0 the required option is deprecated and you want to use `optional: true`.
                "#},
                "belongs_to :blog, optional: true\n",
            )
            .expect_no_offenses("belongs_to :blog, optional: true\n");
    }

    #[test]
    fn corrects_required_true() {
        test::<BelongsTo>()
            .expect_correction(
                indoc! {r#"
                    belongs_to :blog, required: true
                    ^^^^^^^^^^ You specified `required: true`, in Rails > 5.0 the required option is deprecated and you want to use `optional: false`. In most configurations, this is the default and you can omit this option altogether
                "#},
                "belongs_to :blog, optional: false\n",
            )
            .expect_no_offenses("belongs_to :blog, optional: false\n");
    }
}
murphy_plugin_api::submit_cop!(BelongsTo);
