//! `Style/NumericLiterals` — add underscores to large numeric literals to
//! improve readability.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NumericLiterals
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Non-decimal literals (0x…, 0b…, 0o…, and legacy octal 0NNN) are skipped
//!   with a TODO comment in RuboCop's source; same behavior here.
//!   AllowedNumbers is supported (Vec<String>, compared as digit-only strings
//!   against the raw integer part).
//!   AllowedPatterns (regex) is not supported — derive only covers Vec<String>;
//!   users relying on AllowedPatterns in .rubocop.yml will not get the same
//!   exemption in Murphy.
//!   Strict mode is supported: in non-strict mode, a short leading group is
//!   allowed (e.g. `10_000`); in strict mode every group must be exactly 3.
//!   MinDigits defaults to 5 (matching RuboCop default).
//!   Detection and autocorrect operate on raw source (the AST value loses
//!   underscores and original radix prefix).
//!   The offense range covers the entire literal node.
//! ```
//!
//! ## Detection
//!
//! Subscribes to `int` and `float` nodes. For each:
//! - Skip non-decimal literals (raw source starts with `0x`, `0b`, `0o`, etc.)
//! - Extract the integer part: skip a leading `-`, then take bytes before
//!   the first `.`, `e`, or `E`.
//! - Count digit characters (ignoring underscores).
//! - If digit count < MinDigits, skip.
//! - Check if AllowedNumbers contains the raw integer part (digits only).
//! - Offense when `\d{4}` or (non-strict) interior short group or (strict)
//!   any short group after an underscore.
//!
//! ## Autocorrect
//!
//! Replaces the integer-part sub-range within the literal with the canonical
//! form (underscores every 3 digits from the right). Surgical edit on only
//! the integer-part bytes; sign and decimal/exponent suffix are preserved.

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, cop};

const MSG: &str =
    "Use underscores(_) as thousands separator and separate every 3 digits with them.";

#[derive(Default)]
pub struct NumericLiterals;

#[derive(CopOptions)]
pub struct NumericLiteralsOptions {
    #[option(
        name = "MinDigits",
        default = 5,
        description = "Minimum digit count in a numeric literal before underscores are required."
    )]
    pub min_digits: i64,

    #[option(
        name = "Strict",
        default = false,
        description = "When true, every underscore group must be exactly 3 digits."
    )]
    pub strict: bool,

    #[option(
        name = "AllowedNumbers",
        default = [],
        description = "Specific numbers (as strings) that are exempt from this cop."
    )]
    pub allowed_numbers: Vec<String>,
}

#[cop(
    name = "Style/NumericLiterals",
    description = "Add underscores to large numeric literals to improve their readability.",
    default_severity = "warning",
    default_enabled = true,
    options = NumericLiteralsOptions,
)]
impl NumericLiterals {
    #[on_node(kind = "int")]
    fn check_int(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_node(node, cx);
    }

    #[on_node(kind = "float")]
    fn check_float(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_node(node, cx);
    }
}

impl NumericLiterals {
    fn check_node(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<NumericLiteralsOptions>();
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        let bytes = src.as_bytes();

        // Skip non-decimal literals (hex, binary, octal).
        // RuboCop: "TODO: handle non-decimal literals as well"
        // A leading `-` may precede the `0`-prefixed form (e.g. `-0xFF`).
        let after_sign = if bytes.first() == Some(&b'-') {
            &bytes[1..]
        } else {
            bytes
        };
        if after_sign.len() >= 2 && after_sign[0] == b'0' {
            let second = after_sign[1];
            // 0x/0X (hex), 0b/0B (binary), 0o/0O (new-style octal),
            // or legacy octal (0 followed by a digit or underscore, e.g. `01234567`).
            if matches!(second, b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
                || second.is_ascii_digit()
                || second == b'_'
            {
                return;
            }
        }

        // Find integer-part byte offsets within `src`.
        // The integer part starts after any leading `-` and ends before the
        // first `.`, `e`, or `E`.
        let int_start = usize::from(bytes.first() == Some(&b'-'));
        let int_end = bytes[int_start..]
            .iter()
            .position(|&b| b == b'.' || b == b'e' || b == b'E')
            .map(|p| int_start + p)
            .unwrap_or(bytes.len());

        let int_part = &bytes[int_start..int_end];
        let int_str = match std::str::from_utf8(int_part) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Count actual digits (ignoring underscores).
        let digit_count = int_str.bytes().filter(|b| b.is_ascii_digit()).count();

        // Skip if below the MinDigits threshold (>= comparison, matching RuboCop).
        if digit_count < opts.min_digits as usize {
            return;
        }

        // AllowedNumbers: compare digit-only form of integer part as a string.
        let stripped_int: String = int_str
            .bytes()
            .filter(|b| b.is_ascii_digit())
            .map(|b| b as char)
            .collect();
        if opts.allowed_numbers.iter().any(|n| {
            let n_stripped: String = n
                .bytes()
                .filter(|b| b.is_ascii_digit())
                .map(|b| b as char)
                .collect();
            n_stripped == stripped_int
        }) {
            return;
        }

        // Check if formatting is already correct.
        if is_correctly_formatted(int_str, opts.strict) {
            return;
        }

        cx.emit_offense(node_range, MSG, None);

        // Autocorrect: replace only the integer-part bytes with canonical form.
        let formatted = format_int_part(&stripped_int);
        let int_part_range = Range {
            start: node_range.start + int_start as u32,
            end: node_range.start + int_end as u32,
        };
        cx.emit_edit(int_part_range, &formatted);
    }
}

/// Returns `true` if the integer part string is already correctly formatted.
///
/// A correctly formatted integer:
/// - Has no run of 4+ consecutive digits, AND
/// - In non-strict mode: no interior short group (`_\d{1,2}_`).
/// - In strict mode: no short group after any underscore (`_\d{1,2}(_|$)`).
fn is_correctly_formatted(int_str: &str, strict: bool) -> bool {
    !has_four_consecutive_digits(int_str) && !has_short_group(int_str, strict)
}

/// Returns `true` if `s` contains 4 or more consecutive ASCII digit chars.
fn has_four_consecutive_digits(s: &str) -> bool {
    let mut run = 0u32;
    for b in s.bytes() {
        if b.is_ascii_digit() {
            run += 1;
            if run >= 4 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Returns `true` if `s` has a short underscore group that violates the style.
///
/// Non-strict: offend on `_\d{1,2}_` (interior short group).
/// Strict: offend on `_\d{1,2}(_|$)` (any group after `_` shorter than 3).
fn has_short_group(s: &str, strict: bool) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'_' {
            i += 1;
            continue;
        }
        // Found an underscore. Count digits until next `_` or end.
        let j = i + 1;
        let mut k = j;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        let group_len = k - j;
        if group_len < 3 {
            if strict {
                // Strict: any short group after `_` is invalid.
                return true;
            }
            // Non-strict: only interior short group (followed by another `_`).
            if k < bytes.len() && bytes[k] == b'_' {
                return true;
            }
        }
        i = k;
    }
    false
}

/// Format a digit-only string with underscores every 3 digits from the right.
///
/// Examples: `"10000"` → `"10_000"`, `"1000000"` → `"1_000_000"`.
fn format_int_part(digits: &str) -> String {
    if digits.is_empty() {
        return String::new();
    }
    // Build via reversal: reverse, insert `_` every 3 positions, reverse back.
    let rev: Vec<u8> = digits.bytes().rev().collect();
    let mut result = Vec::with_capacity(digits.len() + digits.len() / 3);
    for (i, &b) in rev.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(b'_');
        }
        result.push(b);
    }
    result.reverse();
    // SAFETY: input was ASCII digits only.
    unsafe { String::from_utf8_unchecked(result) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- format_int_part unit tests ---

    #[test]
    fn format_five_digits() {
        assert_eq!(format_int_part("10000"), "10_000");
    }

    #[test]
    fn format_six_digits() {
        assert_eq!(format_int_part("100000"), "100_000");
    }

    #[test]
    fn format_seven_digits() {
        assert_eq!(format_int_part("1000000"), "1_000_000");
    }

    #[test]
    fn format_nine_digits() {
        assert_eq!(format_int_part("100000000"), "100_000_000");
    }

    #[test]
    fn format_three_digits_no_sep() {
        assert_eq!(format_int_part("100"), "100");
    }

    // --- is_correctly_formatted unit tests ---

    #[test]
    fn correct_non_strict_allows_short_leading() {
        // `10_000` has a 2-digit leading group — valid in non-strict mode.
        assert!(is_correctly_formatted("10_000", false));
    }

    #[test]
    fn correct_non_strict_full_groups() {
        assert!(is_correctly_formatted("100_000", false));
    }

    #[test]
    fn incorrect_four_consecutive_digits() {
        assert!(!is_correctly_formatted("10000", false));
    }

    #[test]
    fn incorrect_interior_short_group_non_strict() {
        // `1_00_000` has an interior group of 2 — invalid even non-strict.
        assert!(!is_correctly_formatted("1_00_000", false));
    }

    #[test]
    fn correct_strict_leading_group_two_digits() {
        // `10_000` — leading group of 2 before the first `_`.
        // RuboCop's strict regex `/_\d{1,2}(_|$)/` only fires on groups
        // *after* an underscore; the leading group has no preceding `_`,
        // so it is NOT flagged even in strict mode.
        assert!(is_correctly_formatted("10_000", true));
    }

    #[test]
    fn incorrect_strict_trailing_short_group() {
        // `1_00` — `_00` matches `/_\d{1,2}(_|$)/` (trailing) in strict mode.
        assert!(!is_correctly_formatted("1_00", true));
    }

    #[test]
    fn correct_non_strict_trailing_short_group() {
        // `1_00` — NOT flagged in non-strict mode; regex requires `_\d{1,2}_`.
        assert!(is_correctly_formatted("1_00", false));
    }

    #[test]
    fn correct_non_strict_trailing_two_digit_group_at_end() {
        // `100_00` — NOT flagged in non-strict mode.
        // RuboCop's non-strict regex `/_\d{1,2}_/` requires the short group to
        // be surrounded by underscores on both sides; `_00$` (trailing) does not
        // match, so `100_00` passes in non-strict mode.  Strict mode catches it
        // via `/_\d{1,2}(_|$)/`.
        assert!(is_correctly_formatted("100_00", false));
        assert!(!is_correctly_formatted("100_00", true));
    }

    #[test]
    fn correct_strict_three_groups() {
        assert!(is_correctly_formatted("100_000", true));
    }

    // --- cop detection tests ---

    #[test]
    fn accepts_small_integer_below_min_digits() {
        // 4-digit number is below default MinDigits=5 — no offense.
        test::<NumericLiterals>().expect_no_offenses("x = 9999\n");
    }

    #[test]
    fn flags_five_digit_unformatted() {
        // Exactly MinDigits=5 and unformatted → offense.
        test::<NumericLiterals>().expect_offense(indoc! {"
            x = 10000
                ^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
        "});
    }

    #[test]
    fn accepts_correctly_formatted_five_digit() {
        test::<NumericLiterals>().expect_no_offenses("x = 10_000\n");
    }

    #[test]
    fn accepts_correctly_formatted_six_digit() {
        test::<NumericLiterals>().expect_no_offenses("x = 100_000\n");
    }

    #[test]
    fn accepts_correctly_formatted_large_integer() {
        test::<NumericLiterals>().expect_no_offenses("x = 1_000_000\n");
    }

    #[test]
    fn flags_large_integer_without_underscores() {
        test::<NumericLiterals>().expect_offense(indoc! {"
            x = 1000000
                ^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
        "});
    }

    #[test]
    fn accepts_hex_literal() {
        // 0xFFFF00 — non-decimal, always skipped.
        test::<NumericLiterals>().expect_no_offenses("x = 0xFFFF00\n");
    }

    #[test]
    fn accepts_binary_literal() {
        test::<NumericLiterals>().expect_no_offenses("x = 0b10000000\n");
    }

    #[test]
    fn accepts_octal_literal() {
        test::<NumericLiterals>().expect_no_offenses("x = 0o77777\n");
    }

    #[test]
    fn accepts_legacy_octal_literal() {
        // `01234567` is a legacy Ruby octal literal — non-decimal, skip.
        test::<NumericLiterals>().expect_no_offenses("x = 01234567\n");
    }

    #[test]
    fn accepts_negative_legacy_octal_literal() {
        test::<NumericLiterals>().expect_no_offenses("x = -01234567\n");
    }

    #[test]
    fn flags_float_with_unformatted_integer_part() {
        test::<NumericLiterals>().expect_offense(indoc! {"
            x = 10000.5
                ^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
        "});
    }

    #[test]
    fn accepts_correctly_formatted_float() {
        test::<NumericLiterals>().expect_no_offenses("x = 10_000.5\n");
    }

    #[test]
    fn accepts_small_float_decimal_only() {
        // Float with fewer than MinDigits in integer part.
        test::<NumericLiterals>().expect_no_offenses("x = 1.23456\n");
    }

    #[test]
    fn accepts_allowed_number() {
        test::<NumericLiterals>()
            .with_options(&NumericLiteralsOptions {
                min_digits: 5,
                strict: false,
                allowed_numbers: vec!["10000".to_string()],
            })
            .expect_no_offenses("x = 10000\n");
    }

    #[test]
    fn flags_interior_short_group_non_strict() {
        // `1_00_000` is malformed even in non-strict mode.
        test::<NumericLiterals>().expect_offense(indoc! {"
            x = 1_00_000
                ^^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
        "});
    }

    #[test]
    fn strict_mode_accepts_ten_thousand_with_underscore() {
        // `10_000` is valid even in strict mode because the leading group
        // (`10`) has no preceding underscore and is not caught by the regex.
        test::<NumericLiterals>()
            .with_options(&NumericLiteralsOptions {
                min_digits: 5,
                strict: true,
                allowed_numbers: vec![],
            })
            .expect_no_offenses("x = 10_000
");
    }

    #[test]
    fn strict_mode_flags_trailing_short_group() {
        // `1_00_000_00` — trailing `_00` after the last underscore is flagged.
        // We use a 9-digit example `100_000_00` where the trailing group is short.
        // Actually use 8 digits: `1_000_00` → trailing `_00` → offense.
        test::<NumericLiterals>()
            .with_options(&NumericLiteralsOptions {
                min_digits: 5,
                strict: true,
                allowed_numbers: vec![],
            })
            .expect_offense(indoc! {"
                x = 1_000_00
                    ^^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
            "});
    }

    #[test]
    fn accepts_short_leading_group_non_strict() {
        test::<NumericLiterals>()
            .with_options(&NumericLiteralsOptions {
                min_digits: 5,
                strict: false,
                allowed_numbers: vec![],
            })
            .expect_no_offenses("x = 10_000\n");
    }

    // --- autocorrect tests ---

    #[test]
    fn autocorrects_five_digit_integer() {
        test::<NumericLiterals>().expect_correction(
            indoc! {"
                x = 10000
                    ^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
            "},
            "x = 10_000\n",
        );
    }

    #[test]
    fn autocorrects_large_integer() {
        test::<NumericLiterals>().expect_correction(
            indoc! {"
                x = 1000000
                    ^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
            "},
            "x = 1_000_000\n",
        );
    }

    #[test]
    fn autocorrects_float_integer_part_only() {
        test::<NumericLiterals>().expect_correction(
            indoc! {"
                x = 10000.5
                    ^^^^^^^ Use underscores(_) as thousands separator and separate every 3 digits with them.
            "},
            "x = 10_000.5\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent_for_formatted() {
        test::<NumericLiterals>().expect_no_offenses("x = 10_000\n");
    }

    // --- config tests ---

    #[test]
    fn options_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts = NumericLiteralsOptions::from_config_json(
            br#"{"MinDigits": 4, "Strict": true, "AllowedNumbers": ["9999"]}"#,
        )
        .expect("valid config");
        assert_eq!(opts.min_digits, 4);
        assert!(opts.strict);
        assert_eq!(opts.allowed_numbers, vec!["9999"]);
    }

    #[test]
    fn default_options() {
        let opts = NumericLiteralsOptions::default();
        assert_eq!(opts.min_digits, 5);
        assert!(!opts.strict);
        assert!(opts.allowed_numbers.is_empty());
    }
}

murphy_plugin_api::submit_cop!(NumericLiterals);
