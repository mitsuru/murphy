//! `Lint/MixedRegexpCaptureTypes` — flags mixing named and numbered capture groups.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MixedRegexpCaptureTypes
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/MixedRegexpCaptureTypes. Uses a linear byte-level
//!   scanner on the regexp body to detect both named `(?<name>...)`/`(?'name'...)`
//!   and numbered `(...)` capture groups.
//! ```
//!
//! ## Matched shapes
//! - `/(?<foo>FOO)(BAR)/` — mixing named and numbered capture groups
//! - `/(BAR)(?<foo>FOO)/` — mixing numbered and named capture groups
//!
//! ## No autocorrect
//!
//! This cop has no safe autocorrect. The user must decide whether to convert
//! numbered captures to named or to non-capturing groups.

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

const MSG: &str = "Do not mix named captures and numbered captures in a Regexp literal.";

#[derive(Default)]
pub struct MixedRegexpCaptureTypes;

#[cop(
    name = "Lint/MixedRegexpCaptureTypes",
    description = "Do not mix named captures and numbered captures in a Regexp literal.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MixedRegexpCaptureTypes {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
            return;
        };

        let parts_list = cx.list(parts);
        // Skip interpolated regexps — we cannot reliably determine capture types.
        if parts_list.len() != 1 {
            return;
        }
        if !matches!(cx.kind(parts_list[0]), NodeKind::Str(_)) {
            return;
        }

        let part_range = cx.range(parts_list[0]);
        let source = cx.raw_source(part_range);
        let (has_named, has_numbered) = scan_capture_types(source);

        if has_named && has_numbered {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

/// Scan a regexp body string for named and numbered capture groups.
///
/// Returns `(has_named, has_numbered)`.
///
/// Recognised groups:
/// - `(?<name>...)` / `(?'name'...)` — named captures
/// - `(...)` — numbered captures
/// - `(?:...)` — non-capturing (skipped)
/// - `(?=...)` / `(?!...)` — lookahead (skipped)
/// - `(?<=...)` / `(?<!...)` — lookbehind (skipped)
/// - `(?>...)` — atomic group (skipped)
/// - `(?~...)` — absence group (skipped)
/// - `(?flags:...)` / `(?flags-flags:...)` — groups with flags (skipped)
/// - `[` … `]` — character classes (skipped)
/// - `\(` — escaped paren
fn scan_capture_types(source: &str) -> (bool, bool) {
    let bytes = source.as_bytes();
    let mut has_named = false;
    let mut has_numbered = false;
    let mut i = 0usize;
    let len = bytes.len();
    let mut cc_depth = 0u32;

    while i < len {
        match bytes[i] {
            b'\\' => {
                i += 2;
            }
            b'[' => {
                cc_depth += 1;
                i += 1;
            }
            b']' => {
                cc_depth = cc_depth.saturating_sub(1);
                i += 1;
            }
            b'(' => {
                if cc_depth > 0 {
                    i += 1;
                    continue;
                }
                i += 1;
                if i < len && bytes[i] == b'?' {
                    i += 1;
                    if i >= len {
                        continue;
                    }
                    match bytes[i] {
                        // Non-capturing groups and special groups:
                        // (?:...) (?=...) (?!...) (?>...) (?~...)
                        b':' | b'=' | b'!' | b'>' | b'~' => {
                            i += 1;
                            continue;
                        }
                        // Flag groups: (?flags:...), (?flags-flags:...), (?-flags:...)
                        b'-' | b'i' | b'm' | b'x' | b'd' | b'a' | b'u' => {
                            i += 1;
                            continue;
                        }
                        // Named capture or lookbehind: (?<name>...), (?'name'...),
                        // (?<=...) (?<!...)
                        b'<' | b'\'' => {
                            if bytes[i] == b'<' {
                                // Peek ahead: if next byte is '=' or '!', it's a
                                // lookbehind, not a named capture.
                                if i + 1 < len
                                    && (bytes[i + 1] == b'=' || bytes[i + 1] == b'!')
                                {
                                    i += 2;
                                    continue;
                                }
                            }
                            has_named = true;
                            i += 1;
                            continue;
                        }
                        // Other (?...) — treat as non-capturing.
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                }
                // Plain capturing group: (...)
                has_numbered = true;
            }
            _ => {
                i += 1;
            }
        }
    }

    (has_named, has_numbered)
}

murphy_plugin_api::submit_cop!(MixedRegexpCaptureTypes);

#[cfg(test)]
mod tests {
    use super::MixedRegexpCaptureTypes;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_when_both_named_and_numbered_captures_are_used() {
        test::<MixedRegexpCaptureTypes>().expect_offense(indoc! {r#"
            /(?<foo>bar)(baz)/
            ^^^^^^^^^^^^^^^^^^ Do not mix named captures and numbered captures in a Regexp literal.
        "#});
    }

    #[test]
    fn does_not_flag_named_capture_only() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<foo>foo?<bar>bar)/
        "#});
    }

    #[test]
    fn does_not_flag_lookbehind_with_numbered_capture() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<=>)(<br>)(?=><)/
        "#});
    }

    #[test]
    fn does_not_flag_numbered_capture_only() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(foo)(bar)/
        "#});
    }

    #[test]
    fn does_not_flag_named_capture_and_non_capturing_group() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<foo>bar)(?:bar)/
        "#});
    }

    #[test]
    fn does_not_flag_named_capture_with_lookahead() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<foo>bar)(?=baz)/
        "#});
    }

    #[test]
    fn does_not_flag_named_capture_with_atomic_group() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<foo>bar)(?>baz)/
        "#});
    }

    #[test]
    fn does_not_flag_interpolated_regexp() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            var = '(\d+)'
            /(?<foo>#{var}*)/
        "#});
    }

    #[test]
    fn does_not_flag_lookbehind_with_named_capture() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?<=-)(?<name>\w+)/
        "#});
    }

    #[test]
    fn flags_numbered_before_named() {
        test::<MixedRegexpCaptureTypes>().expect_offense(indoc! {r#"
            /(foo)(?<bar>baz)/
            ^^^^^^^^^^^^^^^^^^ Do not mix named captures and numbered captures in a Regexp literal.
        "#});
    }

    #[test]
    fn does_not_flag_alternate_named_syntax_with_numbered_only() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(foo)(bar)/
        "#});
    }

    #[test]
    fn flags_alternate_named_syntax_with_numbered() {
        test::<MixedRegexpCaptureTypes>().expect_offense(indoc! {r#"
            /(foo)(?'bar'baz)/
            ^^^^^^^^^^^^^^^^^^ Do not mix named captures and numbered captures in a Regexp literal.
        "#});
    }

    #[test]
    fn does_not_flag_alternate_named_syntax_only() {
        test::<MixedRegexpCaptureTypes>().expect_no_offenses(indoc! {r#"
            /(?'foo'bar)(?'baz'qux)/
        "#});
    }
}
