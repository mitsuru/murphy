//! `Rails/SquishedSQLHeredocs` — use `.squish` on SQL heredocs.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SquishedSQLHeredocs
//! upstream_version_checked: 2.35.0
//! version_added: "2.8"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_heredoc`: flags heredoc `str` /
//!   `dstr` nodes whose delimiter is `SQL` (detected via the
//!   `HeredocStart` token, mirroring `delimiter_string`), unless the
//!   heredoc is already `.squish`ed (parent is a `squish` send) or the
//!   body contains a `--` single-line comment outside `"..."`, `'...'`
//!   and `[...]` identifier markers. Offense is the heredoc opener;
//!   autocorrect appends `.squish` after it. Autocorrect is unsafe
//!   upstream (`SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SquishedSQLHeredocs;

#[cop(
    name = "Rails/SquishedSQLHeredocs",
    description = "Checks SQL heredocs to use `.squish`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl SquishedSQLHeredocs {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_)) {
        return;
    }
    let Some(opener) = heredoc_opener(node, cx) else {
        return;
    };
    // Upstream `sql_heredoc?`: `delimiter_string(node) == 'SQL'`.
    if heredoc_delimiter(cx, opener) != Some("SQL".to_string()) {
        return;
    }
    // Upstream `using_squish?`: `node.parent&.send_type? && method?(:squish)`.
    if let Some(parent) = cx.parent(node).get()
        && matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && cx.method_name(parent) == Some("squish")
    {
        return;
    }
    // Upstream `singleline_comments_present?`.
    if has_singleline_comment(cx, opener) {
        return;
    }
    let opener_src = cx.raw_source(opener).to_owned();
    cx.emit_offense(
        opener,
        &format!("Use `{opener_src}.squish` instead of `{opener_src}`."),
        None,
    );
    cx.emit_edit(
        Range {
            start: opener.end,
            end: opener.end,
        },
        ".squish",
    );
}

/// The `<<SQL` / `<<-SQL` / `<<~SQL` opener token of a heredoc string node.
fn heredoc_opener(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    cx.tokens_in(cx.range(node))
        .iter()
        .find(|t| t.kind == SourceTokenKind::HeredocStart)
        .map(|t| t.range)
}

/// The heredoc delimiter without opener prefix or quotes (`SQL`).
fn heredoc_delimiter(cx: &Cx<'_>, opener: Range) -> Option<String> {
    let src = cx.raw_source(opener);
    let delim = src
        .strip_prefix("<<~")
        .or_else(|| src.strip_prefix("<<-"))
        .or_else(|| src.strip_prefix("<<"))?;
    let delim = delim
        .strip_prefix('"')
        .or_else(|| delim.strip_prefix('\''))
        .or_else(|| delim.strip_prefix('`'))
        .unwrap_or(delim);
    let delim = delim
        .strip_suffix('"')
        .or_else(|| delim.strip_suffix('\''))
        .or_else(|| delim.strip_suffix('`'))
        .unwrap_or(delim);
    Some(delim.to_owned())
}

/// Upstream `singleline_comments_present?`: the SQL with `"..."`, `'...'`
/// and `[...]` identifier markers removed still contains `--`.
fn has_singleline_comment(cx: &Cx<'_>, opener: Range) -> bool {
    // A heredoc `str` node's own range covers just the opener, so read
    // the body (opener end through heredoc-end token start) from source.
    let mut end_start = cx.source().len() as u32;
    let mut seen_opener = false;
    for tok in cx.sorted_tokens() {
        if tok.range == opener {
            seen_opener = true;
            continue;
        }
        if seen_opener && tok.kind == SourceTokenKind::HeredocEnd {
            end_start = tok.range.start;
            break;
        }
    }
    let body = cx
        .source()
        .get(opener.end as usize..end_start as usize)
        .unwrap_or("");
    strip_identifier_markers(body).contains("--")
}

/// Removes `"..."`, `'...'` and `[...]` spans (no escape handling —
/// matches the upstream `SQL_IDENTIFIER_MARKERS` regexp semantics).
fn strip_identifier_markers(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        let closer = match c {
            '"' => Some('"'),
            '\'' => Some('\''),
            '[' => Some(']'),
            _ => None,
        };
        if let Some(close) = closer {
            for inner in chars.by_ref() {
                if inner == close {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SquishedSQLHeredocs;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiline_heredoc() {
        test::<SquishedSQLHeredocs>().expect_correction(
            indoc! {r#"
                <<~SQL
                ^^^^^^ Use `<<~SQL.squish` instead of `<<~SQL`.
                  SELECT * FROM posts
                    WHERE id = 1
                SQL
            "#},
            indoc! {r#"
                <<~SQL.squish
                  SELECT * FROM posts
                    WHERE id = 1
                SQL
            "#},
        );
    }

    #[test]
    fn flags_singleline_heredoc() {
        test::<SquishedSQLHeredocs>().expect_correction(
            indoc! {r#"
                <<-SQL
                ^^^^^^ Use `<<-SQL.squish` instead of `<<-SQL`.
                  SELECT * FROM posts;
                SQL
            "#},
            indoc! {r#"
                <<-SQL.squish
                  SELECT * FROM posts;
                SQL
            "#},
        );
    }

    #[test]
    fn flags_heredoc_argument() {
        test::<SquishedSQLHeredocs>().expect_correction(
            indoc! {r#"
                execute(<<~SQL, "Post Load")
                        ^^^^^^ Use `<<~SQL.squish` instead of `<<~SQL`.
                  SELECT * FROM posts
                    WHERE post_id = 1
                SQL
            "#},
            indoc! {r#"
                execute(<<~SQL.squish, "Post Load")
                  SELECT * FROM posts
                    WHERE post_id = 1
                SQL
            "#},
        );
    }

    #[test]
    fn allows_squished_heredoc() {
        test::<SquishedSQLHeredocs>().expect_no_offenses(indoc! {r#"
            <<-SQL.squish
              SELECT * FROM posts;
            SQL
        "#});
    }

    #[test]
    fn allows_comment_heredoc() {
        test::<SquishedSQLHeredocs>().expect_no_offenses(indoc! {r#"
            <<-SQL
              SELECT * FROM posts
                -- This is a comment, so squish can't be used
                WHERE id = 1
            SQL
        "#});
    }

    #[test]
    fn allows_comment_like_strings_and_identifiers() {
        test::<SquishedSQLHeredocs>().expect_correction(
            indoc! {r#"
                <<~SQL
                ^^^^^^ Use `<<~SQL.squish` instead of `<<~SQL`.
                  SELECT * FROM posts
                    WHERE [--another identifier!] = 1
                    AND "-- this is a name, not a comment" = '-- this is a string, not a comment'
                SQL
            "#},
            indoc! {r#"
                <<~SQL.squish
                  SELECT * FROM posts
                    WHERE [--another identifier!] = 1
                    AND "-- this is a name, not a comment" = '-- this is a string, not a comment'
                SQL
            "#},
        );
    }

    #[test]
    fn allows_non_sql_heredoc() {
        test::<SquishedSQLHeredocs>().expect_no_offenses(indoc! {r#"
            <<~EOS
              some text
            EOS
        "#});
    }
}
murphy_plugin_api::submit_cop!(SquishedSQLHeredocs);
