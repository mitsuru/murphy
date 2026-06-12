//! `Layout/LeadingCommentSpace` — comments should start with a space after the
//! leading `#`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LeadingCommentSpace
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation`: for each comment whose
//!   text matches `/\A(?!#\+\+|#--)(#+[^#\s=])/` (i.e. is not an RDoc `#++`/`#--`
//!   directive and has a non-`#`, non-whitespace, non-`=` char immediately after
//!   its run of `#`), insert a single space after the FIRST `#` (`hash_mark` =
//!   `[begin, begin + 1)`). Skip conditions are ported verbatim: line-1
//!   shebang / rackup `config.ru` `#\` options, multi-line shebang continuation,
//!   and the four `Allow*` style options (Doxygen `#*`, Gemfile `#ruby`, RBS
//!   inline `#:`/`#[...]`/`#|`, Steep `#$`/`#:`), all of which default to
//!   `false`. The offense range is the comment's source range (Murphy's
//!   `cx.comments()` ranges exclude the trailing newline, matching RuboCop's
//!   `comment.source_range`). File-path-dependent branches (rackup, Gemfile) use
//!   `cx.file_path()`.
//! ```

use murphy_plugin_api::{Comment, CopOptions, Cx, Range, cop};

/// Carries the comment ranges already flagged in this investigation — RuboCop's
/// `current_offense_locations`. Used by `shebang_continuation` so a `#!` line
/// whose predecessor `#!` line was itself flagged (not a real shebang) is also
/// flagged, matching upstream.
type FlaggedRanges = Vec<Range>;

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct LeadingCommentSpace;

#[derive(CopOptions)]
pub struct LeadingCommentSpaceOptions {
    #[option(
        name = "AllowDoxygenCommentStyle",
        default = false,
        description = "Allow Doxygen-style comments starting with `#*`."
    )]
    pub allow_doxygen_comment_style: bool,

    #[option(
        name = "AllowGemfileRubyComment",
        default = false,
        description = "Allow `#ruby=...` version comments in a Gemfile."
    )]
    pub allow_gemfile_ruby_comment: bool,

    #[option(
        name = "AllowRBSInlineAnnotation",
        default = false,
        description = "Allow RBS::Inline annotation comments (`#:`, `#[...]`, `#|`)."
    )]
    pub allow_rbs_inline_annotation: bool,

    #[option(
        name = "AllowSteepAnnotation",
        default = false,
        description = "Allow Steep type annotation comments (`#$`, `#:`)."
    )]
    pub allow_steep_annotation: bool,
}

#[cop(
    name = "Layout/LeadingCommentSpace",
    description = "Comments should start with a space.",
    default_severity = "warning",
    default_enabled = true,
    options = LeadingCommentSpaceOptions
)]
impl LeadingCommentSpace {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<LeadingCommentSpaceOptions>();
        let comments = cx.comments();
        // RuboCop's `current_offense_locations`, accumulated as we go.
        let mut flagged: FlaggedRanges = Vec::new();

        for comment in comments {
            let text = cx.raw_source(comment.range);

            // RuboCop: `next unless /\A(?!#\+\+|#--)(#+[^#\s=])/.match?(text)`.
            if !needs_space(text) {
                continue;
            }

            let line = line_of(cx, comment.range.start);

            // `next if comment.loc.line == 1 && allowed_on_first_line?(comment)`.
            if line == 1 && allowed_on_first_line(cx, text) {
                continue;
            }
            // `next if shebang_continuation?(comment)`.
            if shebang_continuation(cx, comments, *comment, text, line, &flagged) {
                continue;
            }
            // `next if doxygen_comment_style?(comment)`.
            if opts.allow_doxygen_comment_style && text.starts_with("#*") {
                continue;
            }
            // `next if gemfile_ruby_comment?(comment)`.
            if opts.allow_gemfile_ruby_comment && gemfile(cx) && text.starts_with("#ruby") {
                continue;
            }
            // `next if rbs_inline_annotation?(comment)`.
            if opts.allow_rbs_inline_annotation && rbs_inline_annotation(text) {
                continue;
            }
            // `next if steep_annotation?(comment)`.
            if opts.allow_steep_annotation && steep_annotation(text) {
                continue;
            }

            // `add_offense(comment)` then
            // `corrector.insert_after(hash_mark(expr), ' ')` where `hash_mark`
            // is `[begin, begin + 1)` — i.e. insert after the first `#`.
            cx.emit_offense(comment.range, "Missing space after `#`.", None);
            flagged.push(comment.range);
            let insert_at = comment.range.start + 1;
            cx.emit_edit(
                Range {
                    start: insert_at,
                    end: insert_at,
                },
                " ",
            );
        }
    }
}

/// RuboCop's `/\A(?!#\+\+|#--)(#+[^#\s=])/` match test.
///
/// True iff `text` is a comment (`#`-prefixed), is NOT an RDoc `#++` / `#--`
/// directive, and the first character after its run of `#` exists and is not
/// `#`, ASCII/Unicode whitespace, or `=`.
fn needs_space(text: &str) -> bool {
    // `(?!#\+\+|#--)` negative lookahead.
    if text.starts_with("#++") || text.starts_with("#--") {
        return false;
    }
    // `#+` — must start with at least one `#`.
    let after_hashes = text.trim_start_matches('#');
    if after_hashes.len() == text.len() {
        // No leading `#` at all (e.g. a block `=begin` comment) — never matches.
        return false;
    }
    // `[^#\s=]` — the char after the run of `#` must exist and not be
    // whitespace or `=`. Ruby's `\s` is ASCII (`[ \t\r\n\f\v]`), so use
    // `is_ascii_whitespace` rather than the Unicode `char::is_whitespace`.
    // (`trim_start_matches('#')` already guarantees the next char, if any,
    // is not `#`.)
    match after_hashes.as_bytes().first() {
        Some(&b) => !b.is_ascii_whitespace() && b != b'=',
        None => false,
    }
}

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

/// RuboCop `shebang?` — `comment.text.start_with?('#!')`.
fn shebang(text: &str) -> bool {
    text.starts_with("#!")
}

/// RuboCop `allowed_on_first_line?` —
/// `shebang?(comment) || (rackup_config_file? && rackup_options?(comment))`.
fn allowed_on_first_line(cx: &Cx<'_>, text: &str) -> bool {
    shebang(text) || (rackup_config_file(cx) && text.starts_with("#\\"))
}

/// RuboCop `rackup_config_file?` —
/// `File.basename(processed_source.file_path).eql?('config.ru')`.
fn rackup_config_file(cx: &Cx<'_>) -> bool {
    basename(cx.file_path()) == "config.ru"
}

/// RuboCop `gemfile?` — `File.basename(processed_source.file_path).eql?('Gemfile')`.
fn gemfile(cx: &Cx<'_>) -> bool {
    basename(cx.file_path()) == "Gemfile"
}

/// `File.basename` — the path component after the final `/`.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// RuboCop `rbs_inline_annotation?` —
/// `comment.text.start_with?(/#:|#\[.+\]|#\|/)`.
fn rbs_inline_annotation(text: &str) -> bool {
    if text.starts_with("#:") || text.starts_with("#|") {
        return true;
    }
    // `#\[.+\]` — `#[`, at least one char, then a `]` somewhere after it.
    if let Some(rest) = text.strip_prefix("#[") {
        // `.+\]` requires at least one char before the closing `]`.
        if let Some(close) = rest.find(']') {
            return close >= 1;
        }
    }
    false
}

/// RuboCop `steep_annotation?` — `comment.text.start_with?(/#[$:]/)`.
fn steep_annotation(text: &str) -> bool {
    text.starts_with("#$") || text.starts_with("#:")
}

/// RuboCop `shebang_continuation?`.
///
/// ```text
/// return false unless shebang?(comment)
/// return true if comment.loc.line == 1
/// previous_line_comment = processed_source.comment_at_line(comment.loc.line - 1)
/// return false unless previous_line_comment
/// shebang?(previous_line_comment) &&
///   !current_offense_locations.include?(previous_line_comment.source_range)
/// ```
///
/// `flagged` is RuboCop's `current_offense_locations`: when the previous-line
/// `#!` comment was itself already flagged (i.e. it was not a genuine shebang),
/// this comment is NOT treated as a continuation and is flagged in turn.
fn shebang_continuation(
    cx: &Cx<'_>,
    comments: &[Comment],
    comment: Comment,
    text: &str,
    line: usize,
    flagged: &[Range],
) -> bool {
    if !shebang(text) {
        return false;
    }
    if line == 1 {
        return true;
    }
    // `comment_at_line(line - 1)` — the comment whose start is on `line - 1`.
    let prev_line = line - 1;
    let Some(prev) = comments
        .iter()
        .copied()
        .find(|c| c.range != comment.range && line_of(cx, c.range.start) == prev_line)
    else {
        return false;
    };
    // `shebang?(prev) && !current_offense_locations.include?(prev.source_range)`.
    shebang(cx.raw_source(prev.range)) && !flagged.contains(&prev.range)
}

murphy_plugin_api::submit_cop!(LeadingCommentSpace);

#[cfg(test)]
mod tests {
    use super::{LeadingCommentSpace, LeadingCommentSpaceOptions};
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, run_cop_with_options};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut out = String::with_capacity(source.len() + edits.len());
        let mut last = 0usize;
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        for e in ordered {
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }

    fn allow_all() -> LeadingCommentSpaceOptions {
        LeadingCommentSpaceOptions {
            allow_doxygen_comment_style: true,
            allow_gemfile_ruby_comment: true,
            allow_rbs_inline_annotation: true,
            allow_steep_annotation: true,
        }
    }

    #[test]
    fn accepts_comment_with_space() {
        assert!(run_cop::<LeadingCommentSpace>("# good comment\n").is_empty());
    }

    #[test]
    fn flags_and_corrects_missing_space() {
        let run = run_cop_with_edits::<LeadingCommentSpace>("#bad comment\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Missing space after `#`.");
        assert_eq!(apply("#bad comment\n", &run.edits), "# bad comment\n");
    }

    #[test]
    fn flags_inline_comment_missing_space() {
        let run = run_cop_with_edits::<LeadingCommentSpace>("x = 1 #inline\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("x = 1 #inline\n", &run.edits), "x = 1 # inline\n");
    }

    #[test]
    fn corrects_double_hash_inserting_after_first_hash_only() {
        // `hash_mark` is `[begin, begin + 1)`, so the space lands after the
        // first `#`: `##foo` -> `# #foo`.
        let run = run_cop_with_edits::<LeadingCommentSpace>("##foo\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("##foo\n", &run.edits), "# #foo\n");
    }

    #[test]
    fn accepts_run_of_hashes_only() {
        // `#+[^#\s=]` needs a non-`#`/space/`=` char after the run of `#`.
        assert!(run_cop::<LeadingCommentSpace>("###\n").is_empty());
    }

    #[test]
    fn accepts_rdoc_plus_directive() {
        assert!(run_cop::<LeadingCommentSpace>("#++\n").is_empty());
    }

    #[test]
    fn accepts_rdoc_minus_directive() {
        assert!(run_cop::<LeadingCommentSpace>("#--\n").is_empty());
    }

    #[test]
    fn accepts_equals_after_hash() {
        // `[^#\s=]` excludes `=`, so `#=foo` is not flagged.
        assert!(run_cop::<LeadingCommentSpace>("#=foo\n").is_empty());
    }

    #[test]
    fn accepts_shebang_on_first_line() {
        assert!(run_cop::<LeadingCommentSpace>("#!/usr/bin/env ruby\nx = 1\n").is_empty());
    }

    #[test]
    fn flags_shebang_not_on_first_line() {
        // A `#!` comment that is not a line-1 shebang and not a continuation of
        // a previous shebang line is flagged like any other `#x` comment.
        let run = run_cop::<LeadingCommentSpace>("x = 1\n#!notashebang\n");
        assert_eq!(run.len(), 1);
    }

    #[test]
    fn accepts_multiline_shebang_continuation() {
        // Two consecutive `#!` lines: line 1 is the shebang, line 2 continues
        // it and is exempt.
        assert!(run_cop::<LeadingCommentSpace>("#!/usr/bin/env ruby\n#!extra\nx = 1\n").is_empty());
    }

    #[test]
    fn flags_both_bang_lines_when_first_is_not_a_real_shebang() {
        // Neither `#!foo` (line 2) nor `#!bar` (line 3) is a line-1 shebang.
        // `#!foo` is flagged; because it lands in `current_offense_locations`,
        // `#!bar` is NOT treated as a continuation and is flagged too. RuboCop
        // emits 2 here.
        let run = run_cop::<LeadingCommentSpace>("x = 1\n#!foo\n#!bar\n");
        assert_eq!(run.len(), 2, "both bang lines must be flagged: {run:?}");
    }

    #[test]
    fn doxygen_flagged_by_default() {
        let run = run_cop::<LeadingCommentSpace>("#*foo\n");
        assert_eq!(run.len(), 1);
    }

    #[test]
    fn doxygen_allowed_with_option() {
        assert!(run_cop_with_options::<LeadingCommentSpace>("#*foo\n", &allow_all()).is_empty());
    }

    #[test]
    fn rbs_inline_colon_allowed_with_option() {
        assert!(run_cop_with_options::<LeadingCommentSpace>("#: String\n", &allow_all()).is_empty());
    }

    #[test]
    fn rbs_inline_bracket_allowed_with_option() {
        assert!(
            run_cop_with_options::<LeadingCommentSpace>("#[Integer]\n", &allow_all()).is_empty()
        );
    }

    #[test]
    fn rbs_inline_pipe_allowed_with_option() {
        assert!(run_cop_with_options::<LeadingCommentSpace>("#| String\n", &allow_all()).is_empty());
    }

    #[test]
    fn steep_dollar_allowed_with_option() {
        assert!(run_cop_with_options::<LeadingCommentSpace>("#$foo\n", &allow_all()).is_empty());
    }

    #[test]
    fn rbs_colon_flagged_without_option() {
        // `#:` matches `#+[^#\s=]` (`:` is not whitespace/`=`), so it is flagged
        // when the RBS/Steep options are off.
        let run = run_cop::<LeadingCommentSpace>("#: String\n");
        assert_eq!(run.len(), 1);
    }

    #[test]
    fn flags_multiple_comments() {
        let run = run_cop::<LeadingCommentSpace>("#one\n#two\n");
        assert_eq!(run.len(), 2);
    }
}
