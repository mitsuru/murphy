//! `Lint/RegexpAsCondition` — avoids regexp literals as implicit `$_` conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RegexpAsCondition
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port covers regexp literals used directly as `if`, `unless`,
//!   `while`, and `until` conditions and autocorrects them to `/re/ =~ $_`.
//!   Murphy does not expose RuboCop's `match-current-line` hook as a dispatched
//!   AST node, so this cop uses a conservative raw-source scan for simple
//!   line-leading conditions. Negated/modifier/nested conditions and
//!   ignored-node handling are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, Range};

const MSG: &str =
    "Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.";

#[derive(Default)]
pub struct RegexpAsCondition;

#[cop(
    name = "Lint/RegexpAsCondition",
    description = "Checks regexp literals used as implicit current-line conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RegexpAsCondition {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let mut offset = 0usize;
        for line in cx.source().split_inclusive('\n') {
            check_line(line, offset, cx);
            offset += line.len();
        }
    }
}

fn check_line(line: &str, line_offset: usize, cx: &Cx<'_>) {
    let leading = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let Some(after_keyword) = condition_tail(trimmed) else {
        return;
    };
    let tail_leading = after_keyword.len() - after_keyword.trim_start().len();
    let tail = after_keyword.trim_start();
    if !tail.starts_with('/') {
        return;
    }
    let Some(end) = regexp_literal_end(tail) else {
        return;
    };
    if tail[end..].trim_start().starts_with("=~") {
        return;
    }

    let start = line_offset + leading + (trimmed.len() - after_keyword.len()) + tail_leading;
    let range = Range {
        start: start as u32,
        end: (start + end) as u32,
    };
    let source = cx.raw_source(range);
    let replacement = format!("{source} =~ $_");
    cx.emit_offense(range, MSG, None);
    cx.emit_edit(range, &replacement);
}

fn condition_tail(trimmed: &str) -> Option<&str> {
    ["if", "unless", "while", "until"]
        .iter()
        .find_map(|keyword| {
            trimmed
                .strip_prefix(keyword)
                .filter(|tail| tail.starts_with(char::is_whitespace))
        })
}

fn regexp_literal_end(source: &str) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in source.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '/' {
            return Some(idx + ch.len_utf8());
        }
    }
    None
}

murphy_plugin_api::submit_cop!(RegexpAsCondition);

#[cfg(test)]
mod tests {
    use super::RegexpAsCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_regexp_if_condition() {
        test::<RegexpAsCondition>().expect_correction(
            indoc! {r#"
                if /foo/
                   ^^^^^ Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.
                  work
                end
            "#},
            "if /foo/ =~ $_\n  work\nend\n",
        );
    }

    #[test]
    fn flags_regexp_while_condition() {
        test::<RegexpAsCondition>().expect_offense(indoc! {r#"
            while /foo/
                  ^^^^^ Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.
              work
            end
        "#});
    }

    #[test]
    fn accepts_explicit_match() {
        test::<RegexpAsCondition>().expect_no_offenses("if /foo/ =~ line\n  work\nend\n");
    }
}
