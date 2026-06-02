//! `Style/NumericLiteralPrefix` — enforces lowercase prefixes for numeric literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NumericLiteralPrefix
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Flags uppercase hex prefix (`0X`) → autocorrects to `0x`.
//!     - Flags uppercase binary prefix (`0B`) → autocorrects to `0b`.
//!     - Flags decimal prefix (`0D`/`0d`) → autocorrects by stripping prefix.
//!     - EnforcedOctalStyle: zero_with_o (default) — flags bare `0NNN` (no `o`)
//!       and uppercase `0O` → autocorrects to `0o`.
//!     - EnforcedOctalStyle: zero_only — flags `0o`/`0O` → autocorrects to bare `0`.
//!     - Negative literals: sign is stripped before prefix matching (mirrors
//!       RuboCop's `integer_part` which does `source.sub(/^[+-]/, '').split(/[eE.]/).first`).
//!       The autocorrect edits the prefix bytes relative to the sign offset.
//!
//!   Gaps vs RuboCop:
//!     - RuboCop's HEX_REGEX is `^0X[0-9A-F]+$` — it only flags uppercase-X
//!       prefix AND uppercase A-F digits. Forms like `0Xabcd` (lowercase digits
//!       with uppercase X) are NOT flagged by RuboCop and are similarly skipped here.
//!     - Underscores in literals (e.g. `0X1_2`) are accepted by the regex matching
//!       (the regex doesn't include `_`, so `0X1_2` is not flagged — faithful parity).
//! ```
//!
//! ## Matched shapes
//!
//! `Int` nodes whose raw source has an uppercase prefix or a decimal prefix.
//!
//! ## Prefix detection
//!
//! The prefix is detected on the raw source after stripping any leading sign
//! (`+`/`-`) using simple string matching, not regex.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

const OCTAL_ZERO_ONLY_MSG: &str = "Use 0 for octal literals.";
const OCTAL_MSG: &str = "Use 0o for octal literals.";
const HEX_MSG: &str = "Use 0x for hexadecimal literals.";
const BINARY_MSG: &str = "Use 0b for binary literals.";
const DECIMAL_MSG: &str = "Do not use prefixes for decimal literals.";

/// Enforced style for octal literals.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedOctalStyle {
    /// Prefer `0o` prefix for octal literals (default).
    #[default]
    #[option(value = "zero_with_o")]
    ZeroWithO,
    /// Prefer bare `0` prefix for octal literals.
    #[option(value = "zero_only")]
    ZeroOnly,
}

/// Cop options for [`NumericLiteralPrefix`].
#[derive(CopOptions)]
pub struct NumericLiteralPrefixOptions {
    #[option(
        name = "EnforcedOctalStyle",
        default = "zero_with_o",
        description = "Preferred prefix style for octal literals."
    )]
    pub enforced_octal_style: EnforcedOctalStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct NumericLiteralPrefix;

/// What kind of prefix violation was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralType {
    Octal,
    OctalZeroOnly,
    Hex,
    Binary,
    Decimal,
}

#[cop(
    name = "Style/NumericLiteralPrefix",
    description = "Use smallcase prefixes for numeric literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NumericLiteralPrefixOptions,
)]
impl NumericLiteralPrefix {
    #[on_node(kind = "int")]
    fn check_int(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<NumericLiteralPrefixOptions>();
        let range = cx.range(node);
        let src = cx.raw_source(range);

        // Strip leading sign to get the integer part (mirrors RuboCop's integer_part).
        let sign_len = if src.starts_with('+') || src.starts_with('-') {
            1usize
        } else {
            0
        };
        let int_part = &src[sign_len..];

        let literal_type = classify(int_part, opts.enforced_octal_style);
        let Some(lt) = literal_type else {
            return;
        };

        let msg = match lt {
            LiteralType::Octal => OCTAL_MSG,
            LiteralType::OctalZeroOnly => OCTAL_ZERO_ONLY_MSG,
            LiteralType::Hex => HEX_MSG,
            LiteralType::Binary => BINARY_MSG,
            LiteralType::Decimal => DECIMAL_MSG,
        };
        cx.emit_offense(range, msg, None);
        emit_autocorrect(lt, range, sign_len as u32, int_part, cx);
    }
}

/// Classify the integer part (without sign) into a `LiteralType` if it's a violation.
fn classify(int_part: &str, style: EnforcedOctalStyle) -> Option<LiteralType> {
    let bytes = int_part.as_bytes();
    if bytes.len() < 2 {
        return None;
    }

    // Must start with '0'.
    if bytes[0] != b'0' {
        return None;
    }

    match bytes[1] {
        // Uppercase hex prefix: 0X followed by uppercase hex digits only.
        b'X' => {
            // Mirror RuboCop's HEX_REGEX = /^0X[0-9A-F]+$/.
            if !bytes[2..].is_empty()
                && bytes[2..].iter().all(|&b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))
            {
                Some(LiteralType::Hex)
            } else {
                None
            }
        }
        // Uppercase binary prefix: 0B followed by binary digits only.
        b'B' => {
            // Mirror RuboCop's BINARY_REGEX = /^0B[01]+$/.
            if !bytes[2..].is_empty() && bytes[2..].iter().all(|&b| matches!(b, b'0' | b'1')) {
                Some(LiteralType::Binary)
            } else {
                None
            }
        }
        // Decimal prefix: 0d or 0D.
        b'd' | b'D' => {
            // Mirror RuboCop's DECIMAL_REGEX = /^0[dD][0-9]+$/.
            if !bytes[2..].is_empty() && bytes[2..].iter().all(|&b| b.is_ascii_digit()) {
                Some(LiteralType::Decimal)
            } else {
                None
            }
        }
        // Uppercase octal prefix 0O.
        b'O' => {
            // Mirror RuboCop's OCTAL_ZERO_ONLY_REGEX = /^0[Oo][0-7]+$/.
            if !bytes[2..].is_empty()
                && bytes[2..].iter().all(|&b| matches!(b, b'0'..=b'7'))
            {
                match style {
                    EnforcedOctalStyle::ZeroWithO => Some(LiteralType::Octal),
                    EnforcedOctalStyle::ZeroOnly => Some(LiteralType::OctalZeroOnly),
                }
            } else {
                None
            }
        }
        // Lowercase octal prefix 0o — only bad under zero_only style.
        b'o' => {
            if style == EnforcedOctalStyle::ZeroOnly
                && !bytes[2..].is_empty()
                && bytes[2..].iter().all(|&b| matches!(b, b'0'..=b'7'))
            {
                Some(LiteralType::OctalZeroOnly)
            } else {
                None
            }
        }
        // Bare octal 0NNN (no prefix letter) — bad under zero_with_o style.
        b'0'..=b'7' => {
            if style == EnforcedOctalStyle::ZeroWithO
                && bytes[1..].iter().all(|&b| matches!(b, b'0'..=b'7'))
            {
                Some(LiteralType::Octal)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Emit the autocorrect edits.
///
/// The edit targets the prefix bytes inside the literal, offset by `sign_len`
/// bytes from `range.start` to skip any leading sign.
fn emit_autocorrect(
    lt: LiteralType,
    node_range: Range,
    sign_len: u32,
    int_part: &str,
    cx: &Cx<'_>,
) {
    let int_start = node_range.start + sign_len;
    match lt {
        // 0O... or 0NNN → 0o...
        LiteralType::Octal => {
            let int_bytes = int_part.as_bytes();
            if int_bytes.len() >= 2 && int_bytes[1] == b'O' {
                // Replace '0O' with '0o'.
                let prefix_range = Range {
                    start: int_start,
                    end: int_start + 2,
                };
                cx.emit_edit(prefix_range, "0o");
            } else {
                // Bare '0NNN' → insert 'o' after the leading '0'.
                let after_zero = Range {
                    start: int_start + 1,
                    end: int_start + 1,
                };
                cx.emit_edit(after_zero, "o");
            }
        }
        // 0O... or 0o... → 0NNN (strip the prefix letter).
        LiteralType::OctalZeroOnly => {
            // Delete the second byte (the 'O' or 'o').
            let prefix_byte = Range {
                start: int_start + 1,
                end: int_start + 2,
            };
            cx.emit_edit(prefix_byte, "");
        }
        // 0X... → 0x...
        LiteralType::Hex => {
            let x_pos = Range {
                start: int_start + 1,
                end: int_start + 2,
            };
            cx.emit_edit(x_pos, "x");
        }
        // 0B... → 0b...
        LiteralType::Binary => {
            let b_pos = Range {
                start: int_start + 1,
                end: int_start + 2,
            };
            cx.emit_edit(b_pos, "b");
        }
        // 0d... or 0D... → strip the two-byte prefix.
        LiteralType::Decimal => {
            let prefix = Range {
                start: int_start,
                end: int_start + 2,
            };
            cx.emit_edit(prefix, "");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::test;

    // --- Hex ---

    #[test]
    fn flags_uppercase_hex_prefix() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 0X12AB\n      ^^^^^^ Use 0x for hexadecimal literals.\n",
            "num = 0x12AB\n",
        );
    }

    #[test]
    fn accepts_lowercase_hex_prefix() {
        test::<NumericLiteralPrefix>().expect_no_offenses("num = 0x12AB\n");
    }

    #[test]
    fn accepts_hex_with_lowercase_digits() {
        test::<NumericLiteralPrefix>().expect_no_offenses("num = 0xabcd\n");
    }

    // --- Binary ---

    #[test]
    fn flags_uppercase_binary_prefix() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 0B10101\n      ^^^^^^^ Use 0b for binary literals.\n",
            "num = 0b10101\n",
        );
    }

    #[test]
    fn accepts_lowercase_binary_prefix() {
        test::<NumericLiteralPrefix>().expect_no_offenses("num = 0b10101\n");
    }

    // --- Decimal prefix ---

    #[test]
    fn flags_uppercase_decimal_prefix() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 0D1234\n      ^^^^^^ Do not use prefixes for decimal literals.\n",
            "num = 1234\n",
        );
    }

    #[test]
    fn flags_lowercase_decimal_prefix() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 0d1234\n      ^^^^^^ Do not use prefixes for decimal literals.\n",
            "num = 1234\n",
        );
    }

    #[test]
    fn accepts_plain_decimal() {
        test::<NumericLiteralPrefix>().expect_no_offenses("num = 1234\n");
    }

    // --- Octal (zero_with_o style, default) ---

    #[test]
    fn flags_uppercase_octal_prefix_zero_with_o() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 0O1234\n      ^^^^^^ Use 0o for octal literals.\n",
            "num = 0o1234\n",
        );
    }

    #[test]
    fn flags_bare_octal_zero_with_o() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = 01234\n      ^^^^^ Use 0o for octal literals.\n",
            "num = 0o1234\n",
        );
    }

    #[test]
    fn accepts_lowercase_octal_zero_with_o() {
        test::<NumericLiteralPrefix>().expect_no_offenses("num = 0o1234\n");
    }

    // --- Octal (zero_only style) ---

    #[test]
    fn flags_lowercase_octal_prefix_zero_only() {
        test::<NumericLiteralPrefix>()
            .with_options(&NumericLiteralPrefixOptions {
                enforced_octal_style: EnforcedOctalStyle::ZeroOnly,
            })
            .expect_correction(
                "num = 0o1234\n      ^^^^^^ Use 0 for octal literals.\n",
                "num = 01234\n",
            );
    }

    #[test]
    fn flags_uppercase_octal_prefix_zero_only() {
        test::<NumericLiteralPrefix>()
            .with_options(&NumericLiteralPrefixOptions {
                enforced_octal_style: EnforcedOctalStyle::ZeroOnly,
            })
            .expect_correction(
                "num = 0O1234\n      ^^^^^^ Use 0 for octal literals.\n",
                "num = 01234\n",
            );
    }

    #[test]
    fn accepts_bare_octal_zero_only() {
        test::<NumericLiteralPrefix>()
            .with_options(&NumericLiteralPrefixOptions {
                enforced_octal_style: EnforcedOctalStyle::ZeroOnly,
            })
            .expect_no_offenses("num = 01234\n");
    }

    // --- Negative literals ---

    #[test]
    fn flags_negative_uppercase_hex() {
        test::<NumericLiteralPrefix>().expect_correction(
            "num = -0X1F\n      ^^^^^^ Use 0x for hexadecimal literals.\n",
            "num = -0x1F\n",
        );
    }
}
murphy_plugin_api::submit_cop!(NumericLiteralPrefix);
