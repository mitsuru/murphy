//! `Lint/SymbolConversion` — checks unnecessary symbol conversions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SymbolConversion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop 1.87 `Lint/SymbolConversion`: literal string/symbol/dstr
//!   receiver `to_sym`/`intern` autocorrect via `Symbol#inspect` parity,
//!   quoted symbol simplification with single/double-quote equivalence,
//!   hash-key correction preserving `=>` vs `:` delimiters, operator hash-key
//!   guard (`/\\A[a-z0-9_]/i`), `%i`/`%I` percent-array guard via
//!   `is_percent_literal`, `Alias`-node guard, and `EnforcedStyle`
//!   strict/consistent behavior including `requires_quotes?`
//!   (`/^:".*?"|=$/`) and `properly_quoted?` single-quote normalization.
//!   Dstr correction preserves source escapes (`:"foo-#{bar}"`); RuboCop's
//!   `value.to_sym.to_s` collapses `\\n` to a literal newline for Dstr with
//!   escapes, which is semantically equal but byte-different.
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
            NodeKind::Str(s) => symbol_inspect(cx.string_str(s)),
            NodeKind::Sym(s) => symbol_inspect(cx.symbol_str(s)),
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
        if pair_parent(node, cx).is_some() {
            return;
        }
        let opts = cx.options_or_default::<SymbolConversionOptions>();
        let correction = symbol_inspect(sym);
        let src = cx.raw_source(cx.range(node));
        if properly_quoted(src, &correction, opts.enforced_style) {
            return;
        }
        emit(cx, node, &correction, MSG);
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
                if matches!(cx.kind(key), NodeKind::Sym(_)) {
                    correct_hash_key(key, pair, cx, opts.enforced_style, MSG);
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
            requires_quotes(&symbol_inspect(cx.symbol_str(sym)))
        });
        if any_requires_quotes {
            for &(pair, key) in &keys {
                let NodeKind::Sym(sym) = *cx.kind(key) else { continue };
                let name = cx.symbol_str(sym);
                if requires_quotes(&symbol_inspect(name)) {
                    continue;
                }
                let expected = format!("\"{}\"", escape_symbol_body(name));
                let src = key_source_without_colon(key, cx);
                if properly_quoted(src, &expected, opts.enforced_style) {
                    continue;
                }
                let offense_range = key_range_without_colon(key, cx);
                let correction = expected.clone();
                let message =
                    MSG_CONSISTENCY.replace("%<correction>s", &format!("{correction}:"));
                cx.emit_offense(offense_range, &message, None);
                cx.emit_edit(offense_range, &correction);
                let _ = pair;
            }
        } else {
            for &(pair, key) in &keys {
                correct_hash_key(key, pair, cx, opts.enforced_style, MSG);
            }
        }
    }
}

fn key_range_without_colon(node: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(node);
    if cx.raw_source(r).ends_with(':') {
        Range { start: r.start, end: r.end.saturating_sub(1) }
    } else {
        r
    }
}

fn key_source_without_colon<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    let r = cx.range(node);
    let src = cx.raw_source(r);
    if src.ends_with(':') && !src.is_empty() {
        &src[..src.len() - 1]
    } else {
        src
    }
}

fn emit(cx: &Cx<'_>, node: NodeId, correction: &str, template: &str) {
    let message = template.replace("%<correction>s", correction);
    cx.emit_offense(cx.range(node), &message, None);
    cx.emit_edit(cx.range(node), correction);
}

fn correct_hash_key(
    node: NodeId,
    pair: NodeId,
    cx: &Cx<'_>,
    style: EnforcedStyle,
    template: &str,
) {
    let NodeKind::Sym(sym) = *cx.kind(node) else { return };
    let name = cx.symbol_str(sym);
    if !starts_alnum_underscore(name) {
        return;
    }
    let is_colon = cx.is_colon(pair);
    let inspect = symbol_inspect(name);
    let correction = if is_colon {
        inspect.strip_prefix(':').unwrap_or(&inspect).to_string()
    } else {
        inspect.clone()
    };
    let src = key_source_without_colon(node, cx);
    if properly_quoted(src, &correction, style) {
        return;
    }
    let edit_range = key_range_without_colon(node, cx);
    let message_correction = if is_colon { format!("{correction}:") } else { correction.clone() };
    let message = template.replace("%<correction>s", &message_correction);
    cx.emit_offense(edit_range, &message, None);
    cx.emit_edit(edit_range, &correction);
}

/// Ruby `Symbol#inspect` parity: `:foo` for bare symbols, `:"..."` with
/// Ruby-compatible escapes otherwise.
fn symbol_inspect(name: &str) -> String {
    if is_bare_symbol(name) {
        format!(":{name}")
    } else {
        format!(":\"{}\"", escape_symbol_body(name))
    }
}

fn is_bare_symbol(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if is_bare_operator(name) {
        return true;
    }
    if let Some(rest) = name.strip_prefix("@@") {
        return is_plain_identifier(rest);
    }
    if let Some(rest) = name.strip_prefix('@') {
        return is_plain_identifier(rest);
    }
    if let Some(rest) = name.strip_prefix('$') {
        return is_bare_global_rest(rest);
    }
    if name.ends_with('!') || name.ends_with('?') || name.ends_with('=') {
        let base = &name[..name.len() - 1];
        if base.ends_with('!') || base.ends_with('?') || base.ends_with('=') {
            return false;
        }
        return is_plain_identifier(base);
    }
    is_plain_identifier(name)
}

fn is_plain_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn is_bare_operator(name: &str) -> bool {
    matches!(
        name,
        "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "=="
            | "==="
            | "!="
            | "=~"
            | "!~"
            | "<"
            | ">"
            | "<="
            | ">="
            | "<=>"
            | "<<"
            | ">>"
            | "&"
            | "|"
            | "^"
            | "~"
            | "!"
            | "`"
            | "[]"
            | "[]="
            | "+@"
            | "-@"
    )
}

fn is_bare_global_rest(rest: &str) -> bool {
    if rest.is_empty() {
        return false;
    }
    if is_plain_identifier(rest) {
        return true;
    }
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if rest.chars().count() == 1 {
        let c = rest.chars().next().unwrap();
        return c.is_ascii_punctuation()
            && !matches!(c, '"' | '\'' | '#' | '\\')
            && !c.is_ascii_whitespace();
    }
    false
}

fn escape_symbol_body(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 2);
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{07}' => out.push_str("\\a"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0b}' => out.push_str("\\v"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            '\0' => out.push_str("\\x00"),
            '#' => {
                let next = chars.get(i + 1);
                if matches!(next, Some('{') | Some('$') | Some('@')) {
                    out.push_str("\\#");
                } else {
                    out.push('#');
                }
            }
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// RuboCop `properly_quoted?`: `source == value` or single-to-double
/// normalization matches, plus strict-mode early return for quoteless
/// sources and `=`-suffixed corrections.
fn properly_quoted(source: &str, expected: &str, style: EnforcedStyle) -> bool {
    if style == EnforcedStyle::Strict
        && (!source.contains('\'') && !source.contains('"') || expected.ends_with('='))
    {
        return true;
    }
    if source == expected {
        return true;
    }
    let gsubbed = source.replace('"', "\\\"");
    let normalized: String = gsubbed.chars().map(|c| if c == '\'' { '"' } else { c }).collect();
    normalized == expected
}

/// RuboCop `requires_quotes?`: `value.inspect.match?(/^:".*?"|=$/)`.
fn requires_quotes(inspect: &str) -> bool {
    inspect.starts_with(":\"") || inspect.ends_with('=')
}

/// RuboCop `correct_hash_key` guard: `value.to_s.match?(/\\A[a-z0-9_]/i)`.
fn starts_alnum_underscore(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn strip_quotes(src: &str) -> &str {
    src.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(src)
}

fn pair_parent(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(node).get()?;
    matches!(cx.kind(parent), NodeKind::Pair { key, .. } if *key == node).then_some(parent)
}

fn in_alias(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| matches!(cx.kind(a), NodeKind::Alias { .. }))
}

fn in_percent_literal_array(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| cx.is_percent_literal(a))
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
                      ^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                "{ :foo => 'bar' }\n",
            )
            .expect_correction(
                indoc! {r#"
                    { :"foo" => 'bar' }
                      ^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                "{ :foo => 'bar' }\n",
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

    #[test]
    fn flags_operator_string_to_sym() {
        test::<SymbolConversion>()
            .expect_correction(
                indoc! {r#"
                    "==".to_sym
                    ^^^^^^^^^^^ Unnecessary symbol conversion; use `:==` instead.
                "#},
                ":==\n",
            )
            .expect_correction(
                indoc! {r#"
                    "@foo".to_sym
                    ^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:@foo` instead.
                "#},
                ":@foo\n",
            )
            .expect_correction(
                indoc! {r#"
                    "foo=".to_sym
                    ^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo=` instead.
                "#},
                ":foo=\n",
            );
    }

    #[test]
    fn flags_sym_receiver_with_quotes_needed() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                :"foo-bar".to_sym
                ^^^^^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:"foo-bar"` instead.
            "#},
            ":\"foo-bar\"\n",
        );
    }

    #[test]
    fn flags_escaped_string_to_sym() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                "foo\nbar".to_sym
                ^^^^^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:"foo\nbar"` instead.
            "#},
            ":\"foo\\nbar\"\n",
        );
    }

    #[test]
    fn accepts_properly_quoted_symbols() {
        test::<SymbolConversion>()
            .expect_no_offenses(":\"foo-bar\"\n")
            .expect_no_offenses(":'foo-bar'\n")
            .expect_no_offenses(":'foo-bar\"\"'\n")
            .expect_no_offenses(":'Foo/Bar/Baz'\n")
            .expect_no_offenses(":\"foo\\nbar\"\n");
    }

    #[test]
    fn accepts_operator_hash_key_and_ignores_percent_i() {
        test::<SymbolConversion>()
            .expect_no_offenses("{ '==': 'bar' }\n")
            .expect_no_offenses("{ 'foo=': 'bar' }\n")
            .expect_no_offenses("{ 'foo:bar': 'bar' }\n")
            .expect_no_offenses("%i(foo bar foo-bar)\n")
            .expect_no_offenses("%I(foo bar foo-bar)\n")
            .expect_no_offenses("alias foo bar\n")
            .expect_no_offenses("alias == equal\n");
    }

    #[test]
    fn flags_hash_key_bang_and_question() {
        test::<SymbolConversion>()
            .expect_correction(
                indoc! {r#"
                    { 'foo!': 'bar' }
                      ^^^^^^ Unnecessary symbol conversion; use `foo!:` instead.
                "#},
                "{ foo!: 'bar' }\n",
            )
            .expect_correction(
                indoc! {r#"
                    { 'foo?': 'bar' }
                      ^^^^^^ Unnecessary symbol conversion; use `foo?:` instead.
                "#},
                "{ foo?: 'bar' }\n",
            );
    }

    #[test]
    fn flags_dstr_to_sym() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                "foo-#{bar}".to_sym
                ^^^^^^^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:"foo-#{bar}"` instead.
            "#},
            ":\"foo-#{bar}\"\n",
        );
    }

    #[test]
    fn flags_single_quoted_sym_to_bare() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                :'foo'
                ^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
            "#},
            ":foo\n",
        );
    }

    #[test]
    fn flags_hash_value_quoted_symbol() {
        test::<SymbolConversion>().expect_correction(
            indoc! {r#"
                { foo: :'bar' }
                       ^^^^^^ Unnecessary symbol conversion; use `:bar` instead.
            "#},
            "{ foo: :bar }\n",
        );
    }

    #[test]
    fn spec_to_sym_and_intern_shapes() {
        test::<SymbolConversion>()
            .expect_correction(
                indoc! {r#"
                    :foo.to_sym
                    ^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                ":foo\n",
            )
            .expect_correction(
                indoc! {r#"
                    "foo_bar".to_sym
                    ^^^^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo_bar` instead.
                "#},
                ":foo_bar\n",
            )
            .expect_correction(
                indoc! {r#"
                    "foo-bar".to_sym
                    ^^^^^^^^^^^^^^^^ Unnecessary symbol conversion; use `:"foo-bar"` instead.
                "#},
                ":\"foo-bar\"\n",
            )
            .expect_correction(
                indoc! {r#"
                    :foo.intern
                    ^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                ":foo\n",
            )
            .expect_correction(
                indoc! {r#"
                    "foo".intern
                    ^^^^^^^^^^^^ Unnecessary symbol conversion; use `:foo` instead.
                "#},
                ":foo\n",
            )
            .expect_no_offenses(":foo\n")
            .expect_no_offenses("{ foo: 'bar' }\n")
            .expect_no_offenses("{ foo: :bar }\n")
            .expect_no_offenses("alias foo bar\nalias == equal\nalias eq? ==\n");
    }

    #[test]
    fn spec_consistent_no_quoting_required() {
        test::<SymbolConversion>()
            .with_options(&SymbolConversionOptions { enforced_style: EnforcedStyle::Consistent })
            .expect_no_offenses("{\n  a: 1,\n  b: 2\n}\n")
            .expect_correction(
                indoc! {r#"
                    {
                      a: 1,
                      'b': 2
                      ^^^ Unnecessary symbol conversion; use `b:` instead.
                    }
                "#},
                "{\n  a: 1,\n  b: 2\n}\n",
            );
    }

    #[test]
    fn spec_consistent_different_quote_styles_ok() {
        test::<SymbolConversion>()
            .with_options(&SymbolConversionOptions { enforced_style: EnforcedStyle::Consistent })
            .expect_no_offenses("{\n  'a': 1,\n  \"b\": 2,\n  \"c-d\": 3,\n  'e-f': 4\n}\n")
            .expect_no_offenses("{\n  'a' => 1,\n  'b-c': 2\n}\n");
    }

    #[test]
    fn spec_consistent_equal_key_requires_quotes() {
        test::<SymbolConversion>()
            .with_options(&SymbolConversionOptions { enforced_style: EnforcedStyle::Consistent })
            .expect_correction(
                indoc! {r#"
                    {
                      'a=': 1,
                      b: 2
                      ^ Symbol hash key should be quoted for consistency; use `"b":` instead.
                    }
                "#},
                "{\n  'a=': 1,\n  \"b\": 2\n}\n",
            );
    }

    #[test]
    fn ignores_implicit_to_sym_and_dstr_key() {
        test::<SymbolConversion>()
            .expect_no_offenses("to_sym == other\n")
            .expect_no_offenses("{ \"foo-#{'bar'}\": 'baz' }\n");
    }
}
