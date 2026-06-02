//! `Style/FormatStringToken` — enforces a consistent format string token style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FormatStringToken
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Enforced styles: annotated (`%<name>s`), template (`%{name}`), and
//!   unannotated (`%s`, `%d`, etc.) are all implemented.
//!   Mode: aggressive (default) and conservative are implemented.
//!   In aggressive mode: all `str` nodes are scanned; autocorrect only
//!   fires when the string is in a format context (format/sprintf/printf/%).
//!   In conservative mode: only strings in format contexts are scanned.
//!   MaxUnannotatedPlaceholdersAllowed (default: 1) is supported.
//!   AllowedMethods and AllowedPatterns are supported.
//!
//!   Autocorrect is available for annotated↔template conversions (swapping
//!   `%<name>type` ↔ `%{name}` when the sequence has a name). Unannotated
//!   style does not autocorrect (RuboCop parity).
//!
//!   Gaps vs RuboCop:
//!   - Heredoc body strings: offense range is computed from raw source
//!     heuristic; may be slightly off for complex heredoc indentation.
//!   - Percent-literal strings (%q{}, %Q{}): content start/end heuristic
//!     used (3-byte prefix, 1-byte suffix assumed).
//!   - Strings inside `xstr` (backtick) or `regexp` nodes are skipped.
//!   - `__FILE__` token is not excluded (minor parity gap).
//! ```
//!
//! ## Matched shapes
//!
//! `Str` nodes (plain string literals) that:
//! - Contain `%` in their raw source
//! - Are not in `xstr` or `regexp` ancestor nodes
//! - Have format-style tokens matching the three supported categories
//!
//! ## Offense
//!
//! The offense range covers the format token (e.g. `%{name}`, `%<name>s`,
//! or `%s`). The message says which style was detected and which is preferred.
//!
//! ## Autocorrect
//!
//! Rewrites `annotated` ↔ `template` when the token has a name.
//! Unannotated tokens are never autocorrected.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};


#[derive(Default)]
pub struct FormatStringToken;

/// Enforced format string style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FmtStyle {
    /// `%<name>s` style (default).
    #[default]
    #[option(value = "annotated")]
    Annotated,
    /// `%{name}` style.
    #[option(value = "template")]
    Template,
    /// `%s` / `%d` etc. style.
    #[option(value = "unannotated")]
    Unannotated,
}

impl FmtStyle {
    fn description(self) -> &'static str {
        match self {
            FmtStyle::Annotated => "annotated tokens (like `%<foo>s`)",
            FmtStyle::Template => "template tokens (like `%{foo}`)",
            FmtStyle::Unannotated => "unannotated tokens (like `%s`)",
        }
    }
}

/// Scan mode: aggressive (all strings) or conservative (only format contexts).
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScanMode {
    #[default]
    #[option(value = "aggressive")]
    Aggressive,
    #[option(value = "conservative")]
    Conservative,
}

#[derive(CopOptions)]
pub struct FormatStringTokenOptions {
    #[option(
        name = "EnforcedStyle",
        default = "annotated",
        description = "The token style to enforce."
    )]
    pub enforced_style: FmtStyle,

    #[option(
        name = "MaxUnannotatedPlaceholdersAllowed",
        default = 1,
        description = "Maximum number of unannotated placeholders allowed."
    )]
    pub max_unannotated_placeholders_allowed: i64,

    #[option(
        name = "Mode",
        default = "aggressive",
        description = "aggressive = check all strings; conservative = only format contexts."
    )]
    pub mode: ScanMode,

    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Method names to exempt from checking."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns matching method names to exempt."
    )]
    pub allowed_patterns: Vec<String>,
}

/// A detected format sequence within a string literal.
#[derive(Debug, Clone)]
struct FmtSeq {
    /// Detected style of this sequence.
    style: FmtStyle,
    /// The name (for annotated/template styles), e.g. `"greeting"`.
    name: Option<String>,
    /// Flags, e.g. `"-"`.
    flags: String,
    /// Width, e.g. `"10"`.
    width: String,
    /// Precision, e.g. `".5"`.
    precision: String,
    /// Conversion type, e.g. `"s"`, `"d"`.
    type_char: String,
    /// Byte offset of `%` within the string content slice.
    token_start: usize,
    /// Byte offset past the last char of the token.
    token_end: usize,
}

#[cop(
    name = "Style/FormatStringToken",
    description = "Use a consistent style for format string tokens.",
    default_severity = "warning",
    default_enabled = true,
    options = FormatStringTokenOptions,
)]
impl FormatStringToken {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check_str_node(node, cx);
    }
}

fn check_str_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Str(_) = *cx.kind(node) else {
        return;
    };

    // Skip strings inside xstr (backtick) or regexp.
    if cx
        .ancestors(node)
        .any(|a| matches!(*cx.kind(a), NodeKind::Xstr(_) | NodeKind::Regexp { .. }))
    {
        return;
    }

    let opts = cx.options_or_default::<FormatStringTokenOptions>();
    let node_range = cx.range(node);
    let raw = cx.raw_source(node_range);

    // Quick reject: must contain '%'.
    if !raw.contains('%') {
        return;
    }

    // Check AllowedMethods: skip if direct send parent is in allowed list.
    if is_allowed_method(node, cx, &opts) {
        return;
    }

    // Determine if the string is in a format context.
    let in_format_ctx = in_format_context(node, cx);

    // In conservative mode, only check strings in format context.
    if opts.mode == ScanMode::Conservative && !in_format_ctx {
        return;
    }

    // Find the content slice (raw between the delimiters).
    let (content_start, content_end) = string_content_range(raw, node_range.start);
    if content_start >= content_end {
        return;
    }
    let content_bytes = &raw.as_bytes()[content_start..content_end];

    // Collect all format sequences from the content.
    let seqs = scan_format_sequences(content_bytes);
    if seqs.is_empty() {
        return;
    }

    // Check named vs unannotated token counts without heap allocation.
    let has_named = seqs.iter().any(|s| s.style != FmtStyle::Unannotated);
    let unannotated_count = seqs.iter().filter(|s| s.style == FmtStyle::Unannotated).count();

    // Unannotated: treated as conservative regardless of mode, unless in format context.
    // If all are unannotated, apply MaxUnannotatedPlaceholdersAllowed.
    if !has_named {
        // All unannotated.
        let max = opts.max_unannotated_placeholders_allowed as usize;
        if unannotated_count <= max {
            return;
        }
        // All exceed the limit: check only in format context (or aggressive if > limit).
        // RuboCop: even in aggressive mode, unannotated tokens are treated conservatively
        // (only flagged in format context).
        if !in_format_ctx {
            return;
        }
    }

    // Process each detected sequence.
    let content_start_abs = node_range.start + content_start as u32;
    for seq in &seqs {
        if seq.style == opts.enforced_style {
            continue;
        }
        // Unannotated tokens: only flag in format context (conservative-like).
        if seq.style == FmtStyle::Unannotated && !in_format_ctx {
            continue;
        }

        let token_range = Range {
            start: content_start_abs + seq.token_start as u32,
            end: content_start_abs + seq.token_end as u32,
        };

        let msg = format!(
            "Prefer {} over {}.",
            opts.enforced_style.description(),
            seq.style.description()
        );

        // Only autocorrect when in format context.
        if in_format_ctx {
            cx.emit_offense(token_range, &msg, None);
            autocorrect_sequence(cx, token_range, seq, opts.enforced_style);
        } else {
            cx.emit_offense(token_range, &msg, None);
        }
    }
}

/// Returns true if the str node is a direct argument to format/sprintf/printf
/// or the left operand of `%`.
fn in_format_context(node: NodeId, cx: &Cx<'_>) -> bool {
    // Check parent.
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    // `format(str, ...)`, `sprintf(str, ...)`, `printf(str, ...)`
    // The string must be the first positional argument.
    if let NodeKind::Send { method, .. } = *cx.kind(parent) {
        let method_name = cx.symbol_str(method);
        if matches!(method_name, "format" | "sprintf" | "printf") {
            // String is the first arg.
            if let Some(&first_arg) = cx.call_arguments(parent).first()
                && first_arg == node {
                    return true;
                }
        }
        // `str % args` — the str is the receiver of `%`.
        if method_name == "%" && cx.call_receiver(parent).get() == Some(node) {
            return true;
        }
    }
    // Also check if the str is inside a dstr that is in format context.
    for ancestor in cx.ancestors(node) {
        if matches!(*cx.kind(ancestor), NodeKind::Dstr(_)) {
            if in_format_context(ancestor, cx) {
                return true;
            }
        } else {
            break;
        }
    }
    false
}

/// Checks if the string node's enclosing send method is in the AllowedMethods list.
fn is_allowed_method(
    node: NodeId,
    cx: &Cx<'_>,
    opts: &FormatStringTokenOptions,
) -> bool {
    if opts.allowed_methods.is_empty() && opts.allowed_patterns.is_empty() {
        return false;
    }
    // Find enclosing send.
    for ancestor in cx.ancestors(node) {
        if let NodeKind::Send { method, .. } = *cx.kind(ancestor) {
            let name = cx.symbol_str(method);
            if opts.allowed_methods.iter().any(|m| m == name) {
                return true;
            }
            // Pattern matching: simple substring match (RuboCop uses Regexp).
            if opts
                .allowed_patterns
                .iter()
                .any(|p| name.contains(p.as_str()))
            {
                return true;
            }
            break;
        }
    }
    false
}

/// Find the byte range of string content within `raw` (excluding delimiters).
/// Returns (content_start, content_end) as byte offsets within `raw`.
fn string_content_range(raw: &str, _abs_start: u32) -> (usize, usize) {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return (0, 0);
    }
    match bytes[0] {
        b'"' | b'\'' => {
            // Simple "..." or '...'
            let end = if bytes.last() == Some(&bytes[0]) {
                bytes.len().saturating_sub(1)
            } else {
                bytes.len()
            };
            (1, end)
        }
        b'%' => {
            // %q{...}, %Q{...}, %(...), etc.
            // Find the opening delimiter character.
            let prefix_end = bytes
                .iter()
                .position(|&b| b == b'{' || b == b'(' || b == b'[' || b == b'<' || b == b'|')
                .unwrap_or(2);
            let content_start = prefix_end + 1;
            let content_end = bytes.len().saturating_sub(1);
            (content_start, content_end)
        }
        _ => {
            // Fallback: treat whole as content (heredoc or unusual delimiter).
            (0, bytes.len())
        }
    }
}

/// Scan a content slice for format sequences.
///
/// Recognizes:
/// - `%<name>flags_width_precisiontype` (annotated)
/// - `%{name}` (template)
/// - `%[flags][width][.precision]type` (unannotated, where type is a format char)
/// - `%%` (literal percent, skipped)
fn scan_format_sequences(content: &[u8]) -> Vec<FmtSeq> {
    let mut seqs = Vec::new();
    let mut i = 0;
    while i < content.len() {
        if content[i] != b'%' {
            i += 1;
            continue;
        }
        let token_start = i;
        i += 1; // skip '%'
        if i >= content.len() {
            break;
        }
        // `%%` → literal percent, skip.
        if content[i] == b'%' {
            i += 1;
            continue;
        }
        // Try to parse an annotated token `%<name>flags_width_precisiontype`.
        if content[i] == b'<'
            && let Some((seq, end)) = parse_annotated(&content[token_start..], token_start) {
                seqs.push(seq);
                i = end;
                continue;
            }
        // Try to parse a template token `%{name}`.
        if content[i] == b'{'
            && let Some((seq, end)) = parse_template(&content[token_start..], token_start) {
                seqs.push(seq);
                i = end;
                continue;
            }
        // Try to parse an unannotated token `%[flags][width][.precision]type`.
        if let Some((seq, end)) = parse_unannotated(&content[token_start..], token_start) {
            seqs.push(seq);
            i = end;
            continue;
        }
        // Not a recognized format sequence.
        i += 1;
    }
    seqs
}

/// Parse `%<name>flags_width_precisiontype` from slice starting at `%`.
fn parse_annotated(slice: &[u8], base: usize) -> Option<(FmtSeq, usize)> {
    // slice[0] == '%', slice[1] == '<'
    if slice.len() < 4 || slice[1] != b'<' {
        return None;
    }
    let name_end = slice[2..].iter().position(|&b| b == b'>')?;
    let name_end_abs = 2 + name_end;
    let name = std::str::from_utf8(&slice[2..name_end_abs]).ok()?;
    let rest = &slice[name_end_abs + 1..]; // skip '>'
    let (flags, width, precision, type_char, len) = parse_flags_width_precision_type(rest)?;
    let total_len = 2 + name_end + 1 + len; // %< + name + > + rest_consumed
    Some((
        FmtSeq {
            style: FmtStyle::Annotated,
            name: Some(name.to_string()),
            flags,
            width,
            precision,
            type_char,
            token_start: base,
            token_end: base + total_len,
        },
        base + total_len,
    ))
}

/// Parse `%{name}` from slice starting at `%`.
fn parse_template(slice: &[u8], base: usize) -> Option<(FmtSeq, usize)> {
    if slice.len() < 3 || slice[1] != b'{' {
        return None;
    }
    let name_end = slice[2..].iter().position(|&b| b == b'}')?;
    let name_end_abs = 2 + name_end;
    let name = std::str::from_utf8(&slice[2..name_end_abs]).ok()?;
    let total_len = name_end_abs + 1; // %{ + name + }
    Some((
        FmtSeq {
            style: FmtStyle::Template,
            name: Some(name.to_string()),
            flags: String::new(),
            width: String::new(),
            precision: String::new(),
            type_char: String::new(),
            token_start: base,
            token_end: base + total_len,
        },
        base + total_len,
    ))
}

/// Parse `%[flags][width][.precision]type` from slice starting at `%`.
fn parse_unannotated(slice: &[u8], base: usize) -> Option<(FmtSeq, usize)> {
    if slice.len() < 2 {
        return None;
    }
    let rest = &slice[1..]; // skip '%'
    let (flags, width, precision, type_char, len) = parse_flags_width_precision_type(rest)?;
    if type_char.is_empty() {
        return None;
    }
    let total_len = 1 + len;
    Some((
        FmtSeq {
            style: FmtStyle::Unannotated,
            name: None,
            flags,
            width,
            precision,
            type_char,
            token_start: base,
            token_end: base + total_len,
        },
        base + total_len,
    ))
}

/// Parse `[flags][width][.precision]type` from `rest`, returning
/// (flags, width, precision, type_char, bytes_consumed).
/// Returns None if no valid type char is found.
fn parse_flags_width_precision_type(
    rest: &[u8],
) -> Option<(String, String, String, String, usize)> {
    let mut i = 0;
    // Flags: ` `, `#`, `+`, `-`, `0`
    let flag_start = i;
    while i < rest.len() && matches!(rest[i], b' ' | b'#' | b'+' | b'-' | b'0') {
        i += 1;
    }
    let flags = std::str::from_utf8(&rest[flag_start..i])
        .unwrap_or("")
        .to_string();
    // Width: `*` or digits, or `{...}` reference (skip for simplicity).
    let width_start = i;
    if i < rest.len() && rest[i] == b'*' {
        i += 1;
    } else {
        while i < rest.len() && rest[i].is_ascii_digit() {
            i += 1;
        }
    }
    let width = std::str::from_utf8(&rest[width_start..i])
        .unwrap_or("")
        .to_string();
    // Precision: `.` followed by `*` or digits.
    let precision_start = i;
    if i < rest.len() && rest[i] == b'.' {
        i += 1;
        if i < rest.len() && rest[i] == b'*' {
            i += 1;
        } else {
            while i < rest.len() && rest[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    let precision = std::str::from_utf8(&rest[precision_start..i])
        .unwrap_or("")
        .to_string();
    // Type char: must be a valid format type.
    if i >= rest.len() {
        return None;
    }
    let type_byte = rest[i];
    // Valid Ruby format types.
    if !matches!(
        type_byte,
        b's' | b'd'
            | b'i'
            | b'u'
            | b'o'
            | b'x'
            | b'X'
            | b'b'
            | b'B'
            | b'e'
            | b'E'
            | b'f'
            | b'g'
            | b'G'
            | b'a'
            | b'A'
            | b'c'
            | b'p'
    ) {
        return None;
    }
    i += 1;
    Some((
        flags,
        width,
        precision,
        char::from(type_byte).to_string(),
        i,
    ))
}

/// Emit an autocorrect edit for a format sequence conversion.
fn autocorrect_sequence(cx: &Cx<'_>, token_range: Range, seq: &FmtSeq, target: FmtStyle) {
    if target == FmtStyle::Unannotated {
        // Never autocorrect to unannotated.
        return;
    }
    let Some(name) = &seq.name else {
        // Cannot convert unannotated → named without a name.
        return;
    };
    let replacement = match target {
        FmtStyle::Annotated => {
            // → `%<name>flags_width_precisiontype`
            let type_char = if seq.type_char.is_empty() { "s" } else { &seq.type_char };
            format!("%<{}>{}{}{}{}",
                name,
                seq.flags,
                seq.width,
                seq.precision,
                type_char,
            )
        }
        FmtStyle::Template => {
            // → `%{name}` (flags/width/precision are lost)
            format!("%{{{}}}", name)
        }
        FmtStyle::Unannotated => return,
    };
    cx.emit_edit(token_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::{FormatStringToken, FormatStringTokenOptions, FmtStyle, ScanMode};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn option_defaults_match_rubocop() {
        let opts = FormatStringTokenOptions::default();
        assert_eq!(opts.enforced_style, FmtStyle::Annotated);
        assert_eq!(opts.max_unannotated_placeholders_allowed, 1);
        assert_eq!(opts.mode, ScanMode::Aggressive);
    }

    // --- annotated (default) mode ---

    #[test]
    fn flags_template_token_in_format_call() {
        test::<FormatStringToken>().expect_offense(indoc! {r#"
            format('%{greeting}', greeting: 'Hello')
                    ^^^^^^^^^^^ Prefer annotated tokens (like `%<foo>s`) over template tokens (like `%{foo}`).
        "#});
    }

    #[test]
    fn flags_template_token_outside_format_call_in_aggressive_mode() {
        test::<FormatStringToken>().expect_offense(indoc! {r#"
            x = '%{greeting}'
                 ^^^^^^^^^^^ Prefer annotated tokens (like `%<foo>s`) over template tokens (like `%{foo}`).
        "#});
    }

    #[test]
    fn accepts_annotated_token_in_default_mode() {
        test::<FormatStringToken>()
            .expect_no_offenses("format('%<greeting>s', greeting: 'Hello')\n");
    }

    #[test]
    fn corrects_template_to_annotated() {
        test::<FormatStringToken>().expect_correction(
            indoc! {r#"
                format('%{greeting}', greeting: 'Hello')
                        ^^^^^^^^^^^ Prefer annotated tokens (like `%<foo>s`) over template tokens (like `%{foo}`).
            "#},
            "format('%<greeting>s', greeting: 'Hello')\n",
        );
    }

    #[test]
    fn accepts_single_unannotated_token_with_default_max() {
        // MaxUnannotatedPlaceholdersAllowed defaults to 1.
        test::<FormatStringToken>()
            .expect_no_offenses("format('%06d', 10)\n");
    }

    #[test]
    fn flags_multiple_unannotated_tokens_exceeding_max() {
        test::<FormatStringToken>().expect_offense(indoc! {r#"
            format('%s %s.', 'Hello', 'world')
                    ^^ Prefer annotated tokens (like `%<foo>s`) over unannotated tokens (like `%s`).
                       ^^ Prefer annotated tokens (like `%<foo>s`) over unannotated tokens (like `%s`).
        "#});
    }

    // --- template mode ---

    #[test]
    fn flags_annotated_token_in_template_mode() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                enforced_style: FmtStyle::Template,
                ..FormatStringTokenOptions::default()
            })
            .expect_offense(indoc! {r#"
                format('%<greeting>s', greeting: 'Hello')
                        ^^^^^^^^^^^^ Prefer template tokens (like `%{foo}`) over annotated tokens (like `%<foo>s`).
            "#});
    }

    #[test]
    fn corrects_annotated_to_template() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                enforced_style: FmtStyle::Template,
                ..FormatStringTokenOptions::default()
            })
            .expect_correction(
                indoc! {r#"
                    format('%<greeting>s', greeting: 'Hello')
                            ^^^^^^^^^^^^ Prefer template tokens (like `%{foo}`) over annotated tokens (like `%<foo>s`).
                "#},
                "format('%{greeting}', greeting: 'Hello')\n",
            );
    }

    // --- unannotated mode ---

    #[test]
    fn flags_annotated_token_in_unannotated_mode() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                enforced_style: FmtStyle::Unannotated,
                ..FormatStringTokenOptions::default()
            })
            .expect_offense(indoc! {r#"
                format('%<greeting>s', greeting: 'Hello')
                        ^^^^^^^^^^^^ Prefer unannotated tokens (like `%s`) over annotated tokens (like `%<foo>s`).
            "#});
    }

    #[test]
    fn flags_template_token_in_unannotated_mode() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                enforced_style: FmtStyle::Unannotated,
                ..FormatStringTokenOptions::default()
            })
            .expect_offense(indoc! {r#"
                format('%{greeting}', greeting: 'Hello')
                        ^^^^^^^^^^^ Prefer unannotated tokens (like `%s`) over template tokens (like `%{foo}`).
            "#});
    }

    // --- conservative mode ---

    #[test]
    fn conservative_mode_skips_template_outside_format_context() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                mode: ScanMode::Conservative,
                ..FormatStringTokenOptions::default()
            })
            .expect_no_offenses("x = '%{greeting}'\n");
    }

    #[test]
    fn conservative_mode_flags_template_in_format_call() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                mode: ScanMode::Conservative,
                ..FormatStringTokenOptions::default()
            })
            .expect_offense(indoc! {r#"
                format('%{greeting}', greeting: 'Hello')
                        ^^^^^^^^^^^ Prefer annotated tokens (like `%<foo>s`) over template tokens (like `%{foo}`).
            "#});
    }

    // --- MaxUnannotatedPlaceholdersAllowed ---

    #[test]
    fn max_zero_flags_single_unannotated_in_format_ctx() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                max_unannotated_placeholders_allowed: 0,
                ..FormatStringTokenOptions::default()
            })
            .expect_offense(indoc! {r#"
                format('%06d', 10)
                        ^^^^ Prefer annotated tokens (like `%<foo>s`) over unannotated tokens (like `%s`).
            "#});
    }

    // --- AllowedMethods ---

    #[test]
    fn allowed_methods_skips_flagged_method() {
        test::<FormatStringToken>()
            .with_options(&FormatStringTokenOptions {
                allowed_methods: vec!["redirect".to_string()],
                ..FormatStringTokenOptions::default()
            })
            .expect_no_offenses("redirect('foo/%{bar_id}')\n");
    }

    // --- % operator ---

    #[test]
    fn flags_template_token_with_percent_operator() {
        test::<FormatStringToken>().expect_offense(indoc! {r#"
            '%{greeting}' % { greeting: 'Hello' }
             ^^^^^^^^^^^ Prefer annotated tokens (like `%<foo>s`) over template tokens (like `%{foo}`).
        "#});
    }
}
murphy_plugin_api::submit_cop!(FormatStringToken);
