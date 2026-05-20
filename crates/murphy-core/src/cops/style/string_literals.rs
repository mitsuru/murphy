use crate::cop::{Cop, CopContext};
use crate::cops::support::{
    offense_with_edit, percent_literal_end, replace_edit, slash_literal_end,
};
use crate::{Offense, Range};

pub struct StringLiterals;

impl Cop for StringLiterals {
    fn name(&self) -> &str {
        "Style/StringLiterals"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        for literal in simple_double_quoted_literals(ctx.source) {
            let range = Range {
                start_offset: literal.start as u32,
                end_offset: literal.end as u32,
            };
            let Ok(body) = std::str::from_utf8(literal.body) else {
                continue;
            };
            sink.push(offense_with_edit(
                ctx.file,
                self.name(),
                range,
                "Prefer single-quoted strings when interpolation is not needed.",
                replace_edit(range.start_offset, range.end_offset, &format!("'{body}'")),
            ));
        }
    }
}

struct Literal<'a> {
    start: usize,
    end: usize,
    body: &'a [u8],
}

fn simple_double_quoted_literals(source: &[u8]) -> Vec<Literal<'_>> {
    let mut literals = Vec::new();
    let mut idx = 0;
    while idx < source.len() {
        match source[idx] {
            b'#' => idx = skip_until_newline(source, idx),
            b'\'' => idx = skip_quoted(source, idx, b'\''),
            b'%' => {
                if let Some(end) = percent_literal_end(source, idx) {
                    idx = end;
                } else {
                    idx += 1;
                }
            }
            b'/' => {
                if is_regex_like(source, idx) && let Some(end) = slash_literal_end(source, idx) {
                    idx = end;
                } else {
                    idx += 1;
                }
            }
            b'[' => {
                if let Some(end) = simple_word_array_end(source, idx) {
                    idx = end;
                } else {
                    idx += 1;
                }
            }
            b'"' => {
                let Some(end) = closing_quote(source, idx) else {
                    idx = skip_quoted(source, idx, b'"');
                    continue;
                };
                let body = &source[idx + 1..end];
                if is_simple_single_quote_body(body) {
                    literals.push(Literal {
                        start: idx,
                        end: end + 1,
                        body,
                    });
                }
                idx = end + 1;
            }
            b'<' => {
                if let Some(end) = skip_heredoc(source, idx) {
                    idx = end;
                } else {
                    idx += 1;
                }
            }
            _ => idx += 1,
        }
    }
    literals
}

fn skip_heredoc(source: &[u8], start: usize) -> Option<usize> {
    let Some((delimiter, strip_indent, mut idx)) = heredoc_delimiter(source, start) else {
        return None;
    };

    while idx < source.len() {
        let line_end = idx
            + source[idx..]
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(source.len() - idx);
        let line = &source[idx..line_end];

        if is_heredoc_terminator(line, &delimiter, strip_indent) {
            return Some(if line_end < source.len() {
                line_end + 1
            } else {
                line_end
            });
        }

        if line_end >= source.len() {
            break;
        }

        idx = line_end + 1;
    }

    None
}

fn heredoc_delimiter(source: &[u8], start: usize) -> Option<(Vec<u8>, bool, usize)> {
    if source.get(start) != Some(&b'<') || source.get(start + 1) != Some(&b'<') {
        return None;
    }
    if !heredoc_prefix_allows(source, start) {
        return None;
    }

    let mut idx = start + 2;
    let mut strip_indent = false;

    if source.get(idx) == Some(&b'~') {
        strip_indent = true;
        idx += 1;
    } else if source.get(idx) == Some(&b'-') {
        idx += 1;
    }

    if source.get(idx) == Some(&b'~') {
        strip_indent = true;
        idx += 1;
    }

    let delimiter_start = idx;
    let delimiter = if source.get(idx) == Some(&b'\'') || source.get(idx) == Some(&b'"') {
        let quote = source[idx];
        let quote_end = skip_quoted(source, idx, quote);
        if quote_end <= idx + 1 {
            return None;
        }
        let text = &source[idx + 1..quote_end - 1];
        idx = quote_end;
        text.to_vec()
    } else {
        while idx < source.len() && (source[idx].is_ascii_alphanumeric() || source[idx] == b'_') {
            idx += 1;
        }
        if idx == delimiter_start {
            return None;
        }
        source[delimiter_start..idx].to_vec()
    };

    while idx < source.len() && source[idx].is_ascii_whitespace() && source[idx] != b'\n' {
        idx += 1;
    }
    if idx >= source.len() {
        return None;
    }
    if source[idx] != b'\n' {
        if !is_heredoc_suffix_allowed(source, idx) {
            return None;
        }
        idx = source[idx..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(source.len(), |offset| idx + offset);
        if idx >= source.len() {
            return None;
        }
    }

    if source[idx] != b'\n' {
        return None;
    }

    Some((delimiter, strip_indent, idx + 1))
}

fn heredoc_prefix_allows(source: &[u8], start: usize) -> bool {
    let Some(prev) = previous_significant_byte(source, start) else {
        return true;
    };
    !prev.is_ascii_alphanumeric()
        && prev != b'_'
        && prev != b')'
        && prev != b']'
        && prev != b'}'
}

fn is_heredoc_suffix_allowed(source: &[u8], idx: usize) -> bool {
    if idx >= source.len() {
        return false;
    }

    matches!(
        source[idx],
        b'.' | b',' | b')' | b']' | b'}' | b'#' | b'\n' | b'\r' | b'\t' | b' '
    )
}

fn is_heredoc_terminator(line: &[u8], delimiter: &[u8], strip_indent: bool) -> bool {
    let mut idx = 0;
    if strip_indent {
        while idx < line.len() && line[idx].is_ascii_whitespace() {
            idx += 1;
        }
    }

    if delimiter.is_empty() || line.len() < idx + delimiter.len() {
        return false;
    }

    if !line[idx..].starts_with(delimiter) {
        return false;
    }

    line[idx + delimiter.len()..]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

fn closing_quote(source: &[u8], start: usize) -> Option<usize> {
    let mut idx = start + 1;
    while idx < source.len() {
        match source[idx] {
            b'\\' => return None,
            b'"' => return Some(idx),
            _ => idx += 1,
        }
    }
    None
}

fn is_simple_single_quote_body(body: &[u8]) -> bool {
    !body.contains(&b'\'') && !body.windows(2).any(|w| w == b"#{") && !body.contains(&b'\\')
}

fn simple_word_array_end(source: &[u8], start: usize) -> Option<usize> {
    if is_receiver_like_bracket(source, start) {
        return None;
    }
    let end = source[start + 1..]
        .iter()
        .position(|byte| *byte == b']')
        .map(|offset| start + 1 + offset)?;
    let body = &source[start + 1..end];
    if body.contains(&b'\n')
        || body.contains(&b'#')
        || body.contains(&b'[')
        || body.contains(&b']')
        || parse_word_items(body).is_none()
    {
        return None;
    }
    Some(end + 1)
}

fn parse_word_items(body: &[u8]) -> Option<Vec<&[u8]>> {
    let mut items = Vec::new();
    for raw in body.split(|byte| *byte == b',') {
        let item = trim_ascii(raw);
        if item.len() < 2 || item[0] != b'"' || item[item.len() - 1] != b'"' {
            return None;
        }
        let word = &item[1..item.len() - 1];
        if word.is_empty()
            || word.iter().any(|byte| byte.is_ascii_whitespace())
            || word.contains(&b'\\')
            || word.windows(2).any(|w| w == b"#{")
            || !word.is_ascii()
        {
            return None;
        }
        items.push(word);
    }
    (items.len() >= 2).then_some(items)
}

fn is_receiver_like_bracket(source: &[u8], bracket: usize) -> bool {
    let Some(prev) = previous_significant_byte(source, bracket) else {
        return false;
    };
    prev.is_ascii_alphanumeric() || matches!(prev, b'_' | b')' | b']')
}

fn previous_significant_byte(source: &[u8], before: usize) -> Option<u8> {
    source[..before]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn is_regex_like(source: &[u8], idx: usize) -> bool {
    let Some(prev) = previous_significant_byte(source, idx) else {
        return true;
    };
    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b')' || prev == b']' || prev == b'}' {
        return false;
    }

    let Some(next) = source.get(idx + 1) else {
        return false;
    };

    !matches!(next, b'\n' | b'\r' | b';')
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn skip_quoted(source: &[u8], start: usize, quote: u8) -> usize {
    let mut idx = start + 1;
    while idx < source.len() {
        match source[idx] {
            b'\\' => idx += 2,
            byte if byte == quote => return idx + 1,
            _ => idx += 1,
        }
    }
    source.len()
}

fn skip_until_newline(source: &[u8], start: usize) -> usize {
    source[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(source.len(), |offset| start + offset + 1)
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cop::{Cop, CopContext};
    use crate::cops::style::StringLiterals;
    use crate::cops::support::run_single_cop;

    #[test]
    fn autocorrects_simple_double_quoted_string() {
        let source = "x = \"abc\"\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.replacement, "'abc'");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "x = 'abc'\n"
        );
    }

    #[test]
    fn interpolation_remains_clean() {
        let offenses = run_single_cop(Box::new(StringLiterals), "x = \"#{name}\"\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn unsafe_escaped_quotes_and_backslashes_remain_clean() {
        for source in [
            "x = \"a\\\\b\"\n",
            "x = \"a\\\"b\"\n",
            "x = %Q[\"foo\"]\n",
            "x = %Q[[\"foo\"] \"bar\"]\n",
            "x = /\"foo\"/\n",
        ] {
            let offenses = run_single_cop(Box::new(StringLiterals), source);
            assert!(offenses.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn division_operator_does_not_skip_scanned_string() {
        let source = "x = a / \"abc\" / b\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert_eq!(offenses.len(), 1, "{offenses:?}");
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.replacement, "'abc'");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "x = a / 'abc' / b\n"
        );
    }

    #[test]
    fn quoted_text_in_heredoc_body_remains_clean() {
        let source = "x = <<~TEXT\n\"abc\"\nTEXT\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn quoted_text_in_method_applied_heredoc_body_remains_clean() {
        let source = "x = <<~TEXT.strip\n\"abc\"\nTEXT\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn quoted_text_in_call_argument_heredoc_body_remains_clean() {
        let source = "puts(<<TEXT, 1)\n\"abc\"\nTEXT\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn quoted_text_in_dash_heredoc_body_remains_clean() {
        let source = "x = <<-TEXT\n'abc'\nTEXT\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn quoted_text_in_quoted_delimiter_heredoc_body_remains_clean() {
        let source = "x = <<\"TEXT\"\n\"abc\"\nTEXT\n";
        let offenses = run_single_cop(Box::new(StringLiterals), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn invalid_utf8_literal_body_is_skipped() {
        let mut sink = Vec::new();
        let ctx = CopContext {
            file: "invalid.rb",
            source: b"x = \"\xff\"\n",
        };

        StringLiterals.inspect_file(&ctx, &mut sink);

        assert!(sink.is_empty());
    }
}
