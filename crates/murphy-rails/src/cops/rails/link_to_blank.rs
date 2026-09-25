//! `Rails/LinkToBlank` — require `rel: noopener` with `target: _blank`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/LinkToBlank
//! upstream_version_checked: 2.35.0
//! version_added: "0.62"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [link_to link_to_if link_to_unless] with per-hash `target: _blank`
//!   (sym/str key, str/sym value) gating and `rel` noopener/noreferrer
//!   exemption (str/sym values split on whitespace). Offense is the blank
//!   pair; autocorrect appends ` noopener` inside an existing str `rel`
//!   or inserts `, rel: 'noopener'` (`:noopener` for sym targets) after
//!   the last argument (last pair when the last arg is a hash).
//! ```
//!
//! Checks for calls to `link_to`, `link_to_if`, and `link_to_unless`
//! with `target: '_blank'` but no `rel: 'noopener'`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LinkToBlank;

#[cop(
    name = "Rails/LinkToBlank",
    description = "Checks that `link_to` with a `target: \"_blank\"` have a `rel: \"noopener\"` option passed to them.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl LinkToBlank {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["link_to", "link_to_if", "link_to_unless"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    // Upstream `node.each_child_node(:hash)` — direct hash arguments.
    for &arg in args {
        if !matches!(*cx.kind(arg), NodeKind::Hash(_)) {
            continue;
        }
        let NodeKind::Hash(pairs) = *cx.kind(arg) else {
            continue;
        };
        let pair_ids = cx.list(pairs).to_vec();
        let blank = pair_ids.iter().find(|&&p| is_blank_target(cx, p));
        let Some(&blank_id) = blank else {
            continue;
        };
        if pair_ids.iter().any(|&p| includes_noopener(cx, p)) {
            continue;
        }
        cx.emit_offense(
            cx.range(blank_id),
            "Specify a `:rel` option containing noopener.",
            None,
        );
        emit_correction(cx, node, blank_id, args);
    }
}

fn is_blank_target(cx: &Cx<'_>, pair: NodeId) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    if !is_target_key(cx, key) {
        return false;
    }
    match *cx.kind(value) {
        NodeKind::Str(sid) => cx.string_str(sid) == "_blank",
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "_blank",
        _ => false,
    }
}

fn is_target_key(cx: &Cx<'_>, key: NodeId) -> bool {
    match *cx.kind(key) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "target",
        NodeKind::Str(sid) => cx.string_str(sid) == "target",
        _ => false,
    }
}

fn includes_noopener(cx: &Cx<'_>, pair: NodeId) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    let is_rel = match *cx.kind(key) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "rel",
        NodeKind::Str(sid) => cx.string_str(sid) == "rel",
        _ => false,
    };
    if !is_rel {
        return false;
    }
    let text = match *cx.kind(value) {
        NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
        NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
        _ => return false,
    };
    text.split_whitespace()
        .any(|w| w == "noopener" || w == "noreferrer")
}

fn str_rel_pair(cx: &Cx<'_>, args: &[NodeId]) -> Option<NodeId> {
    for &arg in args {
        if !matches!(*cx.kind(arg), NodeKind::Hash(_)) {
            continue;
        }
        let NodeKind::Hash(pairs) = *cx.kind(arg) else {
            continue;
        };
        for &p in cx.list(pairs) {
            let NodeKind::Pair { key, value } = *cx.kind(p) else {
                continue;
            };
            let is_rel = match *cx.kind(key) {
                NodeKind::Sym(sym) => cx.symbol_str(sym) == "rel",
                NodeKind::Str(sid) => cx.string_str(sid) == "rel",
                _ => false,
            };
            if !is_rel {
                continue;
            }
            if matches!(*cx.kind(value), NodeKind::Str(_)) {
                return Some(p);
            }
        }
    }
    None
}

fn emit_correction(cx: &Cx<'_>, _send: NodeId, blank_pair: NodeId, args: &[NodeId]) {
    // Existing str `rel` anywhere in the call → append inside the string.
    if let Some(rel_pair) = str_rel_pair(cx, args) {
        let NodeKind::Pair { value, .. } = *cx.kind(rel_pair) else {
            return;
        };
        let NodeKind::Str(sid) = *cx.kind(value) else {
            return;
        };
        let existing = cx.string_str(sid).to_owned();
        let r = cx.range(value);
        if r.end <= r.start + 1 {
            return;
        }
        let inner = Range {
            start: r.start + 1,
            end: r.end - 1,
        };
        cx.emit_edit(inner, &format!("{existing} noopener"));
        return;
    }
    // Otherwise insert `, rel: ...` after the last argument.
    let Some(&last) = args.last() else {
        return;
    };
    let insert_pos = match *cx.kind(last) {
        NodeKind::Hash(pairs) => {
            let plist = cx.list(pairs);
            plist
                .last()
                .map(|&p| cx.range(p).end)
                .unwrap_or_else(|| cx.range(last).end)
        }
        _ => cx.range(last).end,
    };
    // Quote style follows the target value (`:_blank` → sym rel).
    let NodeKind::Pair { value, .. } = *cx.kind(blank_pair) else {
        return;
    };
    let target_src = cx.raw_source(cx.range(value));
    let opening = target_src.chars().next().unwrap_or('\'');
    let new_rel = if opening == ':' {
        ", rel: :noopener".to_owned()
    } else {
        format!(", rel: {opening}noopener{opening}")
    };
    cx.emit_edit(
        Range {
            start: insert_pos,
            end: insert_pos,
        },
        &new_rel,
    );
}

#[cfg(test)]
mod tests {
    use super::LinkToBlank;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_link_to_blank_without_rel() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to 'Click here', 'https://www.example.com', target: '_blank'
                                                             ^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn corrects_link_to_blank_without_rel() {
        test::<LinkToBlank>().expect_correction(
            indoc! {r#"
                link_to 'Click here', 'https://www.example.com', target: '_blank'
                                                                 ^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
            "#},
            "link_to 'Click here', 'https://www.example.com', target: '_blank', rel: 'noopener'\n",
        );
    }

    #[test]
    fn flags_string_target_key() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to 'Click here', 'https://www.example.com', "target" => '_blank'
                                                             ^^^^^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn flags_sym_target_value() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to 'Click here', 'https://www.example.com', target: :_blank
                                                             ^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn corrects_sym_target_value_with_sym_rel() {
        test::<LinkToBlank>().expect_correction(
            indoc! {r#"
                link_to 'Click here', 'https://www.example.com', target: :_blank
                                                                 ^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
            "#},
            "link_to 'Click here', 'https://www.example.com', target: :_blank, rel: :noopener\n",
        );
    }

    #[test]
    fn flags_hash_brackets_form() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to 'Click here', 'https://www.example.com', { target: :_blank }
                                                               ^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn corrects_unrelated_rel_by_appending() {
        test::<LinkToBlank>().expect_correction(
            indoc! {r#"
                link_to 'Click here', 'https://www.example.com', "target" => '_blank', rel: 'unrelated'
                                                                 ^^^^^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
            "#},
            "link_to 'Click here', 'https://www.example.com', \"target\" => '_blank', rel: 'unrelated noopener'\n",
        );
    }

    #[test]
    fn allows_rel_noopener() {
        test::<LinkToBlank>().expect_no_offenses(
            "link_to 'Click here', 'https://www.example.com', target: '_blank', rel: 'noopener unrelated'\n",
        );
    }

    #[test]
    fn allows_rel_noreferrer() {
        test::<LinkToBlank>().expect_no_offenses(
            "link_to 'Click here', 'https://www.example.com', target: '_blank', rel: 'unrelated noreferrer'\n",
        );
    }

    #[test]
    fn allows_sym_rel_noopener() {
        test::<LinkToBlank>().expect_no_offenses(
            "link_to 'Click here', 'https://www.example.com', target: :_blank, rel: :noopener\n",
        );
    }

    #[test]
    fn allows_without_blank() {
        test::<LinkToBlank>().expect_no_offenses(
            "link_to 'Click here', 'https://www.example.com', class: 'big'\n",
        );
    }

    #[test]
    fn flags_link_to_if_blank() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to_if condition?, 'Click here', 'https://www.example.com', target: '_blank'
                                                                            ^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn flags_link_to_unless_blank() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to_unless condition?, 'Click here', 'https://www.example.com', target: '_blank'
                                                                                ^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
        "#});
    }

    #[test]
    fn flags_block_form() {
        test::<LinkToBlank>().expect_offense(indoc! {r#"
            link_to 'https://www.example.com', target: '_blank' do
                                               ^^^^^^^^^^^^^^^^ Specify a `:rel` option containing noopener.
              "Click here"
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(LinkToBlank);
