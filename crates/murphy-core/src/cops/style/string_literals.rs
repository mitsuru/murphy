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
                if let Some(end) = slash_literal_end(source, idx) {
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
            _ => idx += 1,
        }
    }
    literals
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
