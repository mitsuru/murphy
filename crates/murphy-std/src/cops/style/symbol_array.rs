//! `Style/SymbolArray` — prefer a configured syntax for arrays of symbols.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SymbolArray
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Implements both `EnforcedStyle` values: percent (default) flags
//!   bracket-style symbol arrays and suggests `%i[]` or `%I[]`; brackets
//!   flags `%i`/`%I` symbol arrays and suggests bracket arrays. MinSize
//!   is implemented (default 2, matching RuboCop). Percent-mode complex
//!   content detection uses the same `complex_content?`-style logic as
//!   RuboCop 1.86.2: balanced delimiter pairs (`[…]`, `(…)`) are allowed,
//!   while bare spaces and unpaired delimiters cause the array to be
//!   skipped. Symbols with embedded whitespace (tabs, newlines) use `%I`
//!   with proper escape sequences. Known v1 limitations: no
//!   `Style/PercentLiteralDelimiters` support (always uses `[]`);
//!   multi-line formatting is not preserved.
//! ```
//!
//! Dispatches on `NodeKind::Array`. In percent mode, flags bracket arrays
//! where every element is a plain `NodeKind::Sym` with non-complex content
//! and whose length meets `MinSize`. In brackets mode, flags `%i` / `%I`
//! arrays and rewrites them to `[:a, :b]` form.
//!
//! ## Checks
//!
//! An array node is flagged when **all** of the following hold:
//!
//! 1. The source text does **not** already start with `%i` or `%I`
//!    (percent-literal guard — avoids flagging what we'd produce).
//! 2. Every child is `NodeKind::Sym` (no dynamic `dsym` elements).
//! 3. No symbol value has "complex content" — a literal space ` `, or an
//!    unpaired delimiter character (`[` `]` `(` `)`) after removing balanced
//!    `[…]` and `(…)` pairs with no inner whitespace or nested same-type
//!    delimiters (mirrors RuboCop 1.86.2 `complex_content?`).
//! 4. The number of elements ≥ `MinSize` (default 2).
//!
//! ## Autocorrect
//!
//! Whole-node interpolation. Percent mode collects each symbol's name via
//! `cx.symbol_str`, formats `%i[name1 name2 …]` (or `%I[name1 name2 …]` when
//! symbol values contain characters that need escape sequences), and replaces
//! the full array range. Brackets mode builds `[:name1, :name2]`, quoting
//! symbols such as `four-five` as `:'four-five'`.
//!
//! Per `.claude/rules/autocorrect-pattern.md`: whole-node replacement is the
//! correct form here because the rewrite fundamentally reshapes the AST
//! (strips colons, commas, brackets → percent literal).
//!
//! ## MinSize option
//!
//! Arrays shorter than `MinSize` are not flagged.  Default is 2 (same as
//! RuboCop), meaning single-element arrays `[:a]` are never flagged.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SymbolArray;

/// Preferred syntax for symbol arrays.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "percent")]
    Percent,
    #[option(value = "brackets")]
    Brackets,
}

/// Cop options for [`SymbolArray`].
#[derive(CopOptions)]
pub struct SymbolArrayOptions {
    #[option(
        name = "EnforcedStyle",
        default = "percent",
        description = "Preferred style for symbol arrays."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(
        name = "MinSize",
        default = 2,
        description = "Minimum array size to trigger the cop."
    )]
    pub min_size: i64,
}

#[cop(
    name = "Style/SymbolArray",
    description = "Use `%i` or `%I` for an array of symbols.",
    default_severity = "warning",
    default_enabled = true,
    options = SymbolArrayOptions,
)]
impl SymbolArray {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let elements = cx.array_elements(node);
        let opts = cx.options_or_default::<SymbolArrayOptions>();

        match opts.enforced_style {
            EnforcedStyle::Percent => check_percent_style(node, elements, &opts, cx),
            EnforcedStyle::Brackets => check_brackets_style(node, elements, &opts, cx),
        }
    }
}

fn check_percent_style(
    node: NodeId,
    elements: &[NodeId],
    opts: &SymbolArrayOptions,
    cx: &Cx<'_>,
) {
    // Cheap early-exit: empty arrays and arrays whose first element is not
    // a symbol are the common case.
    if elements.is_empty() {
        return;
    }
    if !matches!(cx.kind(elements[0]), NodeKind::Sym(_)) {
        return;
    }

    // MinSize guard.
    if elements.len() < opts.min_size as usize {
        return;
    }

    // Percent-literal guard: already `%i[…]` or `%I[…]`.
    let array_src = cx.raw_source(cx.range(node));
    if array_src.starts_with("%i") || array_src.starts_with("%I") {
        return;
    }

    // All elements must be plain Sym nodes that can be safely represented
    // in a percent literal.  A symbol has "complex content" when its value
    // contains a literal space or an un-balanced delimiter character
    // (`[` `]` `(` `)`) — RuboCop 1.86.2 `complex_content?`.
    let has_complex = elements.iter().any(|&elem| {
        if let NodeKind::Sym(sym) = *cx.kind(elem) {
            complex_content(cx.symbol_str(sym))
        } else {
            true
        }
    });
    if has_complex {
        return;
    }

    // Detect whether any symbol value contains characters that require %I
    // (tab, newline, CR, FF, backslash, or `#{`-like sequences).
    let use_interpolation = elements.iter().any(|&elem| {
        if let NodeKind::Sym(sym) = *cx.kind(elem) {
            needs_percent_i(cx.symbol_str(sym))
        } else {
            false
        }
    });

    let range = cx.range(node);
    cx.emit_offense(range, "Use `%i` or `%I` for an array of symbols.", None);

    // Autocorrect: whole-node replacement with percent literal.
    let mut replacement = if use_interpolation {
        String::from("%I[")
    } else {
        String::from("%i[")
    };
    for (i, &elem) in elements.iter().enumerate() {
        if i > 0 {
            replacement.push(' ');
        }
        let NodeKind::Sym(sym) = *cx.kind(elem) else {
            unreachable!("checked above")
        };
        let name = cx.symbol_str(sym);
        if use_interpolation {
            replacement.push_str(&escape_for_percent_i(name));
        } else {
            replacement.push_str(name);
        }
    }
    replacement.push(']');
    cx.emit_edit(range, &replacement);
}

fn check_brackets_style(
    node: NodeId,
    _elements: &[NodeId],
    opts: &SymbolArrayOptions,
    cx: &Cx<'_>,
) {
    let array_src = cx.raw_source(cx.range(node));
    if !array_src.starts_with("%i") && !array_src.starts_with("%I") {
        return;
    }

    let Some(parts) = percent_symbol_tokens(array_src) else {
        return;
    };
    if parts.tokens.len() < opts.min_size as usize {
        return;
    };
    let replacement = build_bracketed_array_from_tokens(
        &parts.tokens,
        array_src,
        parts.interpolation_enabled,
        parts.close,
    );
    let range = cx.range(node);
    let is_multiline = array_src.contains('\n');
    let msg = if is_multiline {
        "Use an array literal `[...]` for an array of symbols.".to_string()
    } else {
        format!("Use `{replacement}` for an array of symbols.")
    };
    let offense_range = if is_multiline {
        Range {
            start: range.start,
            end: range.start + 3,
        }
    } else {
        range
    };
    cx.emit_offense(offense_range, &msg, None);
    cx.emit_edit(range, &replacement);
}

struct PercentSymbolParts {
    tokens: Vec<String>,
    interpolation_enabled: bool,
    close: char,
}

fn percent_symbol_tokens(src: &str) -> Option<PercentSymbolParts> {
    let mut chars = src.char_indices();
    let (_, percent) = chars.next()?;
    let (_, kind) = chars.next()?;
    if percent != '%' || !matches!(kind, 'i' | 'I') {
        return None;
    }
    let (open_idx, open) = chars.next()?;
    let close = matchpair_close(open);
    let close_idx = src.rfind(close)?;
    if close_idx <= open_idx {
        return None;
    }

    let body_start = open_idx + open.len_utf8();
    let body = &src[body_start..close_idx];
    Some(PercentSymbolParts {
        tokens: split_percent_symbol_body(body, close),
        interpolation_enabled: kind == 'I',
        close,
    })
}

fn split_percent_symbol_body(body: &str, close: char) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut interpolation_depth = 0usize;
    let mut chars = body.chars().peekable();

    while let Some(ch) = chars.next() {
        if escaped {
            if ch == '\\' || ch == close || ch.is_whitespace() {
                current.push(ch);
            } else {
                current.push('\\');
                current.push(ch);
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if interpolation_depth == 0 && ch == '#' && chars.peek() == Some(&'{') {
            current.push(ch);
            current.push(chars.next().expect("peeked"));
            interpolation_depth = 1;
            continue;
        }

        if interpolation_depth > 0 {
            match ch {
                '{' => interpolation_depth += 1,
                '}' => interpolation_depth = interpolation_depth.saturating_sub(1),
                _ => {}
            }
            current.push(ch);
            continue;
        }

        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }

        current.push(ch);
    }

    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn build_bracketed_array_from_tokens(
    tokens: &[String],
    src: &str,
    interpolation_enabled: bool,
    close: char,
) -> String {
    if tokens.is_empty() {
        return "[]".to_string();
    }

    if src.contains('\n') {
        let item_indent = first_item_indent(src, close);
        let close_indent = closing_delimiter_indent(src, close);
        let mut out = String::from("[\n");
        for (i, token) in tokens.iter().enumerate() {
            out.push_str(&item_indent);
            out.push_str(&to_symbol_literal(token, interpolation_enabled));
            if i + 1 < tokens.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&close_indent);
        out.push(']');
        return out;
    }

    let mut out = String::from("[");
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&to_symbol_literal(token, interpolation_enabled));
    }
    out.push(']');
    out
}

fn to_symbol_literal(name: &str, interpolation_enabled: bool) -> String {
    if interpolation_enabled && name.contains("#{") {
        return format!(":\"{}\"", escape_double_quoted_symbol(name));
    }
    if symbol_without_quote(name) {
        format!(":{name}")
    } else {
        format!(":'{}'", escape_single_quoted_symbol(name))
    }
}

fn escape_single_quoted_symbol(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '\\' || ch == '\'' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_double_quoted_symbol(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn first_item_indent(src: &str, close: char) -> String {
    src.lines()
        .skip(1)
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || (trimmed.len() == close.len_utf8() && trimmed.starts_with(close))
            {
                None
            } else {
                Some(line.chars().take_while(|c| c.is_whitespace()).collect())
            }
        })
        .unwrap_or_default()
}

fn closing_delimiter_indent(src: &str, close: char) -> String {
    src.lines()
        .rev()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.len() == close.len_utf8() && trimmed.starts_with(close) {
                Some(line.chars().take_while(|c| c.is_whitespace()).collect())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn matchpair_close(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c => c,
    }
}

fn symbol_without_quote(name: &str) -> bool {
    is_simple_identifier(name) || is_instance_or_class_variable(name) || is_global_variable(name)
}

fn is_instance_or_class_variable(name: &str) -> bool {
    let rest = if let Some(rest) = name.strip_prefix("@@") {
        rest
    } else if let Some(rest) = name.strip_prefix('@') {
        rest
    } else {
        return false;
    };
    is_variable_name(rest)
}

fn is_global_variable(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('$') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Returns `true` when `name` is a simple symbol identifier that can be used
/// bare inside `%i[…]`.
///
/// Accepted: `[a-zA-Z_][a-zA-Z0-9_]*[!?]?`
/// Rejected: names with spaces, quotes, brackets, slashes, or other
/// delimiters that would need quoting or would break `%i` parsing.
fn is_simple_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_alphabetic()) {
        return false;
    }
    // Check optional trailing `!` or `?`.
    let (body, tail) = match bytes.last() {
        Some(b'!' | b'?') if bytes.len() > 1 => (&bytes[1..bytes.len() - 1], true),
        _ => (&bytes[1..], false),
    };
    let _ = tail; // informational only
    body.iter().all(|&b| b == b'_' || b.is_ascii_alphanumeric())
}

/// Returns `true` when the symbol value contains "complex content" that
/// prevents it from being safely represented bare inside `%i[…]` or
/// `%I[…]`.
///
/// A value is "complex" when it contains:
/// 1. A literal space (` `), or
/// 2. An unbalanced delimiter character (`[` `]` `(` `)`) — after removing
///    balanced `[…]` and `(…)` pairs whose inner content contains no
///    whitespace or nested same-type delimiters.
///
/// Mirrors RuboCop 1.86.2 `Style/SymbolArray#complex_content?`.
fn complex_content(name: &str) -> bool {
    if name.contains(' ') {
        return true;
    }
    let stripped = strip_balanced_delimiter_pairs(name);
    stripped.contains('[')
        || stripped.contains(']')
        || stripped.contains('(')
        || stripped.contains(')')
}

/// Removes balanced `[…]` and `(…)` delimiter pairs from a symbol value
/// when the content between the delimiters contains no whitespace and no
/// nested same-type delimiters.
///
/// Mirrors the RuboCop regex `(\[[^\s\[\]]*\])|(\([^\s()]*\))`.
fn strip_balanced_delimiter_pairs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut j = i + 1;
            while j < chars.len() {
                let c = chars[j];
                if c == ']' {
                    i = j + 1;
                    break;
                }
                if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '[' || c == ']' {
                    out.push(chars[i]);
                    i += 1;
                    break;
                }
                j += 1;
            }
            if j >= chars.len() {
                out.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '(' {
            let mut j = i + 1;
            while j < chars.len() {
                let c = chars[j];
                if c == ')' {
                    i = j + 1;
                    break;
                }
                if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '(' || c == ')' {
                    out.push(chars[i]);
                    i += 1;
                    break;
                }
                j += 1;
            }
            if j >= chars.len() {
                out.push(chars[i]);
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Returns `true` when the symbol value contains characters that require
/// `%I` (interpolating percent array) instead of `%i` (non-interpolating).
///
/// These characters — tab, newline, carriage return, form feed, backslash,
/// and hash — cannot be represented literally inside `%i[…]` tokens or
/// would be misinterpreted by the `%I` parser without escaping.
fn needs_percent_i(name: &str) -> bool {
    name.contains('\\')
        || name.contains('\t')
        || name.contains('\n')
        || name.contains('\r')
        || name.contains('\x0C')
        || name.contains('#')
}

/// Escapes a symbol value for use inside `%I[…]`.
///
/// | Input char | Escape |
/// |---|---|
/// | `\\` | `\\\\` |
/// | `\t` (tab) | `\\t` |
/// | `\n` (newline) | `\\n` |
/// | `\r` (CR) | `\\r` |
/// | `\x0C` (FF) | `\\f` |
/// | `#` | `\\#` |
/// | Others | literal |
fn escape_for_percent_i(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0C' => out.push_str("\\f"),
            '#' => out.push_str("\\#"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, SymbolArray, SymbolArrayOptions};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- detection -----------------------------------------------------------

    fn brackets_options() -> SymbolArrayOptions {
        SymbolArrayOptions {
            enforced_style: EnforcedStyle::Brackets,
            min_size: 0,
        }
    }

    #[test]
    fn flags_bracket_symbol_array() {
        test::<SymbolArray>().expect_offense(indoc! {r#"
            x = [:foo, :bar]
                ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
        "#});
    }

    #[test]
    fn accepts_percent_literal_already() {
        test::<SymbolArray>().expect_no_offenses("x = %i[foo bar]\n");
    }

    #[test]
    fn brackets_style_accepts_bracket_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_no_offenses("x = [:one, :two, :three]\n");
    }

    #[test]
    fn brackets_style_flags_percent_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_offense(indoc! {r#"
                x = %i(one two three)
                    ^^^^^^^^^^^^^^^^^ Use `[:one, :two, :three]` for an array of symbols.
            "#});
    }

    #[test]
    fn brackets_style_corrects_percent_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %i(one @two $three four-five)
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `[:one, :@two, :$three, :'four-five']` for an array of symbols.
                "#},
                "x = [:one, :@two, :$three, :'four-five']\n",
            );
    }

    #[test]
    fn brackets_style_corrects_empty_percent_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %i()
                        ^^^^ Use `[]` for an array of symbols.
                "#},
                "x = []\n",
            );
    }

    #[test]
    fn brackets_style_corrects_percent_capital_i_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %I(one two)
                        ^^^^^^^^^^^ Use `[:one, :two]` for an array of symbols.
                "#},
                "x = [:one, :two]\n",
            );
    }

    #[test]
    fn brackets_style_corrects_percent_capital_i_with_interpolation() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r##"
                    x = %I(#{foo} #{foo}bar foo#{bar} foo)
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `[:"#{foo}", :"#{foo}bar", :"foo#{bar}", :foo]` for an array of symbols.
                "##},
                r##"x = [:"#{foo}", :"#{foo}bar", :"foo#{bar}", :foo]
"##,
            );
    }

    #[test]
    fn brackets_style_keeps_percent_lower_i_interpolation_literal() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r##"
                    x = %i(#{foo})
                        ^^^^^^^^^^ Use `[:'#{foo}']` for an array of symbols.
                "##},
                r##"x = [:'#{foo}']
"##,
            );
    }

    #[test]
    fn brackets_style_preserves_backslash_before_non_escapable_character() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %i(foo\bar)
                        ^^^^^^^^^^^ Use `[:'foo\\bar']` for an array of symbols.
                "#},
                "x = [:'foo\\\\bar']\n",
            );
    }

    #[test]
    fn brackets_style_uses_actual_closing_delimiter_indent() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %i{
                        ^^^ Use an array literal `[...]` for an array of symbols.
                        one
                        two
                      }
                "#},
                indoc! {r#"
                    x = [
                        :one,
                        :two
                      ]
                "#},
            );
    }

    #[test]
    fn brackets_style_corrects_multiline_percent_symbol_array() {
        test::<SymbolArray>()
            .with_options(&brackets_options())
            .expect_correction(
                indoc! {r#"
                    x = %i(
                        ^^^ Use an array literal `[...]` for an array of symbols.
                      one
                      two
                      three
                    )
                "#},
                indoc! {r#"
                    x = [
                      :one,
                      :two,
                      :three
                    ]
                "#},
            );
    }

    #[test]
    fn brackets_style_respects_min_size() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Brackets,
                min_size: 3,
            })
            .expect_no_offenses("x = %i(one two)\n");
    }

    #[test]
    fn accepts_single_element_below_min_size() {
        // Default MinSize = 2; one-element array is not flagged.
        test::<SymbolArray>().expect_no_offenses("x = [:foo]\n");
    }

    #[test]
    fn accepts_array_with_non_sym_element() {
        test::<SymbolArray>().expect_no_offenses("x = [:foo, 1]\n");
    }

    #[test]
    fn accepts_array_with_complex_symbol_name() {
        // Symbol with spaces or special chars — skip.
        test::<SymbolArray>().expect_no_offenses("x = [:\"foo bar\", :baz]\n");
    }

    #[test]
    fn flags_three_symbol_array() {
        test::<SymbolArray>().expect_offense(indoc! {r#"
            x = [:a, :b, :c]
                ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
        "#});
    }

    #[test]
    fn accepts_array_smaller_than_custom_min_size() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 3,
            })
            .expect_no_offenses("x = [:foo, :bar]\n");
    }

    #[test]
    fn flags_array_meeting_custom_min_size() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 3,
            })
            .expect_offense(indoc! {r#"
                x = [:a, :b, :c]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    // ---- autocorrect --------------------------------------------------------

    #[test]
    fn autocorrects_bracket_array_to_percent_literal() {
        test::<SymbolArray>().expect_correction(
            indoc! {r#"
                x = [:foo, :bar]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#},
            "x = %i[foo bar]\n",
        );
    }

    #[test]
    fn autocorrects_three_symbol_array() {
        test::<SymbolArray>().expect_correction(
            indoc! {r#"
                x = [:a, :b, :c]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#},
            "x = %i[a b c]\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        // After correction the result should not trigger another offense.
        test::<SymbolArray>().expect_no_offenses("x = %i[foo bar]\n");
    }

    // ---- predicate functions ------------------------------------------------

    #[test]
    fn simple_identifier_accepts_plain_words() {
        use super::is_simple_identifier;
        assert!(is_simple_identifier("foo"));
        assert!(is_simple_identifier("foo_bar"));
        assert!(is_simple_identifier("_private"));
        assert!(is_simple_identifier("FooBar"));
        assert!(is_simple_identifier("foo?"));
        assert!(is_simple_identifier("foo!"));
        assert!(is_simple_identifier("foo_bar?"));
    }

    #[test]
    fn simple_identifier_rejects_special_names() {
        use super::is_simple_identifier;
        assert!(!is_simple_identifier(""));
        assert!(!is_simple_identifier("foo bar"));
        assert!(!is_simple_identifier("1foo"));
        assert!(!is_simple_identifier("foo-bar"));
    }

    #[test]
    fn enforced_style_brackets_from_config_json() {
        let opts = SymbolArrayOptions::from_config_json(
            br#"{"EnforcedStyle": "brackets", "MinSize": 0}"#,
        )
        .expect("valid config");
        assert_eq!(opts.enforced_style, EnforcedStyle::Brackets);
        assert_eq!(opts.min_size, 0);
    }

    #[test]
    fn enforced_style_default_is_percent() {
        let opts = SymbolArrayOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Percent);
        assert_eq!(opts.min_size, 2);
    }

    // ---- percent-mode gap fixes (murphy-xzv0) --------------------------------

    #[test]
    fn percent_flags_instance_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:@foo, :@bar]
                    ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_instance_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:@foo, :@bar]
                        ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[@foo @bar]\n",
            );
    }

    #[test]
    fn percent_flags_class_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:@@foo, :@@bar]
                    ^^^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_class_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:@@foo, :@@bar]
                        ^^^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[@@foo @@bar]\n",
            );
    }

    #[test]
    fn percent_flags_global_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:$baz, :$qux]
                    ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_global_variable_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:$baz, :$qux]
                        ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[$baz $qux]\n",
            );
    }

    #[test]
    fn percent_flags_operator_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:==, :!=, :|]
                    ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_operator_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:==, :!=, :|]
                        ^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[== != |]\n",
            );
    }

    #[test]
    fn percent_flags_bracket_method_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:[]]
                    ^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_bracket_method_symbols() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:[]]
                        ^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[[]]\n",
            );
    }

    #[test]
    fn percent_flags_symbol_with_balanced_delimiters() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_offense(indoc! {r#"
                x = [:"foo[bar]", :"baz(qux)"]
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    #[test]
    fn percent_corrects_symbol_with_balanced_delimiters() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:"foo[bar]", :"baz(qux)"]
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %i[foo[bar] baz(qux)]\n",
            );
    }

    #[test]
    fn percent_skips_symbol_with_unbalanced_delimiter() {
        // `:"foo]"` has an unpaired `]` after removing balanced pairs.
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_no_offenses("x = [:one, :\"foo]\", :two]\n");
    }

    #[test]
    fn percent_skips_symbol_with_space_in_delimiters() {
        // `:[ ]` contains space inside delimiters → complex.
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_no_offenses("x = [:one, :\"[ ]\"]\n");
    }

    #[test]
    fn corrects_with_interpolation_for_tab() {
        // `:"\t"` contains a literal tab → needs `%I` with `\t` escape.
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions {
                enforced_style: EnforcedStyle::Percent,
                min_size: 0,
            })
            .expect_correction(
                indoc! {r#"
                    x = [:"\t", :three]
                        ^^^^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
                "#},
                "x = %I[\\t three]\n",
            );
    }

    // ---- complex_content / strip_balanced_delimiter_pairs / escape ----------

    #[test]
    fn complex_content_detects_space() {
        use super::complex_content;
        assert!(complex_content("foo bar"));
    }

    #[test]
    fn complex_content_detects_unbalanced_bracket() {
        use super::complex_content;
        assert!(complex_content("foo]"));
        assert!(complex_content("[bar"));
    }

    #[test]
    fn complex_content_accepts_balanced_delimiters() {
        use super::complex_content;
        assert!(!complex_content("foo[bar]"));
        assert!(!complex_content("baz(qux)"));
        assert!(!complex_content("[]"));
        assert!(!complex_content("()"));
    }

    #[test]
    fn complex_content_accepts_operators() {
        use super::complex_content;
        assert!(!complex_content("=="));
        assert!(!complex_content("!="));
        assert!(!complex_content("|"));
        assert!(!complex_content("[]"));
        assert!(!complex_content("[]="));
    }

    #[test]
    fn complex_content_accepts_variables() {
        use super::complex_content;
        assert!(!complex_content("@foo"));
        assert!(!complex_content("@@bar"));
        assert!(!complex_content("$baz"));
        assert!(!complex_content("$!"));
    }

    #[test]
    fn escape_for_percent_i_handles_control_chars() {
        use super::escape_for_percent_i;
        assert_eq!(escape_for_percent_i("\t"), "\\t");
        assert_eq!(escape_for_percent_i("\n"), "\\n");
        assert_eq!(escape_for_percent_i("\r"), "\\r");
        assert_eq!(escape_for_percent_i("foo"), "foo");
        assert_eq!(escape_for_percent_i("foo#bar"), "foo\\#bar");
        assert_eq!(escape_for_percent_i("foo\\bar"), "foo\\\\bar");
    }
}
murphy_plugin_api::submit_cop!(SymbolArray);
