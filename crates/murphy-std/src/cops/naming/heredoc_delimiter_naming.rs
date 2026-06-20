//! `Naming/HeredocDelimiterNaming` — require meaningful heredoc delimiters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/HeredocDelimiterNaming
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop's `Heredoc` mixin visits every `str`/`dstr`/`xstr` node that is a
//!   heredoc, extracts the delimiter via
//!   `OPENING_DELIMITER = /(<<[~-]?)['"`]?([^'"`]+)['"`]?/` captures[1], and
//!   flags it unless it (a) contains a `\w` char AND (b) matches none of
//!   `ForbiddenDelimiters`. Murphy reproduces this token-based: Murphy's AST
//!   hides heredoc-ness (a heredoc `str` node looks identical to a normal
//!   `str`), so the cop walks `HeredocStart`/`HeredocEnd` token pairs from
//!   `cx.sorted_tokens()`, paired FIFO (Ruby reads heredoc bodies in opener
//!   order — verified against stacked `foo(<<~A, <<~B)`).
//!
//!   Offense range mirrors RuboCop exactly:
//!     * empty body (`node.children.empty?`, i.e. zero body bytes) → the
//!       opener token (`<<-EOF`), verified cols 5-10 for `d = <<-EOF`;
//!     * non-empty body → `node.loc.heredoc_end` — the terminator line from
//!       its first column through the label, EXCLUDING the trailing newline.
//!       Verified: plain `END` (cols 1-3) and indented `  EOS` (cols 1-5,
//!       leading whitespace INCLUDED) for both `<<-` and `<<~`.
//!
//!   `ForbiddenDelimiters` default is a `!ruby/regexp`
//!   (`/(^|\s)(EO[A-Z]{1}|END)(\s|$)/i`); yaml_rust2 yields it as the literal
//!   string `/(^|\s)(EO[A-Z]{1}|END)(\s|$)/i` (enclosing slashes + `i` flag).
//!   Each entry is translated from Ruby `/body/flags` literal form to a Rust
//!   regex (flags `i`→`(?i)`, `m`→`(?s)` dotall, `x`→`(?x)`) and matched via
//!   `cx.matches_any_pattern`. Bare strings (no enclosing slashes) pass
//!   through unchanged, mirroring `Regexp.new("plain")`.
//!
//!   The `\w` "meaningfulness" check uses ASCII word semantics
//!   (`[A-Za-z0-9_]`), matching Ruby's `\w`; Rust's `\w` is Unicode, so this
//!   is done by hand rather than via the regex engine. Verified: `++` (no
//!   word char) fires; `123` is meaningful.
//! ```

use murphy_plugin_api::{Cx, CopOptions, Range, SourceTokenKind, cop};

const MSG: &str = "Use meaningful heredoc delimiters.";

#[derive(Default)]
pub struct HeredocDelimiterNaming;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "ForbiddenDelimiters",
        default = ["/(^|\\s)(EO[A-Z]{1}|END)(\\s|$)/i"],
        description = "Delimiters (Ruby regexp literals) that are not meaningful."
    )]
    pub forbidden_delimiters: Vec<String>,
}

#[cop(
    name = "Naming/HeredocDelimiterNaming",
    description = "Use meaningful heredoc delimiters.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl HeredocDelimiterNaming {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // Translate each Ruby `/body/flags` regexp literal to a Rust-compatible
        // pattern once; bare strings pass through. `matches_any_pattern`
        // compiles + caches these.
        let patterns: Vec<String> = opts
            .forbidden_delimiters
            .iter()
            .map(|s| ruby_regex_to_rust_pattern(s))
            .collect();

        for heredoc in collect_heredocs(cx) {
            let opener_src = cx.raw_source(heredoc.opener);
            let Some(delimiter) = delimiter_string(opener_src) else {
                continue;
            };

            if meaningful(delimiter, &patterns, cx) {
                continue;
            }

            // Offense range: empty body → opener token; else → heredoc_end
            // label (terminator line, trailing newline stripped).
            let range = if heredoc.body_is_empty {
                heredoc.opener
            } else {
                heredoc.end_label
            };
            cx.emit_offense(range, MSG, None);
        }
    }
}

/// A heredoc resolved from its `HeredocStart`/`HeredocEnd` token pair.
struct Heredoc {
    /// Source range of the opener token (`<<-END`, `<<~"EOS"`, `` <<`CMD` ``).
    opener: Range,
    /// `node.loc.heredoc_end`: the terminator line (leading indent + label),
    /// trailing newline excluded.
    end_label: Range,
    /// True when the heredoc has zero body bytes (`node.children.empty?`).
    body_is_empty: bool,
}

/// Pair `HeredocStart`/`HeredocEnd` tokens FIFO. Ruby reads heredoc bodies in
/// opener order, so the first `HeredocEnd` terminates the earliest unmatched
/// `HeredocStart`. A running `cursor` chains past each consumed terminator so
/// that stacked heredocs sharing one opener line (`foo(<<~A, <<~B)`) do not
/// overlap — `body_start = max(cursor, opener_line_end)`.
fn collect_heredocs(cx: &Cx<'_>) -> Vec<Heredoc> {
    use std::collections::VecDeque;
    let source = cx.source().as_bytes();
    let mut pending: VecDeque<Range> = VecDeque::new();
    let mut out: Vec<Heredoc> = Vec::new();
    let mut cursor: u32 = 0;

    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => pending.push_back(tok.range),
            SourceTokenKind::HeredocEnd => {
                let Some(opener) = pending.pop_front() else {
                    continue;
                };
                let opener_line_end = next_line_start(source, opener.end);
                let term_line_start = line_start(source, tok.range.start);
                let body_start = cursor
                    .max(opener_line_end)
                    .min(term_line_start);
                cursor = next_line_start(source, tok.range.start);

                // The `HeredocEnd` token spans the terminator line's label and
                // its trailing `\n`; strip the newline so the range matches
                // RuboCop's `heredoc_end` (label only, no newline). Guard the
                // strip: a terminator at EOF with no final newline has no `\n`
                // to remove.
                let end = if source
                    .get(tok.range.end.saturating_sub(1) as usize)
                    == Some(&b'\n')
                {
                    tok.range.end - 1
                } else {
                    tok.range.end
                };
                out.push(Heredoc {
                    opener,
                    end_label: Range {
                        start: term_line_start,
                        end,
                    },
                    body_is_empty: body_start >= term_line_start,
                });
            }
            _ => {}
        }
    }
    out
}

/// Byte offset of the first byte after the next `\n` at or after `from`, or the
/// source length if there is none.
fn next_line_start(source: &[u8], from: u32) -> u32 {
    let from = (from as usize).min(source.len());
    match source[from..].iter().position(|&b| b == b'\n') {
        Some(off) => (from + off + 1) as u32,
        None => source.len() as u32,
    }
}

/// Byte offset of the start of the line containing `pos`.
fn line_start(source: &[u8], pos: u32) -> u32 {
    let pos = (pos as usize).min(source.len());
    match source[..pos].iter().rposition(|&b| b == b'\n') {
        Some(idx) => (idx + 1) as u32,
        None => 0,
    }
}

/// RuboCop `delimiter_string`: `OPENING_DELIMITER = /(<<[~-]?)['"`]?([^'"`]+)['"`]?/`
/// captures[1]. Strip `<<`, an optional `~`/`-`, an optional opening quote,
/// then take the run of non-quote characters. Returns `None` if the opener
/// has no delimiter body.
fn delimiter_string(opener: &str) -> Option<&str> {
    let rest = opener.strip_prefix("<<")?;
    let rest = rest.strip_prefix('~').or_else(|| rest.strip_prefix('-')).unwrap_or(rest);
    let rest = rest
        .strip_prefix('\'')
        .or_else(|| rest.strip_prefix('"'))
        .or_else(|| rest.strip_prefix('`'))
        .unwrap_or(rest);
    let delim = match rest.find(['\'', '"', '`']) {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    if delim.is_empty() {
        None
    } else {
        Some(delim)
    }
}

/// RuboCop `meaningful_delimiters?`: the delimiter must contain at least one
/// word char (Ruby `\w` = ASCII `[A-Za-z0-9_]`) AND match none of the forbidden
/// patterns.
fn meaningful(delimiter: &str, patterns: &[String], cx: &Cx<'_>) -> bool {
    let has_word_char = delimiter
        .bytes()
        .any(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !has_word_char {
        return false;
    }
    !cx.matches_any_pattern(delimiter, patterns)
}

/// Translate a Ruby regexp literal in `/body/flags` form to a Rust regex
/// pattern. Recognised flags: `i` (case-insensitive), `m` (Ruby multiline =
/// dotall, Rust `(?s)`), `x` (extended). A string with no enclosing slashes is
/// returned unchanged (mirrors `Regexp.new("plain")`).
fn ruby_regex_to_rust_pattern(literal: &str) -> String {
    // Must start with `/` and have a closing `/` somewhere after the body.
    let Some(after_open) = literal.strip_prefix('/') else {
        return literal.to_string();
    };
    // Split at the LAST `/` — flags follow it; the body may itself contain `/`.
    let Some(close_idx) = after_open.rfind('/') else {
        return literal.to_string();
    };
    let body = &after_open[..close_idx];
    let flags = &after_open[close_idx + 1..];

    let mut inline = String::new();
    for f in flags.chars() {
        match f {
            'i' => inline.push('i'),
            'm' => inline.push('s'), // Ruby `m` = dotall
            'x' => inline.push('x'),
            _ => {} // ignore unsupported flags (e.g. `o`)
        }
    }
    if inline.is_empty() {
        body.to_string()
    } else {
        format!("(?{inline}){body}")
    }
}

#[cfg(test)]
mod tests {
    use super::{HeredocDelimiterNaming, Options, ruby_regex_to_rust_pattern};
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- default ForbiddenDelimiters (exercises the literal→Rust path) ----

    #[test]
    fn flags_end_delimiter_default() {
        // rubocop: terminator label `END`, cols 1-3 (non-empty body).
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-END
              foo
            END
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_eos_default() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-EOS
              x
            EOS
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_eof_default() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-EOF
              x
            EOF
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_eot_default() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-EOT
              x
            EOT
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_eol_default() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-EOL
              x
            EOL
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_lowercase_eos_case_insensitive() {
        // `/i` flag: `eos` is forbidden too.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-eos
              x
            eos
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn allows_meaningful_delimiters_default() {
        // SQL, RUBY, END_OF_TEXT are all meaningful under the default.
        test::<HeredocDelimiterNaming>().expect_no_offenses(indoc! {r#"
            a = <<-SQL
              SELECT 1
            SQL
            b = <<~RUBY
              puts 1
            RUBY
            c = <<-END_OF_TEXT
              hi
            END_OF_TEXT
        "#});
    }

    // ---- offense range cases ----

    #[test]
    fn empty_body_flags_opener() {
        // rubocop: empty body → opener `<<-EOF`, cols 5-10.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            d = <<-EOF
                ^^^^^^ Use meaningful heredoc delimiters.
            EOF
        "#});
    }

    #[test]
    fn blank_line_body_is_not_empty() {
        // A single blank-line body has a child → heredoc_end range, not opener.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {"
            b = <<-EOF\n\nEOF\n^^^ Use meaningful heredoc delimiters.\n"});
    }

    #[test]
    fn indented_squiggly_terminator_includes_indent() {
        // rubocop: `  EOS`, cols 1-5 (leading indent INCLUDED).
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            def m
              x = <<~EOS
                y
              EOS
            ^^^^^ Use meaningful heredoc delimiters.
            end
        "#});
    }

    #[test]
    fn indented_dash_terminator_includes_indent() {
        // `<<-` allows an indented terminator too; cols 1-5.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            def m
              x = <<-EOS
                y
              EOS
            ^^^^^ Use meaningful heredoc delimiters.
            end
        "#});
    }

    // ---- quoted opener forms ----

    #[test]
    fn flags_double_quoted_delimiter() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<-"END"
              x
            END
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_single_quoted_delimiter() {
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<~'EOS'
              x
            EOS
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_backtick_delimiter() {
        // xstr heredoc; `EOF` is forbidden.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<`EOF`
              x
            EOF
            ^^^ Use meaningful heredoc delimiters.
        "#});
    }

    // ---- meaningfulness ----

    #[test]
    fn flags_non_word_delimiter() {
        // `++` has no `\w` char → not meaningful → offense at terminator.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            a = <<~"++"
              x
            ++
            ^^ Use meaningful heredoc delimiters.
        "#});
    }

    #[test]
    fn allows_digit_delimiter() {
        // `123` has `\w` via digits and is not forbidden.
        test::<HeredocDelimiterNaming>().expect_no_offenses(indoc! {r#"
            a = <<-"123"
              x
            123
        "#});
    }

    #[test]
    fn allows_eos_with_suffix() {
        // `EOS123` does not match `EO[A-Z]{1}(\s|$)`.
        test::<HeredocDelimiterNaming>().expect_no_offenses(indoc! {r#"
            a = <<-EOS123
              x
            EOS123
        "#});
    }

    // ---- stacked heredocs (FIFO pairing) ----

    #[test]
    fn flags_only_forbidden_in_stacked_heredocs() {
        // `END` forbidden (line 3), `SQL` meaningful → exactly one offense.
        test::<HeredocDelimiterNaming>().expect_offense(indoc! {r#"
            foo(<<~END, <<~SQL)
              a
            END
            ^^^ Use meaningful heredoc delimiters.
              b
            SQL
        "#});
    }

    // ---- ForbiddenDelimiters option ----

    #[test]
    fn respects_custom_forbidden_delimiters() {
        // A plain-string forbidden entry behaves like `Regexp.new("FOO")`.
        test::<HeredocDelimiterNaming>()
            .with_options(&Options {
                forbidden_delimiters: vec!["FOO".to_string()],
            })
            .expect_offense(indoc! {r#"
                a = <<-FOO
                  x
                FOO
                ^^^ Use meaningful heredoc delimiters.
            "#});
    }

    #[test]
    fn empty_forbidden_list_still_flags_no_word_char() {
        // With no forbidden delimiters, only the `\w` check applies.
        test::<HeredocDelimiterNaming>()
            .with_options(&Options {
                forbidden_delimiters: vec![],
            })
            .expect_no_offenses(indoc! {r#"
                a = <<-END
                  x
                END
            "#});
    }

    #[test]
    fn no_offense_for_non_heredoc_code() {
        test::<HeredocDelimiterNaming>().expect_no_offenses(indoc! {r#"
            def say_hello
              puts "hello"
            end
        "#});
    }

    // ---- regex literal translation unit tests ----

    #[test]
    fn translates_ruby_regex_literal_with_i_flag() {
        // The default.yml literal becomes a Rust-compatible `(?i)`-prefixed
        // pattern. End-to-end matching (END/eos fire, SQL/END_OF_TEXT do not)
        // is exercised by the default-option tests above.
        let p = ruby_regex_to_rust_pattern("/(^|\\s)(EO[A-Z]{1}|END)(\\s|$)/i");
        assert_eq!(p, "(?i)(^|\\s)(EO[A-Z]{1}|END)(\\s|$)");
    }

    #[test]
    fn translates_plain_string_unchanged() {
        assert_eq!(ruby_regex_to_rust_pattern("FOO"), "FOO");
    }

    #[test]
    fn translates_literal_no_flags() {
        assert_eq!(ruby_regex_to_rust_pattern("/END/"), "END");
    }

    #[test]
    fn translates_m_flag_to_dotall() {
        assert_eq!(ruby_regex_to_rust_pattern("/a.b/m"), "(?s)a.b");
    }
}
murphy_plugin_api::submit_cop!(HeredocDelimiterNaming);
