//! `Rails/RedirectBackOrTo` — prefer `redirect_back_or_to` over `redirect_back`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedirectBackOrTo
//! upstream_version_checked: 2.35.0
//! version_added: "2.34"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:redirect_back]
//!   with a bare receiver and a single hash argument containing a
//!   `fallback_location:` pair, selector-only offense, selector rename
//!   plus hash-to-positional rewrite (`{fallback: x}` → `(x)`,
//!   multi-pair fallback extraction + insertion before the first
//!   remaining pair), and parenthesis insertion for bare calls.
//!   `minimum_target_rails_version 7.0` is gated (unset means newest).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedirectBackOrTo;

#[cop(
    name = "Rails/RedirectBackOrTo",
    description = "Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedirectBackOrTo {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["redirect_back"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `minimum_target_rails_version 7.0` — unset means newest.
    if !cx.rails_version_at_least(7, 0) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let NodeKind::Hash(list) = *cx.kind(args[0]) else {
        return;
    };
    let pairs = cx.list(list).to_vec();
    let mut fallback_pair = None;
    let mut fallback_value = None;
    for &p in &pairs {
        let NodeKind::Pair { key, value } = *cx.kind(p) else {
            continue;
        };
        if let NodeKind::Sym(s) = *cx.kind(key)
            && cx.symbol_str(s) == "fallback_location"
        {
            fallback_pair = Some(p);
            fallback_value = Some(value);
            break;
        }
    }
    let (Some(fpair), Some(fval)) = (fallback_pair, fallback_value) else {
        return;
    };
    cx.emit_offense(
        cx.selector(node),
        "Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.",
        None,
    );
    // Selector rename.
    cx.emit_edit(cx.selector(node), "redirect_back_or_to");
    let fallback_src = cx.raw_source(cx.range(fval)).to_owned();
    let hash_range = cx.range(args[0]);
    if pairs.len() == 1 {
        // Single-pair hash → positional arg.
        cx.emit_edit(hash_range, &fallback_src);
    } else {
        // Multi-pair: remove the fallback pair, insert its value before
        // the first remaining pair.
        let first_remaining = pairs.iter().copied().find(|&p| p != fpair);
        cx.emit_edit(removal_range(cx, cx.range(fpair)), "");
        if let Some(first) = first_remaining {
            let pos = cx.range(first).start;
            cx.emit_edit(
                Range { start: pos, end: pos },
                &format!("{fallback_src}, "),
            );
        }
    }
    // Bare calls (`redirect_back fallback_location: x`) need parens.
    if !cx.is_parenthesized(node) {
        let sel = cx.selector(node);
        cx.emit_edit(
            Range {
                start: sel.end,
                end: hash_range.start,
            },
            "(",
        );
        let end = cx.range(node).end;
        cx.emit_edit(Range { start: end, end }, ")");
    }
}

/// Left-biased comma-aware removal for the fallback pair.
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
    use super::RedirectBackOrTo;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_fallback() {
        test::<RedirectBackOrTo>().expect_offense(indoc! {r#"
            redirect_back(fallback_location: root_path)
            ^^^^^^^^^^^^^ Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.
        "#});
    }

    #[test]
    fn corrects_single_fallback() {
        test::<RedirectBackOrTo>().expect_correction(
            indoc! {r#"
                redirect_back(fallback_location: root_path)
                ^^^^^^^^^^^^^ Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.
            "#},
            "redirect_back_or_to(root_path)\n",
        );
    }

    #[test]
    fn corrects_with_options() {
        test::<RedirectBackOrTo>().expect_correction(
            indoc! {r#"
                redirect_back(fallback_location: root_path, allow_other_host: false)
                ^^^^^^^^^^^^^ Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.
            "#},
            "redirect_back_or_to(root_path, allow_other_host: false)\n",
        );
    }

    #[test]
    fn corrects_bare_call() {
        test::<RedirectBackOrTo>().expect_correction(
            indoc! {r#"
                redirect_back fallback_location: root_path
                ^^^^^^^^^^^^^ Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.
            "#},
            "redirect_back_or_to(root_path)\n",
        );
    }

    #[test]
    fn allows_no_fallback() {
        test::<RedirectBackOrTo>()
            .expect_no_offenses("redirect_back(allow_other_host: false)\n");
    }

    #[test]
    fn allows_receiver() {
        test::<RedirectBackOrTo>()
            .expect_no_offenses("obj.redirect_back(fallback_location: root_path)\n");
    }

    #[test]
    fn gated_below_rails_70() {
        test::<RedirectBackOrTo>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses("redirect_back(fallback_location: root_path)\n");
    }
}
murphy_plugin_api::submit_cop!(RedirectBackOrTo);
