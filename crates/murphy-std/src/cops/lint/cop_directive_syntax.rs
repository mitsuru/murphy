//! `Lint/CopDirectiveSyntax` — validate the strict formatting of
//! `# rubocop:enable`/`disable`/`todo`/`push`/`pop`/`disable-next`/
//! `todo-next`/`enable-next`/`next` directive comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/CopDirectiveSyntax
//! upstream_version_checked: 1.91.0
//! version_added: "1.72"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Hand-rolled port of RuboCop's `DirectiveComment` regex stack (no `regex`
//!   dependency in murphy-std). Covers the five offense messages (missing mode,
//!   invalid mode, missing cop name, malformed cop names, invalid signed args)
//!   plus the no-offense cases (bare department, `all`, valid trailing
//!   `-- comment`, tab-separated mode, double-comment and quoted
//!   non-directive). Ports the 1.91 `-next` modes (`disable-next`,
//!   `todo-next`, `enable-next` take a comma-separated cop list like
//!   `disable`; `next` takes signed `+Cop -Cop` args like `push` but requires
//!   them, and `pop` takes no arguments). Mode extraction follows Ruby's
//!   `split(' ')` whitespace-run semantics (the whole hyphenated token, so no
//!   longest-first header matching is needed); the trailing-comment check
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
    "The mode name must be one of `enable`, `disable`, `disable-next`, `enable-next`, `todo`, `todo-next`, `next`, `push`, or `pop`.";
const MISSING_COP_NAME_MSG: &str = "The cop name is missing.";
const MALFORMED_COP_NAMES_MSG: &str =
    "Cop names must be separated by commas. Comment in the directive must start with `--`.";
const INVALID_SIGNED_ARGS_MSG: &str =
    "`push` and `next` arguments must be `+`- or `-`-prefixed cop names, and `pop` takes no arguments.";

const AVAILABLE_MODES: &[&str] = &[
    "disable",
    "enable",
    "todo",
    "push",
    "pop",
    "disable-next",
    "todo-next",
    "enable-next",
    "next",
];

/// Argument shape by mode, mirroring RuboCop's `DirectiveComment` predicates.
/// `disable`, `enable`, `todo` and the `-next` single-statement variants
/// (`disable-next`, `todo-next`, `enable-next`) take a comma-separated cop
/// list; `push` and `next` take signed `+Cop -Cop` args (`next` requires them,
/// `push` does not); `pop` takes no arguments at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DirectiveKind {
    List,
    Push,
    Pop,
    Next,
}

fn directive_kind(mode: &str) -> DirectiveKind {
    match mode {
        "push" => DirectiveKind::Push,
        "pop" => DirectiveKind::Pop,
        "next" => DirectiveKind::Next,
        _ => DirectiveKind::List,
    }
}

/// Which arm of RuboCop's `DIRECTIVE_COMMENT_REGEXP` argument alternation
/// (`COPS_PATTERN` first, `PUSH_POP_ARGS_PATTERN` second) matched the start
/// of `args`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgsAlternative {
    /// `(all | cop (, cop)*)` — comma-separated cop list (unsigned by shape).
    CopList,
    /// `+Cop -Cop ...` — signed run.
    Signed,
}
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

    let kind = directive_kind(mode);

    // RuboCop's `missing_cop_name?` is false for `push`/`pop` (a bare
    // `# rubocop:push` is valid); every other mode — including `next` —
    // requires a cop name, so empty args mean a bare `# rubocop:<mode>`.
    if !matches!(kind, DirectiveKind::Push | DirectiveKind::Pop) && missing_cop_name(args) {
        return Some(format!("{COMMON_MSG} {MISSING_COP_NAME_MSG}"));
    }

    // RuboCop's `invalid_signed_args?`: `pop` takes no arguments, and `push` /
    // `next` arguments must be `+`/`-`-prefixed (an unsigned leading cop list
    // matches the `COPS_PATTERN` arm but fails the signed-args check).
    if invalid_signed_args(kind, args) {
        return Some(format!("{COMMON_MSG} {INVALID_SIGNED_ARGS_MSG}"));
    }

    // Well-formed: `(all | cop, cop, ...)` (or push/next `+a -b` args)
    // optionally followed by ` -- comment`. Anything else is malformed
    // cop-name syntax.
    if well_formed_args(kind, args) {
        return None;
    }
    Some(format!("{COMMON_MSG} {MALFORMED_COP_NAMES_MSG}"))
}

/// RuboCop's `missing_cop_name?` for cop-requiring modes: the args are empty
/// (a bare `# rubocop:disable`).
fn missing_cop_name(args: &str) -> bool {
    args.is_empty()
}

/// Mirror RuboCop's `invalid_signed_args?`. `pop` with any captured args is
/// invalid; `push`/`next` are invalid when the args lead with an unsigned cop
/// list (the `COPS_PATTERN` arm); list modes never are.
fn invalid_signed_args(kind: DirectiveKind, args: &str) -> bool {
    match kind {
        DirectiveKind::Pop => match_cops_alternative(args).is_some(),
        DirectiveKind::Push | DirectiveKind::Next => {
            matches!(
                match_cops_alternative(args),
                Some((ArgsAlternative::CopList, _))
            )
        }
        DirectiveKind::List => false,
    }
}

/// Mirror the `DIRECTIVE_COMMENT_REGEXP` argument alternation: try the
/// comma-separated cop list (`COPS_PATTERN`: `all` or `cop (, cop)*`) first,
/// then the signed `+Cop -Cop` run (`PUSH_POP_ARGS_PATTERN`). Returns which
/// arm matched plus the unconsumed remainder, or `None` when neither matches
/// at the start of `args`.
fn match_cops_alternative(args: &str) -> Option<(ArgsAlternative, &str)> {
    if let Some(rest) = strip_prefix_word(args, "all") {
        return Some((ArgsAlternative::CopList, rest));
    }
    if let Some(rest) = match_cop_list(args) {
        return Some((ArgsAlternative::CopList, rest));
    }
    if let Some(rest) = match_signed_run(args) {
        return Some((ArgsAlternative::Signed, rest));
    }
    None
}

/// Mirror RuboCop's `malformed?` tail check: greedily match the cop-list
/// (`all`, comma-separated cop names, or push/next `+a -b` args) from the
/// start of `args`, then require the remainder — after lstrip — to be empty
/// or to start with the `--` trailing-comment marker. RuboCop checks
/// `post_match.lstrip.start_with?('--')`, so `Foo--bad` (no space before
/// `--`) is intentionally accepted: the cop name stops at the non-word `-`,
/// leaving `--bad` as the post-match comment.
fn well_formed_args(kind: DirectiveKind, args: &str) -> bool {
    let rest = match kind {
        DirectiveKind::List => {
            if let Some(rest) = strip_prefix_word(args, "all") {
                rest
            } else {
                match match_cop_list(args) {
                    Some(rest) => rest,
                    // A list mode whose args match the signed run instead
                    // (e.g. `# rubocop:disable +Foo`) is accepted upstream:
                    // the `PUSH_POP_ARGS_PATTERN` arm captures them and the
                    // tail check passes.
                    None => match match_signed_run(args) {
                        Some(rest) => rest,
                        None => return false,
                    },
                }
            }
        }
        DirectiveKind::Push | DirectiveKind::Next | DirectiveKind::Pop => {
            match match_cops_alternative(args) {
                Some((_, rest)) => rest,
                // No capturable args (bare `push`/`pop`, or a `--` trailing
                // comment alone): the tail check below decides.
                None => args,
            }
        }
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

/// Consume a leading `+Cop -Cop ...` signed run (`PUSH_POP_ARGS_PATTERN`)
/// from the start of `args`, returning the remainder, or `None` when `args`
/// does not lead with a signed cop name. A bare `--` trailing-comment marker
/// is not an arg.
fn match_signed_run(args: &str) -> Option<&str> {
    let mut rest = args;
    let mut consumed_any = false;
    loop {
        let after_ws = rest.trim_start();
        // A bare `--` is the trailing-comment marker, not a signed arg.
        if after_ws.starts_with(TRAILING_COMMENT_MARKER) {
            break;
        }
        let Some(after_sign) = after_ws.strip_prefix(['+', '-']) else {
            break;
        };
        let consumed = cop_name_len(after_sign);
        if consumed == 0 {
            return None;
        }
        rest = &after_sign[consumed..];
        consumed_any = true;
    }
    if consumed_any { Some(rest) } else { None }
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
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
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
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "The mode name must be one of `enable`, `disable`, `disable-next`, ",
            "`enable-next`, `todo`, `todo-next`, `next`, `push`, or `pop`.\n",
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
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "Cop names must be separated by commas. Comment in the directive must start with `--`.\n",
        ));
    }

    // ---- push/pop mode ----

    #[test]
    fn accepts_bare_push() {
        // `push`/`pop` need no cop name (RuboCop's `missing_cop_name?` is false).
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:push\n");
    }

    #[test]
    fn accepts_bare_pop() {
        test::<CopDirectiveSyntax>().expect_no_offenses("# rubocop:pop\n");
    }

    #[test]
    fn accepts_push_with_signed_cops() {
        // push/pop args are `+Cop -Cop`, not the comma-separated cop list.
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:push +Layout/LineLength -Style/Encoding\n");
    }

    #[test]
    fn accepts_push_with_trailing_comment() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:push +Layout/LineLength -- reason\n");
    }

    #[test]
    fn flags_push_with_unsigned_cop_name() {
        // A push arg without a `+`/`-` sign matches the cop-list arm but fails
        // RuboCop's `invalid_signed_args?` check (verified vs rubocop 1.91.0).
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:push Layout/LineLength\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "`push` and `next` arguments must be `+`- or `-`-prefixed cop names, ",
            "and `pop` takes no arguments.\n",
        ));
    }

    // ---- `-next` modes (RuboCop 1.91; Mastodon `disable-next` FPs) ----

    #[test]
    fn accepts_disable_next_with_cop() {
        // Mastodon shape (e.g. `# rubocop:disable-next Rails/OutputSafety`):
        // valid in rubocop 1.91.0 full-config, so no offense.
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:disable-next Layout/LineLength\n");
    }

    #[test]
    fn accepts_todo_next_with_cop() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:todo-next Layout/LineLength\n");
    }

    #[test]
    fn accepts_enable_next_with_cop() {
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:enable-next Layout/LineLength\n");
    }

    #[test]
    fn accepts_next_with_signed_cops() {
        // `next` takes signed `+Cop -Cop` args like `push`.
        test::<CopDirectiveSyntax>()
            .expect_no_offenses("# rubocop:next +Layout/LineLength -Style/Encoding\n");
    }

    #[test]
    fn flags_bare_disable_next_missing_cop() {
        // `next` is not exempt from `missing_cop_name?`, unlike `push`/`pop`.
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:disable-next\n",
            "^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "The cop name is missing.\n",
        ));
    }

    #[test]
    fn flags_next_with_unsigned_cop_name() {
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:next Layout/LineLength\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "`push` and `next` arguments must be `+`- or `-`-prefixed cop names, ",
            "and `pop` takes no arguments.\n",
        ));
    }

    #[test]
    fn flags_pop_with_args() {
        // `pop` takes no arguments at all (verified vs rubocop 1.91.0).
        test::<CopDirectiveSyntax>().expect_offense(concat!(
            "# rubocop:pop +Layout/LineLength\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Malformed directive comment detected. ",
            "`push` and `next` arguments must be `+`- or `-`-prefixed cop names, ",
            "and `pop` takes no arguments.\n",
        ));
    }
}
