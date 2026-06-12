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
//! status: partial
//! gap_issues:
//!   - murphy-x57c
//! notes: >
//!   Hand-rolled port of RuboCop's `DirectiveComment` regex stack (no `regex`
//!   dependency in murphy-std). Covers the four offense messages (missing mode,
//!   invalid mode, missing cop name, malformed cop names) plus the no-offense
//!   cases (bare department, `all`, valid trailing `-- comment`, double-comment
//!   and quoted non-directive). Only `rubocop:` directives are validated, mirroring
//!   RuboCop exactly — Murphy's own `murphy:` directives are intentionally not
//!   policed by this cop.
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
    // `mode = after_marker.split(' ', 2).first` — first whitespace-delimited
    // token. Empty remainder → missing mode.
    let mode = after_marker.split(' ').next().unwrap_or("");
    if mode.is_empty() {
        return Some(format!("{COMMON_MSG} {MISSING_MODE_NAME_MSG}"));
    }
    if !AVAILABLE_MODES.contains(&mode) {
        return Some(format!("{COMMON_MSG} {INVALID_MODE_NAME_MSG}"));
    }

    // The argument portion after the mode token.
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

/// Split the args into the cop-list portion and the trailing-comment portion,
/// then validate. `all` and comma-separated cop names are accepted; the tail
/// (if any) must start with `--`.
fn well_formed_args(args: &str, is_push_pop: bool) -> bool {
    if is_push_pop {
        return well_formed_push_pop(args);
    }

    // Split off a `--` trailing comment if present.
    let (cops_part, tail_ok) = split_trailing_comment(args);
    if !tail_ok {
        return false;
    }
    let cops_part = cops_part.trim();
    if cops_part == "all" {
        return true;
    }
    valid_cop_list(cops_part)
}

/// Validate `+Cop -Cop ...` push/pop args (or empty) plus optional `-- comment`.
fn well_formed_push_pop(args: &str) -> bool {
    let (head, tail_ok) = split_trailing_comment(args);
    if !tail_ok {
        return false;
    }
    let head = head.trim();
    if head.is_empty() {
        return true;
    }
    head.split_whitespace().all(|tok| {
        (tok.starts_with('+') || tok.starts_with('-')) && is_cop_name(&tok[1..])
    })
}

/// Returns `(cops_portion, tail_ok)`. When a `--` marker is present, the tail
/// after it is treated as a free-form comment (always ok); when leftover
/// non-`--` text would follow the cop list, validation falls to the cop-list
/// check, which rejects it.
fn split_trailing_comment(args: &str) -> (&str, bool) {
    if let Some(idx) = args.find(TRAILING_COMMENT_MARKER) {
        // Everything before `--` is the cop list; the rest is a valid comment.
        (&args[..idx], true)
    } else {
        (args, true)
    }
}

/// `COP_NAMES_PATTERN` — one or more comma-separated cop names. Whitespace
/// around commas is tolerated (RuboCop's pattern uses `\s*`).
fn valid_cop_list(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.split(',').map(str::trim).all(is_cop_name)
}

/// `COP_NAME_PATTERN` = `([A-Za-z]\w+/)*(?:[A-Za-z]\w+)` — slash-separated
/// segments, each an identifier (letter then one-or-more word chars). A bare
/// department (no slash) is valid.
fn is_cop_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.split('/').all(is_identifier_segment)
}

/// A single `[A-Za-z]\w+` segment: a letter followed by at least one word char
/// (so a single-letter segment fails, matching RuboCop's `\w+`).
fn is_identifier_segment(seg: &str) -> bool {
    let mut chars = seg.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let mut count = 0;
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        count += 1;
    }
    count >= 1
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
