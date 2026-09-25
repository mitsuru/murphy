//! `Rails/RedundantForeignKey` — redundant `:foreign_key` on associations.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantForeignKey
//! upstream_version_checked: 2.35.0
//! version_added: "2.6"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND
//!   [belongs_to has_one has_many has_and_belongs_to_many], bare
//!   calls with a sym/str association name plus a trailing hash
//!   containing a sym/str `:foreign_key`, default computation
//!   (`belongs_to` → `<name>_id`, `as:` override, else enclosing
//!   class/module demodulized `foreign_key`), pair-only offense with
//!   left-biased comma removal.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantForeignKey;

#[cop(
    name = "Rails/RedundantForeignKey",
    description = "Checks for associations where the `:foreign_key` option is redundant.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantForeignKey {
    #[on_node(
        kind = "send",
        methods = [
            "belongs_to",
            "has_one",
            "has_many",
            "has_and_belongs_to_many"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !matches!(
        method.as_str(),
        "belongs_to" | "has_one" | "has_many" | "has_and_belongs_to_many"
    ) {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 2 {
        return;
    }
    let assoc_name = match assoc_string(cx, args[0]) {
        Some(s) => s,
        None => return,
    };
    let NodeKind::Hash(list) = *cx.kind(args[1]) else {
        return;
    };
    let pairs = cx.list(list).to_vec();
    let mut fk_pair = None;
    let mut fk_value = None;
    for &p in &pairs {
        let NodeKind::Pair { key, value } = *cx.kind(p) else {
            continue;
        };
        if let NodeKind::Sym(s) = *cx.kind(key)
            && cx.symbol_str(s) == "foreign_key"
            && let Some(fk) = fk_string(cx, value)
        {
            fk_pair = Some(p);
            fk_value = Some(fk);
            break;
        }
        // Non sym/str foreign_key value — upstream pattern does not
        // capture; keep scanning but never flags.
    }
    let (Some(pair), Some(fk)) = (fk_pair, fk_value) else {
        return;
    };
    let default = default_foreign_key(cx, node, &method, &assoc_name, &pairs);
    let Some(default) = default else {
        return;
    };
    if fk != default {
        return;
    }
    cx.emit_offense(
        cx.range(pair),
        "Specifying the default value for `foreign_key` is redundant.",
        None,
    );
    cx.emit_edit(removal_range(cx, cx.range(pair)), "");
}

fn assoc_string(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Str(sid) => Some(cx.string_str(sid).to_owned()),
        _ => None,
    }
}

fn fk_string(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Str(sid) => Some(cx.string_str(sid).to_owned()),
        _ => None,
    }
}

fn default_foreign_key(
    cx: &Cx<'_>,
    node: NodeId,
    method: &str,
    assoc: &str,
    pairs: &[NodeId],
) -> Option<String> {
    if method == "belongs_to" {
        return Some(format!("{assoc}_id"));
    }
    if let Some(as_name) = find_as_option(cx, pairs) {
        return Some(format!("{as_name}_id"));
    }
    // Enclosing class/module demodulized foreign_key.
    let enclosing = enclosing_class_name(cx, node)?;
    Some(format!("{}_id", underscore(&demodulize(&enclosing))))
}

fn find_as_option(cx: &Cx<'_>, pairs: &[NodeId]) -> Option<String> {
    for &p in pairs {
        let NodeKind::Pair { key, value } = *cx.kind(p) else {
            continue;
        };
        if let NodeKind::Sym(s) = *cx.kind(key) {
            if cx.symbol_str(s) != "as" {
                continue;
            }
            match *cx.kind(value) {
                NodeKind::Sym(v) => return Some(cx.symbol_str(v).to_owned()),
                NodeKind::Str(v) => return Some(cx.string_str(v).to_owned()),
                _ => continue,
            }
        }
    }
    None
}

/// Nearest enclosing `class`/`module` name (const path source).
fn enclosing_class_name(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    for anc in cx.ancestors(node) {
        match *cx.kind(anc) {
            NodeKind::Class { name, .. } => {
                return Some(cx.const_name(name).unwrap_or_else(|| cx.raw_source(cx.range(name)).to_owned()));
            }
            NodeKind::Module { name, .. } => {
                return Some(cx.const_name(name).unwrap_or_else(|| cx.raw_source(cx.range(name)).to_owned()));
            }
            _ => {}
        }
    }
    None
}

fn demodulize(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).to_owned()
}

/// Minimal ActiveSupport `underscore`: `BlogPost` → `blog_post`.
fn underscore(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied();
                if prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase()
                        && next.is_some_and(|n| n.is_ascii_lowercase()))
                {
                    out.push('_');
                }
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

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
    use super::RedundantForeignKey;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_belongs_to_default() {
        test::<RedundantForeignKey>().expect_offense(indoc! {r#"
            class Comment
              belongs_to :post, foreign_key: 'post_id'
                                ^^^^^^^^^^^^^^^^^^^^^^ Specifying the default value for `foreign_key` is redundant.
            end
        "#});
    }

    #[test]
    fn flags_has_many_default() {
        test::<RedundantForeignKey>().expect_offense(indoc! {r#"
            class Post
              has_many :comments, foreign_key: 'post_id'
                                  ^^^^^^^^^^^^^^^^^^^^^^ Specifying the default value for `foreign_key` is redundant.
            end
        "#});
    }

    #[test]
    fn allows_non_default() {
        test::<RedundantForeignKey>().expect_no_offenses(indoc! {r#"
            class Comment
              belongs_to :author, foreign_key: 'user_id'
            end
        "#});
    }

    #[test]
    fn allows_no_class_context_for_has_many() {
        // No enclosing class → no default to compare (upstream `&.`).
        test::<RedundantForeignKey>()
            .expect_no_offenses("has_many :comments, foreign_key: 'post_id'\n");
    }

    #[test]
    fn flags_as_option_default() {
        test::<RedundantForeignKey>().expect_offense(indoc! {r#"
            class Post
              has_many :comments, as: :commentable, foreign_key: 'commentable_id'
                                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specifying the default value for `foreign_key` is redundant.
            end
        "#});
    }

    #[test]
    fn corrects_belongs_to() {
        test::<RedundantForeignKey>().expect_correction(
            indoc! {r#"
                class Comment
                  belongs_to :post, foreign_key: 'post_id'
                                    ^^^^^^^^^^^^^^^^^^^^^^ Specifying the default value for `foreign_key` is redundant.
                end
            "#},
            "class Comment\n  belongs_to :post\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantForeignKey);
