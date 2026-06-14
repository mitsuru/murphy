//! `Layout/EndOfLine` — enforce a consistent line-ending convention.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EndOfLine
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `Layout/EndOfLine`. Three `EnforcedStyle` values:
//!   `native` (default), `lf`, and `crlf`. `native` resolves to the host
//!   platform's convention — `crlf` on Windows, `lf` elsewhere; Murphy is
//!   built/run on non-Windows in CI so `native` resolves to `lf` via
//!   `cfg!(windows)`. For `lf` a line ending in `\r` or `\r\n` is a
//!   "Carriage return character detected." offense; for `crlf` a line not
//!   ending in `\r\n` is a "Carriage return character missing." offense.
//!   Following upstream, only the FIRST offending line is reported and the
//!   scan stops there (line endings are consistent across a file in
//!   practice). The final unterminated line is exempt under `crlf`
//!   (`unimportant_missing_cr?`). The offense range spans the full line
//!   (including the terminator), matching upstream's
//!   `source_range(buffer, line, 0, line.length)`.
//!   Gaps (documented, not bypassed):
//!     - No autocorrect. Upstream's `Layout/EndOfLine` is also non-
//!       autocorrecting (CR/LF normalization is left to the user's editor /
//!       git config), so there is no correction to mirror.
//!     - Scan stops at the last token's line in upstream (`last_line`);
//!       Murphy scans the whole raw source. The only observable difference
//!       would be trailing blank lines after the last token having a
//!       different ending than the code — vanishingly rare and harmless
//!       (still a real offense if present).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EndOfLine;

const MSG_DETECTED: &str = "Carriage return character detected.";
const MSG_MISSING: &str = "Carriage return character missing.";

/// Options for [`EndOfLine`]. `EnforcedStyle` mirrors RuboCop verbatim.
#[derive(CopOptions)]
pub struct EndOfLineOptions {
    #[option(
        name = "EnforcedStyle",
        default = "native",
        description = "Line-ending convention: `native` (platform default), `lf`, or `crlf`."
    )]
    pub enforced_style: EndOfLineStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum EndOfLineStyle {
    /// CR+LF on Windows, LF elsewhere.
    #[option(value = "native")]
    Native,
    /// LF on all platforms.
    #[option(value = "lf")]
    Lf,
    /// CR+LF on all platforms.
    #[option(value = "crlf")]
    Crlf,
}

/// The effective style after resolving `native` to the host platform.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectiveStyle {
    Lf,
    Crlf,
}

impl EndOfLineStyle {
    /// `native` → `crlf` on Windows, `lf` elsewhere. Matches RuboCop's
    /// `Platform.windows?` check.
    fn effective(self) -> EffectiveStyle {
        match self {
            EndOfLineStyle::Native => {
                if cfg!(windows) {
                    EffectiveStyle::Crlf
                } else {
                    EffectiveStyle::Lf
                }
            }
            EndOfLineStyle::Lf => EffectiveStyle::Lf,
            EndOfLineStyle::Crlf => EffectiveStyle::Crlf,
        }
    }
}

#[cop(
    name = "Layout/EndOfLine",
    description = "Enforce a consistent line-ending convention (Unix-style by default).",
    default_severity = "warning",
    default_enabled = true,
    options = EndOfLineOptions,
)]
impl EndOfLine {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<EndOfLineOptions>();
        let effective = opts.enforced_style.effective();
        let bytes = cx.source().as_bytes();

        // Walk lines the way Ruby's `each_line` does: each line includes its
        // trailing `\n`; the final unterminated chunk is its own line.
        let mut line_start = 0usize;
        let mut i = 0usize;
        let total = bytes.len();
        while i < total {
            if bytes[i] == b'\n' {
                // Line is `[line_start, i]` inclusive of the `\n`.
                let line_end = i + 1;
                let has_newline = true;
                if offense_message(bytes, line_start, line_end, has_newline, effective).is_some() {
                    emit(cx, line_start, line_end);
                    // Report only the first offense, then stop — line endings
                    // are consistent across a file in practice (upstream
                    // `break`).
                    return;
                }
                line_start = line_end;
            }
            i += 1;
        }
        // Final unterminated line (no trailing `\n`), if any.
        if line_start < total {
            let line_end = total;
            let has_newline = false;
            if offense_message(bytes, line_start, line_end, has_newline, effective).is_some() {
                emit(cx, line_start, line_end);
            }
        }
    }
}

/// Returns the offense message for `[line_start, line_end)` if the line
/// violates `effective`, or `None` if it conforms.
///
/// `has_newline` is true when the line ends in `\n` (i.e. it is not the
/// unterminated final line). Under `crlf`, an unterminated final line is
/// exempt from the "missing CR" rule (`unimportant_missing_cr?`).
fn offense_message(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    has_newline: bool,
    effective: EffectiveStyle,
) -> Option<&'static str> {
    let line = &bytes[line_start..line_end];
    match effective {
        // lf: any line ending in `\r` or `\r\n` is an offense.
        EffectiveStyle::Lf => ends_with_cr(line).then_some(MSG_DETECTED),
        // crlf: any line NOT ending in `\r\n` is an offense, except an
        // unterminated final line (no `\n` at all → nothing to require a CR
        // before).
        EffectiveStyle::Crlf => {
            if !has_newline {
                // `unimportant_missing_cr?`: no LF on the last line → don't
                // care about a missing CR.
                return None;
            }
            if ends_with_crlf(line) {
                None
            } else {
                Some(MSG_MISSING)
            }
        }
    }
}

/// True if the line ends in `\r` (bare CR) or `\r\n` (CRLF). Mirrors
/// `line.end_with?("\r", "\r\n")`.
fn ends_with_cr(line: &[u8]) -> bool {
    if line.last() == Some(&b'\r') {
        return true;
    }
    // `\r\n`: the byte before the trailing `\n` is `\r`.
    matches!(line, [.., b'\r', b'\n'])
}

/// True if the line ends in exactly `\r\n`.
fn ends_with_crlf(line: &[u8]) -> bool {
    matches!(line, [.., b'\r', b'\n'])
}

/// Emit the whole-line offense covering `[line_start, line_end)`.
///
/// Both messages are static and selected by the style-independent shape of
/// the line (a CR-ending line is a "detected" offense; otherwise it is a
/// "missing" offense). Re-deriving here keeps the emit call signature small
/// and matches what `offense_message` decided for the active style.
fn emit(cx: &Cx<'_>, line_start: usize, line_end: usize) {
    let range = Range {
        start: line_start as u32,
        end: line_end as u32,
    };
    let bytes = cx.source().as_bytes();
    let line = &bytes[line_start..line_end];
    let msg = if ends_with_cr(line) {
        MSG_DETECTED
    } else {
        MSG_MISSING
    };
    cx.emit_offense(range, msg, None);
}

#[cfg(test)]
mod tests {
    use super::{EndOfLine, EndOfLineOptions, EndOfLineStyle, MSG_DETECTED, MSG_MISSING};
    use murphy_plugin_api::Range;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_options, test};

    // The `expect_offense` annotation harness builds its cleaned source via
    // `str::lines().join("\n")`, which strips every `\r`. So `\r` cannot be
    // represented in annotated source and offense-firing tests must drive the
    // cop with the raw-source helpers (`run_cop` / `run_cop_with_options`),
    // which pass the literal bytes (CR included) straight through. Clean
    // (`expect_no_offenses`) cases route raw `src` unchanged and CAN carry
    // `\r`, so those use the fluent `test()` builder.

    fn lf() -> EndOfLineOptions {
        EndOfLineOptions {
            enforced_style: EndOfLineStyle::Lf,
        }
    }

    fn crlf() -> EndOfLineOptions {
        EndOfLineOptions {
            enforced_style: EndOfLineStyle::Crlf,
        }
    }

    // ----- EnforcedStyle: lf ----------------------------------------

    #[test]
    fn lf_accepts_unix_line_endings() {
        test::<EndOfLine>()
            .with_options(&lf())
            .expect_no_offenses("puts 'hello'\nputs 'world'\n");
    }

    #[test]
    fn lf_flags_crlf_line() {
        // "x = 1\r\n": the whole line (bytes 0..7) is the offense range.
        let offenses = run_cop_with_options::<EndOfLine>("x = 1\r\n", &lf());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG_DETECTED);
        assert_eq!(offenses[0].range, Range { start: 0, end: 7 });
    }

    #[test]
    fn lf_reports_only_first_offense() {
        // Both lines are CRLF; only the first is reported (upstream `break`).
        // "a\r\nb\r\n": first line is bytes 0..3.
        let offenses = run_cop_with_options::<EndOfLine>("a\r\nb\r\n", &lf());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG_DETECTED);
        assert_eq!(offenses[0].range, Range { start: 0, end: 3 });
    }

    #[test]
    fn lf_flags_bare_cr_at_eof() {
        // A final line ending in a bare `\r` (no trailing `\n`) is flagged.
        // "x = 1\r": whole final chunk is bytes 0..6.
        let offenses = run_cop_with_options::<EndOfLine>("x = 1\r", &lf());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG_DETECTED);
        assert_eq!(offenses[0].range, Range { start: 0, end: 6 });
    }

    // ----- EnforcedStyle: crlf --------------------------------------

    #[test]
    fn crlf_accepts_windows_line_endings() {
        test::<EndOfLine>()
            .with_options(&crlf())
            .expect_no_offenses("puts 'hello'\r\nputs 'world'\r\n");
    }

    #[test]
    fn crlf_flags_lf_line() {
        // "x = 1\n": LF-only line is missing the CR. Whole line is 6 bytes.
        let offenses = run_cop_with_options::<EndOfLine>("x = 1\n", &crlf());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG_MISSING);
        assert_eq!(offenses[0].range, Range { start: 0, end: 6 });
    }

    #[test]
    fn crlf_exempts_unterminated_final_line() {
        // The last line has no `\n` at all → missing CR is unimportant.
        // First line is proper CRLF, so no offense anywhere.
        test::<EndOfLine>()
            .with_options(&crlf())
            .expect_no_offenses("x = 1\r\ny = 2");
    }

    // ----- EnforcedStyle: native (default) --------------------------

    #[cfg(not(windows))]
    #[test]
    fn native_default_accepts_lf_on_non_windows() {
        // On the (non-Windows) CI/build host, `native` resolves to `lf`, so
        // Unix endings are clean with default options.
        test::<EndOfLine>().expect_no_offenses("puts 'hello'\nputs 'world'\n");
    }

    #[cfg(not(windows))]
    #[test]
    fn native_default_flags_crlf_on_non_windows() {
        // `native` → `lf` on non-Windows, so a CRLF line is flagged with
        // default options (no explicit EnforcedStyle).
        let offenses = run_cop::<EndOfLine>("a\r\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, MSG_DETECTED);
        assert_eq!(offenses[0].range, Range { start: 0, end: 3 });
    }
}

murphy_plugin_api::submit_cop!(EndOfLine);
