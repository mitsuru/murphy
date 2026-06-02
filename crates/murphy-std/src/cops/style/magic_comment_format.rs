//! `Style/MagicCommentFormat` — enforce consistent formatting of magic
//! comments (separator style and capitalization).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MagicCommentFormat
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the default `EnforcedStyle: snake_case` and
//!   `DirectiveCapitalization: lowercase` configuration.  All five RuboCop
//!   magic-comment keywords are covered:
//!   `coding`/`encoding`, `frozen_string_literal`, `shareable_constant_value`,
//!   `rbs_inline`, and `typed`.  Emacs-style multi-directive comments
//!   (`# -*- a: 1; b: 2 -*-`) are also handled.
//!   `ValueCapitalization` (default: nil = any) is implemented; when set to
//!   `lowercase` or `uppercase` the value portion is also checked.
//!   The `kebab_case` `EnforcedStyle` is not implemented (deferred).
//!   Autocorrect normalises the directive key to the configured separator and
//!   capitalization, and the value to the configured capitalization.
//!   The cop is `Enabled: pending` upstream (disabled by default); Murphy
//!   follows that default.
//! ```
//!
//! ## What is checked
//!
//! Magic comments in the file's leading comment block (before the first
//! non-comment code) are checked.  For each recognised directive:
//!
//! - The separator (`_` vs `-`) must match `EnforcedStyle` (`snake_case` →
//!   underscores, `kebab_case` → hyphens).
//! - The directive capitalization must match `DirectiveCapitalization`
//!   (`lowercase` → all lower, `uppercase` → all upper, `nil` → any).
//! - The value capitalization must match `ValueCapitalization`
//!   (`nil` → any, `lowercase` → all lower, `uppercase` → all upper).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MagicCommentFormat;

#[derive(CopOptions)]
pub struct MagicCommentFormatOptions {
    #[option(
        name = "EnforcedStyle",
        default = "snake_case",
        description = "Separator style for magic comment directives: `snake_case` (underscores, default) or `kebab_case` (hyphens)."
    )]
    pub enforced_style: MagicCommentSeparatorStyle,

    #[option(
        name = "DirectiveCapitalization",
        default = "lowercase",
        description = "Required capitalization for directive keys: `lowercase`, `uppercase`, or empty string for any."
    )]
    pub directive_capitalization: DirectiveCapitalizationOption,

    #[option(
        name = "ValueCapitalization",
        default = "",
        description = "Required capitalization for directive values: `lowercase`, `uppercase`, or empty string for any (default)."
    )]
    pub value_capitalization: ValueCapitalizationOption,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagicCommentSeparatorStyle {
    #[option(value = "snake_case")]
    SnakeCase,
    #[option(value = "kebab_case")]
    KebabCase,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirectiveCapitalizationOption {
    #[option(value = "lowercase")]
    Lowercase,
    #[option(value = "uppercase")]
    Uppercase,
    /// nil — any capitalization accepted.
    #[option(value = "")]
    Any,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueCapitalizationOption {
    #[option(value = "lowercase")]
    Lowercase,
    #[option(value = "uppercase")]
    Uppercase,
    /// nil — any capitalization accepted (default).
    #[option(value = "")]
    Any,
}

const MSG_STYLE: &str = "Prefer {style} case for magic comments.";
const MSG_VALUE: &str = "Prefer {case} for magic comment values.";

#[cop(
    name = "Style/MagicCommentFormat",
    description = "Use a consistent style for magic comments.",
    default_severity = "warning",
    default_enabled = false,
    options = MagicCommentFormatOptions,
)]
impl MagicCommentFormat {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<MagicCommentFormatOptions>();
        let src = cx.source().as_bytes();

        // Compute the leading comment region end (mirroring cx internals).
        let region_end = leading_comment_region_end(src);

        for comment in cx.comments() {
            // Only inline comments in the leading region.
            if comment.range.start as usize > region_end {
                continue;
            }
            let text = cx.raw_source(comment.range);
            // Only own-line comments (no code before `#` on the same line).
            if !is_own_line_comment(src, comment.range.start as usize) {
                continue;
            }
            // Parse and check any magic comment directives in this comment.
            check_comment(text, comment.range.start, &opts, cx);
        }
    }
}

/// Compute the byte offset of the end of the leading comment region.
/// Mirrors `Cx::leading_comment_region_end` (private) so we can replicate
/// the same boundary.
fn leading_comment_region_end(source: &[u8]) -> usize {
    let mut line_start = 0;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(source.len(), |pos| line_start + pos);
        let mut content_end = line_end;
        if content_end > line_start && source[content_end - 1] == b'\r' {
            content_end -= 1;
        }

        if line_start == 0 && source.starts_with(b"#!") {
            line_start = line_end.saturating_add(1);
            continue;
        }

        let mut first = line_start;
        while first < content_end && source[first].is_ascii_whitespace() {
            first += 1;
        }
        if first < content_end && source[first] == b'#' {
            line_start = line_end.saturating_add(1);
            continue;
        }
        return line_start;
    }
    source.len()
}

/// Return true iff the comment at `start` is an own-line comment (no code
/// before `#` on the same line).
fn is_own_line_comment(source: &[u8], start: usize) -> bool {
    let line_start = source[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    source[line_start..start]
        .iter()
        .all(|byte| byte.is_ascii_whitespace())
}

/// Check a single comment for magic comment format issues.
///
/// Handles:
/// - Normal: `# directive: value`
/// - Emacs: `# -*- directive: value -*-` and `# -*- d1: v1; d2: v2 -*-`
fn check_comment(text: &str, base_offset: u32, opts: &MagicCommentFormatOptions, cx: &Cx<'_>) {
    let bytes = text.as_bytes();

    // Must start with `#`.
    if bytes.first() != Some(&b'#') {
        return;
    }

    // Check for emacs-style comment `# -*- ... -*-`.
    let inner = if let Some(stripped) = strip_emacs_markers(text) {
        // Split on `;` for multiple directives.
        for part in stripped.split(';') {
            check_directive_part(part, base_offset, text, opts, cx);
        }
        return;
    } else {
        // Normal `# directive: value`
        text
    };

    check_directive_part(inner, base_offset, inner, opts, cx);
}

/// Parse one `directive: value` part from a comment and emit offenses.
fn check_directive_part(
    part: &str,
    base_offset: u32,
    full_comment: &str,
    opts: &MagicCommentFormatOptions,
    cx: &Cx<'_>,
) {
    let bytes = part.as_bytes();

    // Find the start of the directive (skip `#` and leading spaces).
    let mut key_start = 0;
    while key_start < bytes.len() && (bytes[key_start] == b'#' || bytes[key_start] == b' ' || bytes[key_start] == b'\t') {
        key_start += 1;
    }
    if key_start >= bytes.len() {
        return;
    }

    // Read the key: alphanumeric, `_`, `-`.
    let mut key_end = key_start;
    while key_end < bytes.len()
        && (bytes[key_end].is_ascii_alphanumeric()
            || bytes[key_end] == b'_'
            || bytes[key_end] == b'-')
    {
        key_end += 1;
    }
    if key_start == key_end {
        return;
    }

    let key = &part[key_start..key_end];

    // Check if this key is a recognised magic comment keyword.
    if !is_magic_comment_keyword(key) {
        return;
    }

    // Skip separator and whitespace to find the value.
    let mut sep = key_end;
    while sep < bytes.len() && bytes[sep].is_ascii_whitespace() {
        sep += 1;
    }
    if !matches!(bytes.get(sep), Some(b':' | b'=')) {
        return;
    }
    let mut value_start = sep + 1;
    while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
        value_start += 1;
    }
    let mut value_end = bytes.len();
    while value_end > value_start && bytes[value_end - 1].is_ascii_whitespace() {
        value_end -= 1;
    }

    // Compute byte offsets relative to the full comment.
    // `part` is a substring of `full_comment`; find where it starts.
    let part_offset = full_comment
        .find(part)
        .unwrap_or(0);
    let key_range_start = base_offset + (part_offset + key_start) as u32;
    let key_range_end = base_offset + (part_offset + key_end) as u32;

    let value_text = if value_end > value_start {
        &part[value_start..value_end]
    } else {
        ""
    };
    let value_range_start = base_offset + (part_offset + value_start) as u32;
    let value_range_end = base_offset + (part_offset + value_end) as u32;

    // --- Check directive separator style ---
    let has_hyphen = key.contains('-');
    let has_underscore = key.contains('_');
    let separator_offends = match opts.enforced_style {
        MagicCommentSeparatorStyle::SnakeCase => has_hyphen,
        MagicCommentSeparatorStyle::KebabCase => has_underscore && key.len() > 1,
    };

    // --- Check directive capitalization ---
    let directive_cap_offends = match opts.directive_capitalization {
        DirectiveCapitalizationOption::Lowercase => key != key.to_ascii_lowercase(),
        DirectiveCapitalizationOption::Uppercase => key != key.to_ascii_uppercase(),
        DirectiveCapitalizationOption::Any => false,
    };

    let directive_offends = separator_offends || directive_cap_offends;

    if directive_offends {
        let style_label = style_label(opts);
        let msg = MSG_STYLE.replace("{style}", &style_label);
        let key_range = Range {
            start: key_range_start,
            end: key_range_end,
        };
        cx.emit_offense(key_range, &msg, None);

        // Autocorrect: replace the directive key with the corrected version.
        let corrected = correct_key(key, opts);
        cx.emit_edit(key_range, &corrected);
    }

    // --- Check value capitalization ---
    if !value_text.is_empty() {
        let value_offends = match opts.value_capitalization {
            ValueCapitalizationOption::Lowercase => value_text != value_text.to_ascii_lowercase(),
            ValueCapitalizationOption::Uppercase => value_text != value_text.to_ascii_uppercase(),
            ValueCapitalizationOption::Any => false,
        };
        if value_offends {
            let case_label = match opts.value_capitalization {
                ValueCapitalizationOption::Lowercase => "lowercase",
                ValueCapitalizationOption::Uppercase => "uppercase",
                ValueCapitalizationOption::Any => "any",
            };
            let msg = MSG_VALUE.replace("{case}", case_label);
            let val_range = Range {
                start: value_range_start,
                end: value_range_end,
            };
            cx.emit_offense(val_range, &msg, None);

            // Autocorrect: correct the value.
            let corrected_value = match opts.value_capitalization {
                ValueCapitalizationOption::Lowercase => value_text.to_ascii_lowercase(),
                ValueCapitalizationOption::Uppercase => value_text.to_ascii_uppercase(),
                ValueCapitalizationOption::Any => value_text.to_string(),
            };
            cx.emit_edit(val_range, &corrected_value);
        }
    }
}

/// The human-readable style label used in offense messages.
fn style_label(opts: &MagicCommentFormatOptions) -> String {
    let sep = match opts.enforced_style {
        MagicCommentSeparatorStyle::SnakeCase => "snake",
        MagicCommentSeparatorStyle::KebabCase => "kebab",
    };
    let cap = match opts.directive_capitalization {
        DirectiveCapitalizationOption::Lowercase => Some("lowercase"),
        DirectiveCapitalizationOption::Uppercase => Some("uppercase"),
        DirectiveCapitalizationOption::Any => None,
    };
    match cap {
        Some(c) => format!("{} {}", c, sep),
        None => sep.to_string(),
    }
}

/// Produce the corrected directive key given the style options.
fn correct_key(key: &str, opts: &MagicCommentFormatOptions) -> String {
    let sep_char = match opts.enforced_style {
        MagicCommentSeparatorStyle::SnakeCase => '_',
        MagicCommentSeparatorStyle::KebabCase => '-',
    };
    // Replace both `_` and `-` with the correct separator.
    let replaced: String = key
        .chars()
        .map(|c| if c == '_' || c == '-' { sep_char } else { c })
        .collect();
    // Apply capitalization.
    match opts.directive_capitalization {
        DirectiveCapitalizationOption::Lowercase => replaced.to_ascii_lowercase(),
        DirectiveCapitalizationOption::Uppercase => replaced.to_ascii_uppercase(),
        DirectiveCapitalizationOption::Any => replaced,
    }
}

/// Check if a directive key (case-insensitively normalised) is a recognised
/// Ruby magic comment keyword.
///
/// RuboCop's KEYWORDS:
///   encoding: `(?:en)?coding`
///   frozen_string_literal: `frozen[_-]string[_-]literal`
///   rbs_inline: `rbs_inline`  (separator fixed as `_`)
///   shareable_constant_value: `shareable[_-]constant[_-]value`
///   typed: `typed`
fn is_magic_comment_keyword(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    // `coding` or `encoding`
    if lower == "coding" || lower == "encoding" {
        return true;
    }
    // `frozen_string_literal` or `frozen-string-literal` (mixed allowed)
    if matches_frozen_string_literal(&lower) {
        return true;
    }
    // `rbs_inline`
    if lower == "rbs_inline" || lower == "rbs-inline" {
        return true;
    }
    // `shareable_constant_value` or `shareable-constant-value` (mixed allowed)
    if matches_shareable_constant_value(&lower) {
        return true;
    }
    // `typed`
    if lower == "typed" {
        return true;
    }
    false
}

/// Match `frozen[_-]string[_-]literal` (case-insensitive, already lowercased).
fn matches_frozen_string_literal(lower: &str) -> bool {
    // Must start with "frozen" then sep, then "string" then sep, then "literal".
    if lower.len() < "frozen_string_literal".len() {
        return false;
    }
    if !lower.starts_with("frozen") {
        return false;
    }
    let rest = &lower["frozen".len()..];
    let sep1 = rest.as_bytes().first();
    if !matches!(sep1, Some(b'_' | b'-')) {
        return false;
    }
    let rest = &rest[1..];
    if !rest.starts_with("string") {
        return false;
    }
    let rest = &rest["string".len()..];
    let sep2 = rest.as_bytes().first();
    if !matches!(sep2, Some(b'_' | b'-')) {
        return false;
    }
    let rest = &rest[1..];
    rest == "literal"
}

/// Match `shareable[_-]constant[_-]value` (case-insensitive, already lowercased).
fn matches_shareable_constant_value(lower: &str) -> bool {
    if lower.len() < "shareable_constant_value".len() {
        return false;
    }
    if !lower.starts_with("shareable") {
        return false;
    }
    let rest = &lower["shareable".len()..];
    let sep1 = rest.as_bytes().first();
    if !matches!(sep1, Some(b'_' | b'-')) {
        return false;
    }
    let rest = &rest[1..];
    if !rest.starts_with("constant") {
        return false;
    }
    let rest = &rest["constant".len()..];
    let sep2 = rest.as_bytes().first();
    if !matches!(sep2, Some(b'_' | b'-')) {
        return false;
    }
    let rest = &rest[1..];
    rest == "value"
}

/// If the comment text is an emacs-style `# -*- ... -*-` comment, return the
/// inner content (between the `-*- ` markers).  Otherwise return `None`.
fn strip_emacs_markers(text: &str) -> Option<&str> {
    // Must contain `-*-`
    let start = text.find("-*-")?;
    let after_start = start + 3;
    let end = text[after_start..].find("-*-")?;
    Some(text[after_start..after_start + end].trim())
}

#[cfg(test)]
mod tests {
    use super::{
        DirectiveCapitalizationOption, MagicCommentFormat, MagicCommentFormatOptions,
        MagicCommentSeparatorStyle, ValueCapitalizationOption,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts_snake_lower() -> MagicCommentFormatOptions {
        MagicCommentFormatOptions {
            enforced_style: MagicCommentSeparatorStyle::SnakeCase,
            directive_capitalization: DirectiveCapitalizationOption::Lowercase,
            value_capitalization: ValueCapitalizationOption::Any,
        }
    }

    // --- no offense ---

    #[test]
    fn accepts_snake_case_lowercase_frozen() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_no_offenses("# frozen_string_literal: true\n");
    }

    #[test]
    fn accepts_snake_case_lowercase_encoding() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_no_offenses("# encoding: utf-8\n");
    }

    #[test]
    fn accepts_empty_file() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_no_offenses("");
    }

    #[test]
    fn accepts_comment_after_code() {
        // Comment after code is not in the leading region — not checked.
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_no_offenses("x = 1\n# frozen-string-literal: true\n");
    }

    // --- kebab separator offense ---

    #[test]
    fn flags_kebab_frozen_string_literal_in_snake_mode() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_offense(indoc! {r#"
                # frozen-string-literal: true
                  ^^^^^^^^^^^^^^^^^^^^^  Prefer lowercase snake case for magic comments.
            "#});
    }

    #[test]
    fn flags_kebab_encoding() {
        // `encoding` has no separator so only capitalization applies.
        // Test with uppercase encoding instead.
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_offense(indoc! {r#"
                # ENCODING: utf-8
                  ^^^^^^^^ Prefer lowercase snake case for magic comments.
            "#});
    }

    // --- capitalization offense ---

    #[test]
    fn flags_uppercase_directive() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_offense(indoc! {r#"
                # FROZEN_STRING_LITERAL: true
                  ^^^^^^^^^^^^^^^^^^^^^ Prefer lowercase snake case for magic comments.
            "#});
    }

    #[test]
    fn flags_mixed_case_directive() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_offense(indoc! {r#"
                # Frozen_String_Literal: true
                  ^^^^^^^^^^^^^^^^^^^^^ Prefer lowercase snake case for magic comments.
            "#});
    }

    // --- value capitalization ---

    #[test]
    fn flags_uppercase_value_when_lowercase_required() {
        test::<MagicCommentFormat>()
            .with_options(&MagicCommentFormatOptions {
                enforced_style: MagicCommentSeparatorStyle::SnakeCase,
                directive_capitalization: DirectiveCapitalizationOption::Lowercase,
                value_capitalization: ValueCapitalizationOption::Lowercase,
            })
            .expect_offense(indoc! {r#"
                # frozen_string_literal: TRUE
                                         ^^^^ Prefer lowercase for magic comment values.
            "#});
    }

    #[test]
    fn accepts_correct_value_when_lowercase_required() {
        test::<MagicCommentFormat>()
            .with_options(&MagicCommentFormatOptions {
                enforced_style: MagicCommentSeparatorStyle::SnakeCase,
                directive_capitalization: DirectiveCapitalizationOption::Lowercase,
                value_capitalization: ValueCapitalizationOption::Lowercase,
            })
            .expect_no_offenses("# frozen_string_literal: true\n");
    }

    // --- autocorrect ---

    #[test]
    fn autocorrects_kebab_to_snake() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_correction(
                indoc! {r#"
                    # frozen-string-literal: true
                      ^^^^^^^^^^^^^^^^^^^^^  Prefer lowercase snake case for magic comments.
                "#},
                "# frozen_string_literal: true\n",
            );
    }

    #[test]
    fn autocorrects_uppercase_directive_to_lowercase() {
        test::<MagicCommentFormat>()
            .with_options(&opts_snake_lower())
            .expect_correction(
                indoc! {r#"
                    # FROZEN_STRING_LITERAL: true
                      ^^^^^^^^^^^^^^^^^^^^^ Prefer lowercase snake case for magic comments.
                "#},
                "# frozen_string_literal: true\n",
            );
    }
}
murphy_plugin_api::submit_cop!(MagicCommentFormat);
