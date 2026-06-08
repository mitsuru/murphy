//! `Lint/SymbolConversion` — checks unnecessary symbol conversions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SymbolConversion
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-36n9]
//! notes: >
//!   Core RuboCop shapes implemented: literal string/symbol/dstr receiver
//!   to_sym/intern autocorrect, quoted symbol literal simplification, hash-key
//!   quoted symbol simplification, alias and percent-symbol-array guards, and
//!   EnforcedStyle strict/consistent behavior. Known v1 limitation: raw source
//!   escaping follows conservative Ruby identifier checks rather than full
//!   Symbol#inspect parity for every escaped byte sequence (murphy-36n9).
//! ```

use murphy_plugin_api::{cop, CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range};

const MSG: &str = "Unnecessary symbol conversion; use `%<correction>s` instead.";
const MSG_CONSISTENCY: &str =
    "Symbol hash key should be quoted for consistency; use `%<correction>s` instead.";

#[derive(Default)]
pub struct SymbolConversion;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "strict")]
    Strict,
    #[option(value = "consistent")]
    Consistent,
}

#[derive(CopOptions)]
pub struct SymbolConversionOptions {
    #[option(name = "EnforcedStyle", default = "strict", description = "Symbol conversion style.")]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Lint/SymbolConversion",
    description = "Checks unnecessary symbol conversions.",
    default_severity = "warning",
    default_enabled = true,
    options = SymbolConversionOptions,
)]
impl SymbolConversion {
    #[on_node(kind = "send", methods = ["to_sym", "intern"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !cx.list(args).is_empty() {
            return;
        }
        let Some(receiver) = receiver.get() else {
            return;
        };
        let correction = match *cx.kind(receiver) {
            NodeKind::Str(s) => symbol_literal(cx.string_str(s)),
            NodeKind::Sym(s) => format!(":{}", cx.symbol_str(s)),
            NodeKind::Dstr(_) => format!(":\"{}\"", strip_quotes(cx.raw_source(cx.range(receiver)))),
            _ => return,
        };
        emit(cx, node, &correction, MSG);
    }

    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        if in_alias(node, cx) || in_percent_literal_array(node, cx) {
            return;
        }
        let sym = match *cx.kind(node) {
            NodeKind::Sym(sym) => cx.symbol_str(sym),
            _ => return,
        };
        if let Some(pair) = pair_parent(node, cx) {
            let _ = pair;
            return;
        }
        let src = cx.raw_source(cx.range(node));
        let correction = format!(":{sym}");
        if src != correction && bare_symbol_name(sym) {
            emit(cx, node, &correction, MSG);
        }
    }

    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<SymbolConversionOptions>();
        let NodeKind::Hash(pairs) = *cx.kind(node) else {
            return;
        };
        let pair_ids = cx.list(pairs);
        if opts.enforced_style == EnforcedStyle::Strict {
            for &pair in pair_ids {
                let NodeKind::Pair { key, .. } = *cx.kind(pair) else { continue };
                if let NodeKind::Sym(sym) = *cx.kind(key) {
                    correct_hash_key(key, pair, cx.symbol_str(sym), cx, MSG);
                }
            }
            return;
        }
        let keys: Vec<_> = pair_ids
            .iter()
            .filter_map(|&pair| match *cx.kind(pair) {
                NodeKind::Pair { key, .. } if matches!(cx.kind(key), NodeKind::Sym(_)) => Some((pair, key)),
                _ => None,
            })
            .collect();
        let any_requires_quotes = keys.iter().any(|&(_, key)| {
            let NodeKind::Sym(sym) = *cx.kind(key) else { return false };
            !bare_symbol_name(cx.symbol_str(sym))
        });
        if any_requires_quotes {
            for &(_, key) in &keys {
                let NodeKind::Sym(sym) = *cx.kind(key) else { continue };
                let name = cx.symbol_str(sym);
                if bare_symbol_name(name) && !is_quoted_hash_key(key, cx) {
                    let correction = format!("\"{name}\"");
                    let message = MSG_CONSISTENCY.replace("%<correction>s", &format!("{correction}:"));
                    cx.emit_offense(unquoted_hash_key_name_range(key, cx), &message, None);
                    cx.emit_edit(unquoted_hash_key_name_range(key, cx), &correction);
                }
            }
        } else {
            for &(pair, key) in &keys {
                let NodeKind::Sym(sym) = *cx.kind(key) else { continue };
                correct_hash_key(key, pair, cx.symbol_str(sym), cx, MSG);
            }
        }
    }
}

fn unquoted_hash_key_name_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(node);
    if cx.raw_source(r).ends_with(':') {
        Range { start: r.start, end: r.end.saturating_sub(1) }
    } else {
        r
    }
}

fn emit(cx: &Cx<'_>, node: NodeId, correction: &str, template: &str) {
    let message = template.replace("%<correction>s", correction);
    cx.emit_offense(cx.range(node), &message, None);
    cx.emit_edit(cx.range(node), correction);
}

fn correct_hash_key(node: NodeId, pair: NodeId, sym: &str, cx: &Cx<'_>, template: &str) {
    if !bare_hash_key(sym) || !is_quoted_hash_key(node, cx) {
        return;
    }
    let edit_range = hash_key_edit_range(node, pair, cx);
    let correction = if hash_key_uses_hash_rocket(node, pair, cx) {
        format!("{sym}: ")
    } else {
        sym.to_string()
    };
    let message_correction = format!("{sym}:");
    let message = template.replace("%<correction>s", &message_correction);
    cx.emit_offense(edit_range, &message, None);
    cx.emit_edit(edit_range, &correction);
}

fn symbol_literal(value: &str) -> String {
    if bare_symbol_name(value) {
        format!(":{value}")
    } else {
        format!(":\"{value}\"")
    }
}

fn strip_quotes(src: &str) -> &str {
    src.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(src)
}

fn bare_symbol_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else { return false };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    let mut rest: Vec<char> = chars.collect();
    let suffix_ok = rest.last().is_some_and(|c| matches!(c, '!' | '?'));
    if suffix_ok {
        rest.pop();
    }
    rest.into_iter().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn bare_hash_key(value: &str) -> bool {
    bare_symbol_name(value) && !value.ends_with('=')
}

fn pair_parent(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(node).get()?;
    matches!(cx.kind(parent), NodeKind::Pair { key, .. } if *key == node).then_some(parent)
}

fn is_quoted_hash_key(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.raw_source(cx.range(node));
    src.starts_with('"') || src.starts_with('\'') || src.starts_with(":\"") || src.starts_with(":'")
}

fn hash_key_uses_hash_rocket(node: NodeId, pair: NodeId, cx: &Cx<'_>) -> bool {
    let key_range = cx.range(node);
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else { return false };
    let gap = Range { start: key_range.end, end: cx.range(value).start };
    cx.raw_source(gap).contains("=>")
}

fn hash_key_edit_range(node: NodeId, pair: NodeId, cx: &Cx<'_>) -> Range {
    let key_range = cx.range(node);
    if cx.raw_source(key_range).ends_with(':') {
        return Range { start: key_range.start, end: key_range.end.saturating_sub(1) };
    }
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else { return key_range };
    let value_start = cx.range(value).start;
    let gap = Range { start: key_range.end, end: value_start };
    if cx.raw_source(gap).contains("=>") {
        return Range { start: key_range.start, end: value_start };
    }
    key_range
}

fn in_alias(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| cx.raw_source(cx.range(a)).trim_start().starts_with("alias "))
}

fn in_percent_literal_array(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| {
        matches!(cx.kind(a), NodeKind::Array(_))
            && cx.raw_source(cx.range(a)).trim_start().starts_with("%i")
    })
}

murphy_plugin_api::submit_cop!(SymbolConversion);

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, SymbolConversion, SymbolConversionOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_to_sym() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                "foo".to_sym
                ^^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
            "#},
            ":foo\n",
        );
    }

    #[test]
    fn flags_quoted_symbol_and_hash_key() {
        test::<SymbolConversion>()
            .expect_correction(
                indoc! {r#"
                    :"foo"
                    ^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                ":foo\n",
            )
            .expect_correction(
                indoc! {r#"
                    { 'foo': 'bar' }
                      ^^^^^ Unnecessary symbol conversion; use `foo:` instead.
                "#},
                "{ foo: 'bar' }\n",
            )
            .expect_correction(
                indoc! {r#"
                    { :'foo' => 'bar' }
                      ^^^^^^^^^^ Unnecessary symbol conversion; use `foo:` instead.
                "#},
                "{ foo: 'bar' }\n",
            )
            .expect_correction(
                indoc! {r#"
                    { :"foo" => 'bar' }
                      ^^^^^^^^^^ Unnecessary symbol conversion; use `foo:` instead.
                "#},
                "{ foo: 'bar' }\n",
            );
    }

    #[test]
    fn consistent_style_quotes_unquoted_keys_when_required() {
        test::<SymbolConversion>()
            .with_options(&SymbolConversionOptions { enforced_style: EnforcedStyle::Consistent })
            .expect_correction(
                indoc! {r#"
                    {
                      a: 1,
                      ^ Symbol hash key should be quoted for consistency; use `"a":` instead.
                      'b-c': 2
                    }
                "#},
                "{\n  \"a\": 1,\n  'b-c': 2\n}\n",
            );
    }
}
