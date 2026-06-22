//! `Migration/DepartmentName` — check that cop names in `rubocop:disable`,
//! `rubocop:enable`, and `rubocop:todo` directive comments are given with their
//! department name (e.g. `Layout/LineLength`, not a bare `LineLength`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Migration/DepartmentName
//! upstream_version_checked: 1.87.0
//! version_added: "0.75"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Hand-rolled port of RuboCop's `DISABLE_COMMENT_FORMAT` regex
//!   (`/\A(# *rubocop *: *((dis|en)able|todo) +)(.*)/`) — no `regex` dependency
//!   in murphy-std. The cop-name list is tokenised by `scan_tokens` into
//!   maximal comma runs and maximal non-comma runs (a faithful-enough analogue
//!   of RuboCop's `cop_names.scan(/[^,]+|\W+/)` for offense detection; it
//!   differs only in that `", "` becomes `","` + `" Bar"` instead of one
//!   `", "` separator, which is why the offense-range calc below skips a token's
//!   leading whitespace). Detection covers the three directive modes
//!   (`disable`/`enable`/`todo`; `push`/`pop` are deliberately out of scope,
//!   matching RuboCop's regex), the comma-separated token list, the `break` on
//!   the first token containing a character outside `[A-Za-z/, ]` (which
//!   terminates the scan at a trailing `-- comment`), and the three "valid
//!   token" branches: a token containing any non-word char (`/\W+/` partial
//!   match — e.g. has a slash, space, or dash), the `[A-Za-z]+/[A-Za-z]+|all`
//!   partial match, and a registered department. Offense range = the trimmed
//!   bare cop name (leading whitespace skipped), byte-precise per RuboCop's
//!   `range_between(begin_pos + offset, + name.length)`.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop prepends the department via
//!   `Registry.global.qualified_cop_name` (e.g. `AbcSize` -> `Metrics/AbcSize`),
//!   which requires the full ~500-cop bare-name -> department table; murphy-std
//!   has no such registry, so the detect-only port ships without it.
//!
//!   GAP — department set: RuboCop's `department?` consults the live registry,
//!   so with rubocop-rails / rubocop-rspec loaded, bare `Rails` / `RSpec` are
//!   accepted as departments. murphy-std ships only the core department set, so
//!   a bare `# rubocop:disable RSpec` would flag here. Acceptable documented
//!   divergence (murphy-std cannot see plugin departments), not a tracked gap.
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

const MSG: &str = "Department name is missing.";

/// RuboCop's core department set (`Registry.global.departments` with no plugins
/// loaded, verified against rubocop 1.87.0). A bare token matching one of these
/// is a department reference, not a department-less cop name, so it is accepted.
const CORE_DEPARTMENTS: &[&str] = &[
    "Bundler", "Gemspec", "Layout", "Lint", "Metrics", "Migration", "Naming", "Security", "Style",
];

#[derive(Default)]
pub struct DepartmentName;

#[cop(
    name = "Migration/DepartmentName",
    description = "Check that cop names in rubocop:disable (etc) comments are given with department name.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DepartmentName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        for comment in cx.comments() {
            let text = cx.raw_source(comment.range);

            // `next if comment.text !~ DISABLE_COMMENT_FORMAT` — only
            // `disable`/`enable`/`todo` directives, with the prefix captured so
            // its length is the starting `offset` into the comment.
            let Some(prefix_len) = directive_prefix_len(text) else {
                continue;
            };

            // `offset` tracks the byte position of the current token within the
            // comment, mirroring RuboCop's `offset += name.length`.
            let mut offset = prefix_len;
            let cop_names = &text[prefix_len..];

            for token in scan_tokens(cop_names) {
                let trimmed = token.trim();

                if !valid_content_token(trimmed) {
                    // Offense range = the trimmed bare cop name. The comma-run
                    // scan groups `", "` as `","` + `" Bar"`, so a flagged token
                    // can carry leading whitespace (e.g. `" Bar"` after the
                    // comma); advance past it so the range starts at the first
                    // byte of `trimmed`, matching RuboCop's `begin_pos`.
                    let leading_ws = token.len() - token.trim_start().len();
                    let start = comment.range.start + offset as u32 + leading_ws as u32;
                    let range = Range { start, end: start + trimmed.len() as u32 };
                    cx.emit_offense(range, MSG, None);
                }

                // `break if contain_unexpected_character_for_department_name?`.
                // Stops the scan at the first token containing a character
                // outside `[A-Za-z/, ]` — this is what terminates scanning at a
                // trailing `-- comment` so prose words are never flagged.
                if contains_unexpected_character(token) {
                    break;
                }

                offset += token.len();
            }
        }
    }
}

/// `DISABLE_COMMENT_FORMAT = /\A(# *rubocop *: *((dis|en)able|todo) +)(.*)/`.
/// Returns the byte length of capture group 1 (the directive prefix, including
/// its trailing run of spaces) when `text` matches, else `None`. Spaces only —
/// RuboCop's regex uses literal ` `, not `\s`, so tabs do not match.
fn directive_prefix_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;

    // `#`
    if bytes.first() != Some(&b'#') {
        return None;
    }
    i += 1;
    i += count_spaces(&bytes[i..]);

    // `rubocop`
    let rest = text.get(i..)?;
    let rest = rest.strip_prefix("rubocop")?;
    i = text.len() - rest.len();
    i += count_spaces(&bytes[i..]);

    // `:`
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    i += count_spaces(&bytes[i..]);

    // `(dis|en)able|todo`
    let rest = text.get(i..)?;
    let mode = ["disable", "enable", "todo"]
        .into_iter()
        .find(|m| rest.starts_with(m))?;
    i += mode.len();

    // ` +` — at least one trailing space is required by the regex.
    let trailing = count_spaces(&bytes[i..]);
    if trailing == 0 {
        return None;
    }
    i += trailing;

    Some(i)
}

/// Count the leading run of ASCII space (0x20) bytes.
fn count_spaces(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|&&b| b == b' ').count()
}

/// Reproduce Ruby's `cop_names.scan(/[^,]+|\W+/)`: the input splits into maximal
/// runs of non-comma bytes (`[^,]+`) and maximal runs of commas (the only ASCII
/// chars matched by `\W+` once non-comma chars are claimed by `[^,]+` first).
fn scan_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let is_comma = bytes[i] == b',';
        let start = i;
        while i < bytes.len() && (bytes[i] == b',') == is_comma {
            i += 1;
        }
        tokens.push(&s[start..i]);
    }
    tokens
}

/// `valid_content_token?(content_token)` — a token is acceptable when it
/// matches `/\W+/` (contains any non-word char), matches
/// `%r{[A-Za-z]+/[A-Za-z]+|all}` (qualified name or the `all` keyword), or is a
/// registered department. Both regexes are `match?` (partial), so a substring
/// match suffices.
fn valid_content_token(token: &str) -> bool {
    contains_non_word_char(token)
        || contains_qualified_name_or_all(token)
        || CORE_DEPARTMENTS.contains(&token)
}

/// `/\W+/.match?` — true when the token contains at least one char that is not
/// `[A-Za-z0-9_]`. An empty token has no such char and so does not match.
fn contains_non_word_char(token: &str) -> bool {
    token.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// `%r{[A-Za-z]+/[A-Za-z]+|all}.match?` — partial match for either a
/// `Letters/Letters` substring or the literal substring `all`. The
/// `Letters/Letters` branch is, in practice, shadowed by `contains_non_word_char`
/// (any `/` is already a non-word char), faithfully mirroring RuboCop's own
/// redundant alternation against its leading `/\W+/` check — kept for a 1:1 port.
fn contains_qualified_name_or_all(token: &str) -> bool {
    token.contains("all") || contains_qualified_name(token)
}

/// Partial match for `[A-Za-z]+/[A-Za-z]+`: a run of ASCII letters, a `/`, then
/// another run of ASCII letters, appearing anywhere in `token`.
fn contains_qualified_name(token: &str) -> bool {
    let bytes = token.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        let left = bytes[..i].iter().rev().take_while(|&&c| c.is_ascii_alphabetic()).count();
        let right = bytes[i + 1..].iter().take_while(|&&c| c.is_ascii_alphabetic()).count();
        if left >= 1 && right >= 1 {
            return true;
        }
    }
    false
}

/// `contain_unexpected_character_for_department_name?(name)` —
/// `name.match?(%r{[^A-Za-z/, ]})`, true when `name` contains any char outside
/// the set `[A-Za-z/, ]` (letters, slash, comma, space). Operates on the
/// untrimmed token, matching RuboCop.
fn contains_unexpected_character(name: &str) -> bool {
    name.chars()
        .any(|c| !(c.is_ascii_alphabetic() || c == '/' || c == ',' || c == ' '))
}

murphy_plugin_api::submit_cop!(DepartmentName);

#[cfg(test)]
mod tests {
    use super::DepartmentName;
    use murphy_plugin_api::test_support::test;

    // ---- no-offense cases ----

    #[test]
    fn accepts_qualified_cop_name() {
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Layout/LineLength\n");
    }

    #[test]
    fn accepts_bare_department() {
        // `Layout` is a registered department, not a department-less cop name.
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Layout\n");
    }

    #[test]
    fn accepts_all() {
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable all\n");
    }

    #[test]
    fn accepts_multiple_qualified_cops() {
        test::<DepartmentName>()
            .expect_no_offenses("# rubocop:disable Layout/LineLength, Style/Encoding\n");
    }

    #[test]
    fn accepts_qualified_cop_with_trailing_comment() {
        // The `--` comment contains chars outside `[A-Za-z/, ]`, so the scan
        // breaks before reaching the prose words.
        test::<DepartmentName>().expect_no_offenses(
            "# rubocop:disable Layout/LineLength -- Because Reasons Here\n",
        );
    }

    #[test]
    fn accepts_token_containing_all_substring() {
        // `Marshalling` contains the substring `all`, so the `all` partial match
        // makes it valid (a RuboCop quirk we faithfully reproduce).
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Marshalling\n");
    }

    #[test]
    fn ignores_push_directive() {
        // `push`/`pop` are not matched by `DISABLE_COMMENT_FORMAT`.
        test::<DepartmentName>().expect_no_offenses("# rubocop:push AbcSize\n");
    }

    #[test]
    fn ignores_non_directive_comment() {
        test::<DepartmentName>().expect_no_offenses("# just a comment AbcSize\n");
    }

    #[test]
    fn requires_trailing_space_after_mode() {
        // ` +` requires at least one space after the mode; `disable` glued to
        // the cop name is not a directive.
        test::<DepartmentName>().expect_no_offenses("# rubocop:disableAbcSize\n");
    }

    // ---- offense cases ----

    #[test]
    fn flags_bare_cop_name() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable LineLength\n",
            "                        ^^^^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_bare_cop_name_enable() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:enable AbcSize\n",
            "                       ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_bare_cop_name_todo() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:todo AbcSize\n",
            "                     ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_only_the_bare_cop_in_a_mixed_list() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable AbcSize, Metrics/MethodLength\n",
            "                        ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_each_bare_cop_in_a_list() {
        // RuboCop 1.87.0 reports both at cols 25 and 30 (verified against
        // standalone rubocop) — `Foo` then `Bar` after the `, ` separator.
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable Foo, Bar\n",
            "                        ^^^ Department name is missing.\n",
            "                             ^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_lowercase_bare_cop_name() {
        // Pure word chars, not a department, no slash, no `all` substring.
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable abc\n",
            "                        ^^^ Department name is missing.\n",
        ));
    }
}
