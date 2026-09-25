//! `Rails/IgnoredSkipActionFilterOption` — `if`/`only`/`except` misuse.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/IgnoredSkipActionFilterOption
//! upstream_version_checked: 2.35.0
//! version_added: "0.63"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [skip_after_action skip_around_action skip_before_action
//!   skip_action_callback], last-arg hash with sym keys, `if`+`only` flags
//!   the `if:` pair, `if`+`except` flags the `except:` pair, autocorrect
//!   removes the ignored pair (with surrounding comma/space). Offense range
//!   is the ignored pair node. Upstream Include (controllers/mailers) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//!   default.yml, murphy-4gd.1.15).
//! ```
//!
//! Checks that `if` and `only` (or `except`) are not used together as
//! options of `skip_*` action filter.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IgnoredSkipActionFilterOption;

#[cop(
    name = "Rails/IgnoredSkipActionFilterOption",
    description = "Checks that `if` and `only` (or `except`) are not used together as options of `skip_*` action filter.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IgnoredSkipActionFilterOption {
    #[on_node(
        kind = "send",
        methods = [
            "skip_after_action",
            "skip_around_action",
            "skip_before_action",
            "skip_action_callback"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        if receiver.get().is_some() {
            return;
        }
        let m = cx.symbol_str(method);
        if !matches!(
            m,
            "skip_after_action"
                | "skip_around_action"
                | "skip_before_action"
                | "skip_action_callback"
        ) {
            return;
        }
        let args = cx.call_arguments(node);
        let Some(&last) = args.last() else {
            return;
        };
        // Upstream captures `$_` (last arg) then requires hash_type.
        let NodeKind::Hash(pairs) = *cx.kind(last) else {
            return;
        };
        let _ = pairs;
        let map = options_map(cx, last);
        if map.contains_key("if") && map.contains_key("only") {
            let pair = map["if"];
            cx.emit_offense(
                cx.range(pair),
                "`if` option will be ignored when `only` and `if` are used together.",
                None,
            );
            emit_remove_pair(cx, node, pair);
        } else if map.contains_key("if") && map.contains_key("except") {
            let pair = map["except"];
            cx.emit_offense(
                cx.range(pair),
                "`except` option will be ignored when `if` and `except` are used together.",
                None,
            );
            emit_remove_pair(cx, node, pair);
        }
    }
}

fn options_map<'a>(cx: &'a Cx<'a>, hash: NodeId) -> std::collections::HashMap<&'a str, NodeId> {
    let mut out = std::collections::HashMap::new();
    let NodeKind::Hash(pairs) = *cx.kind(hash) else {
        return out;
    };
    for &p in cx.list(pairs) {
        let NodeKind::Pair { key, .. } = *cx.kind(p) else {
            continue;
        };
        if let NodeKind::Sym(k) = *cx.kind(key) {
            out.insert(cx.symbol_str(k), p);
        }
    }
    out
}

/// Remove the ignored pair with surrounding comma/space, mirroring
/// upstream `range_with_surrounding_comma(range_with_surrounding_space(...,
/// :left), :left)`.
fn emit_remove_pair(cx: &Cx<'_>, _send: NodeId, pair: NodeId) {
    let pair_range = cx.range(pair);
    let src = cx.raw_source(cx.range(pair));
    let _ = src;
    // Expand left over spaces, then over a comma if present; otherwise
    // expand right over comma+spaces (for leading-pair case).
    let remove = removal_range(cx, pair_range);
    cx.emit_edit(remove, "");
}

fn removal_range(cx: &Cx<'_>, pair_range: Range) -> Range {
    let full = cx.source().as_bytes();
    let mut start = pair_range.start as usize;
    // Walk left over spaces/tabs.
    while start > 0 && (full[start - 1] == b' ' || full[start - 1] == b'\t') {
        start -= 1;
    }
    // If directly preceded by comma, include it plus any spaces before it.
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
    // Leading-pair case: remove trailing comma + spaces.
    let mut end = pair_range.end as usize;
    let mut tmp = end;
    while tmp < full.len() && (full[tmp] == b' ' || full[tmp] == b'\t') {
        tmp += 1;
    }
    if tmp < full.len() && full[tmp] == b',' {
        end = tmp + 1;
        // Do not consume newline; only spaces/tabs after comma to avoid
        // joining lines. Upstream removes with surrounding space left-side,
        // but leading case needs right-side comma removal.
        while end < full.len() && (full[end] == b' ' || full[end] == b'\t') {
            end += 1;
        }
        return Range {
            start: pair_range.start,
            end: end as u32,
        };
    }
    // Fallback: original pair range with left spaces already consumed.
    Range {
        start: start as u32,
        end: end as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::IgnoredSkipActionFilterOption;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_if_and_only() {
        test::<IgnoredSkipActionFilterOption>().expect_offense(indoc! {r#"
            skip_before_action :login_required, only: :show, if: :trusted_origin?
                                                             ^^^^^^^^^^^^^^^^^^^^ `if` option will be ignored when `only` and `if` are used together.
        "#});
    }

    #[test]
    fn corrects_if_and_only() {
        test::<IgnoredSkipActionFilterOption>().expect_correction(
            indoc! {r#"
                skip_before_action :login_required, only: :show, if: :trusted_origin?
                                                                 ^^^^^^^^^^^^^^^^^^^^ `if` option will be ignored when `only` and `if` are used together.
            "#},
            "skip_before_action :login_required, only: :show\n",
        );
    }

    #[test]
    fn flags_multiple_actions() {
        test::<IgnoredSkipActionFilterOption>().expect_offense(indoc! {r#"
            skip_before_action :login_required, :another_action, only: :show, if: :trusted_origin?
                                                                              ^^^^^^^^^^^^^^^^^^^^ `if` option will be ignored when `only` and `if` are used together.
        "#});
    }

    #[test]
    fn flags_if_and_except() {
        test::<IgnoredSkipActionFilterOption>().expect_offense(indoc! {r#"
            skip_before_action :login_required, except: :admin, if: :trusted_origin?
                                                ^^^^^^^^^^^^^^ `except` option will be ignored when `if` and `except` are used together.
        "#});
    }

    #[test]
    fn corrects_if_and_except() {
        test::<IgnoredSkipActionFilterOption>().expect_correction(
            indoc! {r#"
                skip_before_action :login_required, except: :admin, if: :trusted_origin?
                                                    ^^^^^^^^^^^^^^ `except` option will be ignored when `if` and `except` are used together.
            "#},
            "skip_before_action :login_required, if: :trusted_origin?\n",
        );
    }

    #[test]
    fn does_not_flag_if_only() {
        test::<IgnoredSkipActionFilterOption>().expect_no_offenses(
            "skip_before_action :login_required, if: -> { trusted_origin? && action_name == \"show\" }\n",
        );
    }

    #[test]
    fn does_not_flag_only() {
        test::<IgnoredSkipActionFilterOption>().expect_no_offenses(
            "skip_before_action :login_required, only: :show\n",
        );
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<IgnoredSkipActionFilterOption>().expect_no_offenses(
            "obj.skip_before_action :login_required, only: :show, if: :x\n",
        );
    }
}
murphy_plugin_api::submit_cop!(IgnoredSkipActionFilterOption);
