//! `Lint/CopDirectiveSyntax` — validate the strict formatting of
//! `# rubocop:enable`/`disable`/`todo`/`push`/`pop` directive comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/CopDirectiveSyntax
//! upstream_version_checked: 1.86.2
//! version_added: "1.72"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Hand-rolled port of RuboCop's `DirectiveComment` regex stack (no `regex`
//!   dependency in murphy-std). Covers the four offense messages (missing mode,
//!   invalid mode, missing cop name, malformed cop names) plus the no-offense
//!   cases (bare department, `all`, valid trailing `-- comment`, tab-separated
//!   mode, double-comment and quoted non-directive). Mode extraction follows
//!   Ruby's `split(' ')` whitespace-run semantics; the trailing-comment check
//!   reproduces RuboCop's `post_match.lstrip.start_with?('--')` (so `Foo--bad`
//!   is accepted). Only `rubocop:` directives are validated, mirroring RuboCop
//!   exactly — Murphy's own `murphy:` directives are deliberately out of scope
//!   (RuboCop's cop is `rubocop:`-only), not a tracked gap.
//! ```
//!
//! ## Matched shapes
//!
//! Every comment that starts with the `# rubocop:` marker (whitespace-tolerant)
//! and is malformed per RuboCop's `DirectiveComment#malformed?`.
//!
//! ## No autocorrect
//!
//! RuboCop ships no autocorrect — the intended fix (comma-separated cops,
//! `-- comment` prefix, a valid mode) cannot be inferred unambiguously.

use murphy_plugin_api::{Cx, NoOptions, cop};

const COMMON_MSG: &str = "Malformed directive comment detected.";
const MISSING_MODE_NAME_MSG: &str = "The mode name is missing.";
const INVALID_MODE_NAME_MSG: &str =
    "The mode name must be one of `enable`, `disable`, `todo`, `push`, or `pop`.";
const MISSING_COP_NAME_MSG: &str = "The cop name is missing.";
const MALFORMED_COP_NAMES_MSG: &str =
    "Cop names must be separated by commas. Comment in the directive must start with `--`.";

const AVAILABLE_MODES: &[&str] = &["disable", "enable", "todo", "push", "pop"];
const TRAILING_COMMENT_MARKER: &str = "--";

#[derive(Default)]
pub struct CopDirectiveSyntax;

#[cop(
    name = "Lint/CopDirectiveSyntax",
    description = "Validate the syntax of rubocop directive comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl CopDirectiveSyntax {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        for comment in cx.comments() {
            let text = cx.raw_source(comment.range);
            // `start_with_marker?` — strict anchored `#\s*rubocop\s*:\s*` prefix.
            let Some(after_marker) = strip_directive_marker(text) else {
                continue;
            };
            let Some(message) = offense_message(after_marker) else {
                continue;
            };
            cx.emit_offense(comment.range, &message, None);
        }
    }
}

/// `DirectiveComment::DIRECTIVE_MARKER_REGEXP` (`#\s*rubocop\s*:\s*`) applied
/// as an anchored prefix. Returns the remainder after the marker, or `None`
/// when `text` does not start with the marker.
fn strip_directive_marker(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('#')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("rubocop")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    Some(rest.trim_start())
}

/// Returns the offense message when the directive is malformed, else `None`.
///
/// `after_marker` is the comment text with the `# rubocop:` marker removed,
/// matching RuboCop's `comment.text.sub(DIRECTIVE_MARKER_REGEXP, '')` — note
/// `strip_directive_marker` already consumed the trailing whitespace, so a
/// leading-space `split` would lose the first token; we instead inspect the
/// already-trimmed remainder directly.
fn offense_message(after_marker: &str) -> Option<String> {
    // `mode = after_marker.split(' ', 2).first`. Ruby's `split(' ')` treats any
    // whitespace run (spaces, tabs) as the delimiter and skips leading blanks,
    // so the mode is the first whitespace-delimited token. Empty remainder →
    // missing mode.
    let mode = after_marker.split_whitespace().next().unwrap_or("");
    if mode.is_empty() {
        return Some(format!("{COMMON_MSG} {MISSING_MODE_NAME_MSG}"));
    }
    if !AVAILABLE_MODES.contains(&mode) {
        return Some(format!("{COMMON_MSG} {INVALID_MODE_NAME_MSG}"));
    }

    // The argument portion after the mode token. `strip_directive_marker`
    // already trimmed leading whitespace, so `after_marker` begins with `mode`;
    // strip it and the following whitespace to reach the args.
    let args = after_marker[mode.len()..].trim_start();

    // push/pop never require a cop name and use `+Cop -Cop` args, not the
    // comma-separated cop list. RuboCop's `missing_cop_name?` is false for them
    // and they only fail the trailing-junk check.
    let is_push_pop = mode == "push" || mode == "pop";

    if !is_push_pop && missing_cop_name(args) {
        return Some(format!("{COMMON_MSG} {MISSING_COP_NAME_MSG}"));
    }

    // Well-formed: `(all | cop, cop, ...)` (or push/pop `+a -b` args) optionally
    // followed by ` -- comment`. Anything else is malformed cop-name syntax.
    if well_formed_args(args, is_push_pop) {
        return None;
    }
    Some(format!("{COMMON_MSG} {MALFORMED_COP_NAMES_MSG}"))
}

/// RuboCop's `missing_cop_name?` for non-push/pop modes: the args are empty
/// (a bare `# rubocop:disable`).
fn missing_cop_name(args: &str) -> bool {
    args.is_empty()
}

/// Mirror RuboCop's `malformed?` tail check: greedily match the cop-list
/// (`all`, comma-separated cop names, or push/pop `+a -b` args) from the start
/// of `args`, then require the remainder — after lstrip — to be empty or to
/// start with the `--` trailing-comment marker. RuboCop checks
/// `post_match.lstrip.start_with?('--')`, so `Foo--bad` (no space before `--`)
/// is intentionally accepted: the cop name stops at the non-word `-`, leaving
/// `--bad` as the post-match comment.
fn well_formed_args(args: &str, is_push_pop: bool) -> bool {
    let rest = if is_push_pop {
        match_push_pop_args(args)
    } else if let Some(rest) = strip_prefix_word(args, "all") {
        Some(rest)
    } else {
        match_cop_list(args)
    };

    let Some(rest) = rest else {
        return false;
    };
    let tail = rest.trim_start();
    tail.is_empty() || tail.starts_with(TRAILING_COMMENT_MARKER)
}

/// Consume `all` only when it is a whole word (not a `Foo`-prefixed name).
/// Returns the remainder after `all`, or `None` if `args` does not start with a
/// standalone `all`.
fn strip_prefix_word<'a>(args: &'a str, word: &str) -> Option<&'a str> {
    let rest = args.strip_prefix(word)?;
    match rest.chars().next() {
        Some(c) if c.is_alphanumeric() || c == '_' || c == '/' => None,
        _ => Some(rest),
    }
}

/// Greedily consume `cop (\s*,\s* cop)*` from the start of `args`. Returns the
/// unconsumed remainder, or `None` if no leading cop name is present.
fn match_cop_list(args: &str) -> Option<&str> {
    let mut rest = match_cop_name(args)?;
    loop {
        let after_ws = rest.trim_start();
        let Some(after_comma) = after_ws.strip_prefix(',') else {
            return Some(rest);
        };
        let after_comma = after_comma.trim_start();
        match match_cop_name(after_comma) {
            Some(r) => rest = r,
            // A trailing comma with no following cop name is malformed.
            None => return None,
        }
    }
}

/// Consume one `([A-Za-z]\w+/)*(?:[A-Za-z]\w+)` cop name from the start of `s`,
/// returning the remainder, or `None` if `s` does not start with a cop name.
fn match_cop_name(s: &str) -> Option<&str> {
    let consumed = cop_name_len(s);
    if consumed == 0 {
        None
    } else {
        Some(&s[consumed..])
    }
}

/// Consume `+Cop -Cop ...` push/pop args (possibly empty) from the start of
/// `args`, returning the remainder.
fn match_push_pop_args(args: &str) -> Option<&str> {
    let mut rest = args;
    loop {
        let after_ws = rest.trim_start();
        let Some(after_sign) = after_ws.strip_prefix(['+', '-']) else {
            return Some(rest);
        };
        // A bare `--` is the trailing-comment marker, not a push/pop arg.
        if after_ws.starts_with(TRAILING_COMMENT_MARKER) {
            return Some(rest);
        }
        let consumed = cop_name_len(after_sign);
        if consumed == 0 {
            return None;
        }
        rest = &after_sign[consumed..];
    }
}

/// Length in bytes of the leading `COP_NAME_PATTERN`
/// (`([A-Za-z]\w+/)*(?:[A-Za-z]\w+)`) at the start of `s`, or `0` if `s` does
/// not start with a cop name. Slash-separated segments are each a letter
/// followed by one-or-more word chars; a bare department (no slash) is valid,
/// and a trailing `/` is not consumed (RuboCop requires a final segment).
fn cop_name_len(s: &str) -> usize {
    let mut consumed = 0;
    loop {
        let seg_len = identifier_segment_len(&s[consumed..]);
        if seg_len == 0 {
            // No valid final segment after a `/` → not a cop name; back out.
            return 0;
        }
        consumed += seg_len;
        // Continue across `Department/Cop` separators, but only when another
        // identifier segment follows the `/`.
        let after = &s[consumed..];
        if let Some(rest) = after.strip_prefix('/') {
            if identifier_segment_len(rest) == 0 {
                return consumed;
            }
            consumed += 1; // the `/`
        } else {
            return consumed;
        }
    }
}

/// Length in bytes of a leading `[A-Za-z]\w+` segment (a letter followed by at
/// least one word char), or `0` if absent. A single-letter segment fails,
/// matching RuboCop's `\w+`.
fn identifier_segment_len(s: &str) -> usize {
    let mut chars = s.char_indices();
    let Some((_, first)) = chars.next() else {
        return 0;
    };
    if !first.is_ascii_alphabetic() {
        return 0;
    }
    let mut end = first.len_utf8();
    let mut count = 0;
    for (i, c) in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
            count += 1;
        } else {
            break;
        }
    }
    if count >= 1 { end } else { 0 }
}

murphy_plugin_api::submit_cop!(CopDirectiveSyntax);

#[cfg(test)]
mod tests {
    use super::CopDirectiveSyntax;
    use murphy_plugin_api::test_support::test;

    // ---- no-offense cases ----

    #[test]
    fn accepts_single_cop() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:disable Layout/LineLength\n");
    }

    #[test]
    fn accepts_bare_department() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:disable Layout\n");
    }

    #[test]
    fn accepts_multiple_cops() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:disable Layout/LineLength, Style/Encoding\n");
    }

    #[test]
    fn accepts_all() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:disable all\n");
    }

    #[test]
    fn accepts_enable() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:enable Layout/LineLength\n");
    }

    #[test]
    fn accepts_todo() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:todo Layout/LineLength\n");
    }

    #[test]
    fn accepts_quoted_non_directive() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# \"rubocop:disable Layout/LineLength\"\n");
    }

    #[test]
    fn accepts_double_comment() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# # rubocop:disable Layout/LineLength\n");
    }

    #[test]
    fn accepts_valid_trailing_comment() {
        test::<CopDirectiveSyntax>().expect_no_offenses(
            "# rubocop:disable Layout/LineLength -- This is a good comment.\n",
        );
    }

    #[test]
    fn accepts_inline_valid_trailing_comment() {
        test::<CopDirectiveSyntax>().expect_no_offenses(
            "a = 1 # rubocop:disable Layout/LineLength -- This is a good comment.\n",
        );
    }

    #[test]
    fn accepts_tab_separated_mode_and_cop() {
        // Ruby's `split(' ')` treats a tab as a delimiter, so the mode resolves
        // to `disable` (not `disable\tLayout/...`).
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:disable\tLayout/LineLength\n");
    }

    #[test]
    fn accepts_no_space_double_dash_comment() {
        // RuboCop's cop name stops at the non-word `-`, leaving `--bad` as the
        // post-match comment, which `start_with?('--')` accepts. Mirror that.
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:disable Layout/LineLength--bad\n");
    }

    // ---- offense cases ----

    #[test]
    fn flags_cops_without_comma() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disable Layout/LineLength Style/Encoding\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "Cop names must be separated by commas. Comment in the directive must start with `--`.\n",
        ));
    }

    #[test]
    fn flags_duplicate_directives() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disable Layout/LineLength # rubocop:disable Style/Encoding\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "Cop names must be separated by commas. Comment in the directive must start with `--`.\n",
        ));
    }

    #[test]
    fn flags_missing_cop_name() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disable\n",
            "^^^^^^^^^^^^^^^^^ Malformed directive comment detected. The cop name is missing.\n",
        ));
    }

    #[test]
    fn flags_invalid_mode() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disabled Layout/LineLength\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "The mode name must be one of `enable`, `disable`, `todo`, `push`, or `pop`.\n",
        ));
    }

    #[test]
    fn flags_missing_mode() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:\n",
            "^^^^^^^^^^ Malformed directive comment detected. The mode name is missing.\n",
        ));
    }

    #[test]
    fn flags_bad_trailing_comment() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disable Layout/LineLength == This is a bad comment.\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "Cop names must be separated by commas. Comment in the directive must start with `--`.\n",
        ));
    }
}
