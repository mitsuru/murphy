//! `Style/ExponentialNotation` — enforce consistent exponential notation style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ExponentialNotation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   All three styles from RuboCop are supported:
//!     - `scientific` (default): mantissa >= 1 and < 10
//!       Pattern: `^-?[1-9](\.\d*[0-9])?$`
//!     - `engineering`: exponent divisible by 3, mantissa in [0.1, 1000)
//!       Multiple regex checks ported from RuboCop source.
//!     - `integral`: mantissa is a whole number without trailing zeroes
//!       Pattern: `^-?[1-9](\d*[1-9])?$`
//!   No autocorrect -- RuboCop does not provide one either.
//!   Only floats containing 'e' or 'E' are checked; plain decimals are skipped.
//!   Detection uses raw_source since the AST value loses the notation form.
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # scientific (default)
//! # bad
//! 10e6   # mantissa 10 not in [1,10)
//! 0.3e4  # mantissa 0.3 < 1
//! # good
//! 1e7
//! 3.14
//!
//! # engineering
//! # bad
//! 3.2e7  # exponent 7 not divisible by 3
//! # good
//! 32e6
//!
//! # integral
//! # bad
//! 3.2e7  # mantissa has decimal part
//! # good
//! 32e6
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ExponentialNotation;

/// The enforced style for exponential notation.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExponentialNotationStyle {
    #[default]
    #[option(value = "scientific")]
    Scientific,
    #[option(value = "engineering")]
    Engineering,
    #[option(value = "integral")]
    Integral,
}

#[derive(CopOptions)]
pub struct ExponentialNotationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "scientific",
        description = "The style to enforce for exponential notation."
    )]
    pub enforced_style: ExponentialNotationStyle,
}

#[cop(
    name = "Style/ExponentialNotation",
    description = "When using exponential notation, favor a mantissa between 1 (inclusive) and 10 (exclusive).",
    default_severity = "warning",
    default_enabled = true,
    options = ExponentialNotationOptions,
)]
impl ExponentialNotation {
    #[on_node(kind = "float")]
    fn check_float(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ExponentialNotationOptions>();
    let raw = cx.raw_source(cx.range(node));

    // Only check floats that use exponential notation
    if !raw.contains('e') && !raw.contains('E') {
        return;
    }

    if offense(raw, opts.enforced_style) {
        let msg = message(opts.enforced_style);
        cx.emit_offense(cx.range(node), msg, None);
    }
}

/// Returns true if `raw` violates the given style.
fn offense(raw: &str, style: ExponentialNotationStyle) -> bool {
    match style {
        ExponentialNotationStyle::Scientific => !is_scientific(raw),
        ExponentialNotationStyle::Engineering => !is_engineering(raw),
        ExponentialNotationStyle::Integral => !is_integral(raw),
    }
}

/// scientific: mantissa matches `^-?[1-9](\.\d*[0-9])?$`
fn is_scientific(raw: &str) -> bool {
    let mantissa = split_mantissa(raw);
    is_scientific_mantissa(mantissa)
}

fn is_scientific_mantissa(mantissa: &str) -> bool {
    // ^-?[1-9](\.\d*[0-9])?$
    let m = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let mut chars = m.chars();
    let first = chars.next();
    match first {
        Some(c) if c.is_ascii_digit() && c != '0' => {}
        _ => return false,
    }
    // Optionally a decimal part: `\.\d*[0-9]`
    let rest: &str = chars.as_str();
    if rest.is_empty() {
        return true;
    }
    if !rest.starts_with('.') {
        return false;
    }
    let after_dot = &rest[1..];
    if after_dot.is_empty() {
        return false;
    }
    // All must be digits, last must be nonzero
    after_dot.bytes().all(|b| b.is_ascii_digit()) && after_dot.ends_with(|c: char| c != '0')
}

/// engineering: exponent divisible by 3, mantissa in [0.1, 1000)
fn is_engineering(raw: &str) -> bool {
    // Split on 'e' (case-insensitive, but source always lowercase in Ruby floats)
    let (mantissa, exponent) = split_exp(raw);

    // exponent must be a plain integer
    if !is_plain_integer(exponent) {
        return false;
    }
    // exponent divisible by 3
    let exp_val: i64 = match exponent.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if exp_val % 3 != 0 {
        return false;
    }

    // mantissa must not have 4+ consecutive digits (i.e. >= 1000)
    // RuboCop: `return false if /^-?\d{4}/.match?(mantissa)`
    let m = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let digit_count = m.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count >= 4 {
        return false;
    }
    // RuboCop: `return false if /^-?0\d/.match?(mantissa)` -- leading zero followed by digit
    if m.starts_with('0') && m.chars().nth(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return false;
    }
    // RuboCop: `return false if /^-?0.0/.match?(mantissa)` -- 0.0... (zero decimal)
    if m.starts_with("0.0") {
        return false;
    }

    true
}

/// integral: mantissa matches `^-?[1-9](\d*[1-9])?$` (whole number, no trailing zeros)
fn is_integral(raw: &str) -> bool {
    let mantissa = split_mantissa(raw);
    is_integral_mantissa(mantissa)
}

fn is_integral_mantissa(mantissa: &str) -> bool {
    // ^-?[1-9](\d*[1-9])?$
    let m = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let mut chars = m.chars();
    let first = chars.next();
    match first {
        Some(c) if c.is_ascii_digit() && c != '0' => {}
        _ => return false,
    }
    let rest: &str = chars.as_str();
    if rest.is_empty() {
        return true; // single non-zero digit
    }
    // All must be digits; last must be nonzero
    rest.bytes().all(|b| b.is_ascii_digit()) && rest.ends_with(|c: char| c != '0')
}

/// Split raw source into (mantissa, exponent) on 'e'/'E'.
/// Returns (raw, "") if no 'e' found.
fn split_exp(raw: &str) -> (&str, &str) {
    if let Some(pos) = raw.find('e').or_else(|| raw.find('E')) {
        (&raw[..pos], &raw[pos + 1..])
    } else {
        (raw, "")
    }
}

/// Returns just the mantissa (before 'e').
fn split_mantissa(raw: &str) -> &str {
    split_exp(raw).0
}

/// Returns true if `s` is a plain integer string (optional leading `-`, then digits).
fn is_plain_integer(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn message(style: ExponentialNotationStyle) -> &'static str {
    match style {
        ExponentialNotationStyle::Scientific => "Use a mantissa >= 1 and < 10.",
        ExponentialNotationStyle::Engineering => {
            "Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000."
        }
        ExponentialNotationStyle::Integral => {
            "Use an integer as mantissa, without trailing zero."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- scientific (default) ---

    #[test]
    fn scientific_accepts_1e7() {
        test::<ExponentialNotation>().expect_no_offenses("x = 1e7\n");
    }

    #[test]
    fn scientific_accepts_3e3() {
        test::<ExponentialNotation>().expect_no_offenses("x = 3e3\n");
    }

    #[test]
    fn scientific_accepts_1_17e6() {
        test::<ExponentialNotation>().expect_no_offenses("x = 1.17e6\n");
    }

    #[test]
    fn scientific_accepts_plain_decimal() {
        // No exponential notation -- skip entirely
        test::<ExponentialNotation>().expect_no_offenses("x = 3.14\n");
    }

    #[test]
    fn scientific_flags_10e6() {
        test::<ExponentialNotation>().expect_offense(indoc! {"
            x = 10e6
                ^^^^ Use a mantissa >= 1 and < 10.
        "});
    }

    #[test]
    fn scientific_flags_0_3e4() {
        test::<ExponentialNotation>().expect_offense(indoc! {"
            x = 0.3e4
                ^^^^^ Use a mantissa >= 1 and < 10.
        "});
    }

    #[test]
    fn scientific_flags_11_7e5() {
        test::<ExponentialNotation>().expect_offense(indoc! {"
            x = 11.7e5
                ^^^^^^ Use a mantissa >= 1 and < 10.
        "});
    }


    // --- engineering ---

    #[test]
    fn engineering_accepts_32e6() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_no_offenses("x = 32e6\n");
    }

    #[test]
    fn engineering_accepts_10e3() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_no_offenses("x = 10e3\n");
    }

    #[test]
    fn engineering_accepts_1_2e6() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_no_offenses("x = 1.2e6\n");
    }

    #[test]
    fn engineering_flags_3_2e7() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_offense(indoc! {"
                x = 3.2e7
                    ^^^^^ Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.
            "});
    }

    #[test]
    fn engineering_flags_0_1e5() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_offense(indoc! {"
                x = 0.1e5
                    ^^^^^ Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.
            "});
    }

    #[test]
    fn engineering_flags_12e5() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_offense(indoc! {"
                x = 12e5
                    ^^^^ Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.
            "});
    }

    #[test]
    fn engineering_flags_1232e6() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Engineering,
            })
            .expect_offense(indoc! {"
                x = 1232e6
                    ^^^^^^ Use an exponent divisible by 3 and a mantissa >= 0.1 and < 1000.
            "});
    }

    // --- integral ---

    #[test]
    fn integral_accepts_32e6() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_no_offenses("x = 32e6\n");
    }

    #[test]
    fn integral_accepts_1e4() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_no_offenses("x = 1e4\n");
    }

    #[test]
    fn integral_accepts_12e5() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_no_offenses("x = 12e5\n");
    }

    #[test]
    fn integral_flags_3_2e7() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_offense(indoc! {"
                x = 3.2e7
                    ^^^^^ Use an integer as mantissa, without trailing zero.
            "});
    }

    #[test]
    fn integral_flags_0_1e5() {
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_offense(indoc! {"
                x = 0.1e5
                    ^^^^^ Use an integer as mantissa, without trailing zero.
            "});
    }

    #[test]
    fn integral_flags_120e4() {
        // trailing zero: 120 ends in 0
        test::<ExponentialNotation>()
            .with_options(&ExponentialNotationOptions {
                enforced_style: ExponentialNotationStyle::Integral,
            })
            .expect_offense(indoc! {"
                x = 120e4
                    ^^^^^ Use an integer as mantissa, without trailing zero.
            "});
    }

    // --- no autocorrect ---
    // (No correction tests -- RuboCop provides no autocorrect for this cop)
}

murphy_plugin_api::submit_cop!(ExponentialNotation);
