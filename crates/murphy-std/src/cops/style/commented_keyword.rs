//! `Style/CommentedKeyword` — flags comments on same line as keywords.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CommentedKeyword
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks for comments on the same line as begin/class/def/end/module.
//!   Matches RuboCop's `KEYWORD_REGEXES` (`/^\s*<kw>\s/`): the keyword must be
//!   at the start of the line (after leading whitespace), which is also
//!   RuboCop's behaviour — `foo if bar # c` is intentionally not flagged.
//!   Exempts `:nodoc:`, `:yields:`, RuboCop directive comments
//!   (disable/enable/todo), `# steep:ignore`, and RBS inline annotations
//!   (`#[...]` after a subclass definition, `#:` after `def`/`end`).
//!   Autocorrect is a v1 gap (RuboCop relocates the comment above the keyword,
//!   or removes it for `end`; deferred).
//! ```

use murphy_plugin_api::{Cx, cop};

const KEYWORDS: &[&str] = &["begin", "class", "def", "end", "module"];
/// RuboCop `ALLOWED_COMMENTS` plus directive comments. `:nodoc:`/`:yields:`
/// are matched as `/#\s*<allowed>/`; directives (disable/enable/todo) are the
/// RuboCop `DirectiveComment::DIRECTIVE_COMMENT_REGEXP` family.
const ALLOWED_COMMENTS: &[&str] = &[":nodoc:", ":yields:"];
const DIRECTIVE_PREFIXES: &[&str] = &["rubocop:disable", "rubocop:enable", "rubocop:todo"];

#[derive(Default)]
pub struct CommentedKeyword;

#[cop(
    name = "Style/CommentedKeyword",
    description = "Do not place comments on the same line as certain keywords.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CommentedKeyword {
    #[on_new_investigation]
    fn check_investigation(&self, cx: &Cx<'_>) {
        let bytes = cx.source().as_bytes();
        for comment in cx.comments() {
            // `comment.text` (RuboCop): the comment source from `#` to its end,
            // trailing whitespace trimmed.
            let comment_text = cx.raw_source(comment.range).trim_end();
            if !comment_text.starts_with('#') {
                continue;
            }
            // The whole source line containing the comment (RuboCop `source_line`).
            let line_start = bytes[..comment.range.start as usize]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |p| p + 1);
            let line_end = bytes[comment.range.start as usize..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |p| comment.range.start as usize + p);
            let line = core::str::from_utf8(&bytes[line_start..line_end]).unwrap_or_default();

            if !offensive(line, comment_text) {
                continue;
            }
            // RuboCop derives the keyword from `REGEXP = /(?<keyword>\S+).*#/`,
            // i.e. the first non-whitespace token on the line. `offensive`
            // already guarantees that token is one of KEYWORDS.
            let Some(keyword) = line.split_whitespace().next() else {
                continue;
            };
            cx.emit_offense(
                comment.range,
                &format!("Do not place comments on the same line as the `{keyword}` keyword."),
                None,
            );
        }
    }
}

/// RuboCop `offensive?`: a comment on a keyword line that is neither an RBS
/// inline annotation, a Steep annotation, nor an allowed/directive comment.
fn offensive(line: &str, comment_text: &str) -> bool {
    if rbs_inline_annotation(line, comment_text) || steep_annotation(comment_text) {
        return false;
    }
    keyword_at_line_start(line) && !allowed_comment(line)
}

/// RuboCop `KEYWORD_REGEXES` = `KEYWORDS.map { |w| /^\s*#{w}\s/ }`: the first
/// non-whitespace token on the line is a keyword and is followed by whitespace.
fn keyword_at_line_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    KEYWORDS.iter().any(|&kw| {
        trimmed
            .strip_prefix(kw)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace()))
    })
}

/// RuboCop `ALLOWED_COMMENT_REGEXES`: `/#\s*:nodoc:/`, `/#\s*:yields:/`, and
/// the directive-comment family. RuboCop matches anywhere on the line.
fn allowed_comment(line: &str) -> bool {
    // `#` followed by optional whitespace then an allowed marker, anywhere.
    line.match_indices('#').any(|(i, _)| {
        let after_hash = line[i + 1..].trim_start();
        ALLOWED_COMMENTS.iter().any(|m| after_hash.starts_with(m))
            || DIRECTIVE_PREFIXES.iter().any(|d| after_hash.starts_with(d))
    })
}

/// RuboCop `rbs_inline_annotation?`: `#[...]` after a subclass definition, or
/// `#:` after a `def`/`end` line.
fn rbs_inline_annotation(line: &str, comment_text: &str) -> bool {
    if is_subclass_definition(line) {
        // `comment.text.start_with?(/#\[.+\]/)` — `#[`, at least one char, `]`.
        if let Some(rest) = comment_text.strip_prefix("#[") {
            return rest.contains(']') && !rest.starts_with(']');
        }
        return false;
    }
    if is_method_or_end_definition(line) {
        return comment_text.starts_with("#:");
    }
    false
}

/// RuboCop `SUBCLASS_DEFINITION = /\A\s*class\s+(\w|::)+\s*<\s*(\w|::)+/`.
fn is_subclass_definition(line: &str) -> bool {
    let rest = line.trim_start();
    let Some(after_class) = rest.strip_prefix("class") else {
        return false;
    };
    // `class` must be followed by whitespace, then `Name < Super`.
    if !after_class.starts_with(|c: char| c.is_whitespace()) {
        return false;
    }
    let after_class = after_class.trim_start();
    let Some((lhs, rhs)) = after_class.split_once('<') else {
        return false;
    };
    is_const_path(lhs.trim()) && is_const_path(rhs.trim_start())
}

/// RuboCop `METHOD_OR_END_DEFINITIONS = /\A\s*(def\s|end)/`.
fn is_method_or_end_definition(line: &str) -> bool {
    let rest = line.trim_start();
    rest.strip_prefix("def")
        .is_some_and(|r| r.starts_with(|c: char| c.is_whitespace()))
        || rest.starts_with("end")
}

/// Matches RuboCop's `(\w|::)+` constant-path fragment: one or more word
/// characters or `::` separators (the leading run is enough for the anchored
/// regex, so we only require a non-empty word/`::` prefix).
fn is_const_path(s: &str) -> bool {
    let mut chars = s.char_indices().peekable();
    let mut matched = false;
    while let Some(&(i, c)) = chars.peek() {
        if c == '_' || c.is_alphanumeric() {
            matched = true;
            chars.next();
        } else if c == ':' && s[i..].starts_with("::") {
            matched = true;
            chars.next();
            chars.next();
        } else {
            break;
        }
    }
    matched
}

/// RuboCop `STEEP_REGEXP = /#\ssteep:ignore(\s|\z)/`: `#` + whitespace +
/// `steep:ignore` + (whitespace or end of comment).
fn steep_annotation(comment_text: &str) -> bool {
    let mut search = comment_text;
    while let Some(pos) = search.find('#') {
        let after_hash = &search[pos + 1..];
        if let Some(rest) = after_hash
            .strip_prefix(|c: char| c.is_whitespace())
            .and_then(|r| r.strip_prefix("steep:ignore"))
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()))
        {
            return true;
        }
        search = after_hash;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::CommentedKeyword;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_end_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            if condition
              statement
            end # end if
                ^^^^^^^^ Do not place comments on the same line as the `end` keyword.
        "});
    }

    #[test]
    fn flags_class_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            class X # comment
                    ^^^^^^^^^ Do not place comments on the same line as the `class` keyword.
              statement
            end
        "});
    }

    #[test]
    fn accepts_nodoc() {
        test::<CommentedKeyword>().expect_no_offenses(
            "class X # :nodoc:\n  y\nend\n",
        );
    }

    #[test]
    fn accepts_no_comment() {
        test::<CommentedKeyword>().expect_no_offenses(
            "class X\nend\n",
        );
    }

    #[test]
    fn flags_begin_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            begin # start processing
                  ^^^^^^^^^^^^^^^^^^ Do not place comments on the same line as the `begin` keyword.
              process
            end
        "});
    }

    #[test]
    fn flags_def_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            def foo # method definition
                    ^^^^^^^^^^^^^^^^^^^ Do not place comments on the same line as the `def` keyword.
            end
        "});
    }

    #[test]
    fn flags_module_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            module M # namespace
                     ^^^^^^^^^^^ Do not place comments on the same line as the `module` keyword.
            end
        "});
    }

    #[test]
    fn flags_indented_keyword() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            def foo
              if cond
              end # end if
                  ^^^^^^^^ Do not place comments on the same line as the `end` keyword.
            end
        "});
    }

    #[test]
    fn accepts_yields() {
        test::<CommentedKeyword>().expect_no_offenses("def foo # :yields:\nend\n");
    }

    #[test]
    fn accepts_rubocop_directive() {
        test::<CommentedKeyword>()
            .expect_no_offenses("def foo # rubocop:disable Style/For\nend\n");
    }

    #[test]
    fn accepts_rubocop_enable_directive() {
        test::<CommentedKeyword>()
            .expect_no_offenses("def foo # rubocop:enable Style/For\nend\n");
    }

    #[test]
    fn accepts_steep_ignore() {
        test::<CommentedKeyword>().expect_no_offenses("def foo # steep:ignore\nend\n");
    }

    #[test]
    fn accepts_steep_ignore_with_code() {
        test::<CommentedKeyword>()
            .expect_no_offenses("def foo # steep:ignore NoMethod\nend\n");
    }

    // RBS inline: `#:` after a `def` line is a signature annotation.
    #[test]
    fn accepts_rbs_def_signature() {
        test::<CommentedKeyword>().expect_no_offenses("def foo #: () -> void\nend\n");
    }

    // RBS inline: `#:` after `end`.
    #[test]
    fn accepts_rbs_end_signature() {
        test::<CommentedKeyword>().expect_no_offenses("def foo\nend #: void\n");
    }

    // RBS inline: `#[...]` after a subclass definition is a generics annotation.
    #[test]
    fn accepts_rbs_subclass_generics() {
        test::<CommentedKeyword>()
            .expect_no_offenses("class Foo < Array #[Integer]\nend\n");
    }

    // A non-RBS comment after a subclass definition still fires.
    #[test]
    fn flags_subclass_with_plain_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            class Foo < Array # subclass
                              ^^^^^^^^^^ Do not place comments on the same line as the `class` keyword.
            end
        "});
    }

    // A non-RBS comment after `def` (not `#:`) still fires.
    #[test]
    fn flags_def_with_plain_comment_not_rbs() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            def foo # not a signature
                    ^^^^^^^^^^^^^^^^^ Do not place comments on the same line as the `def` keyword.
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(CommentedKeyword);
