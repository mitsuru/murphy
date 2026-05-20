use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
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
            let body = std::str::from_utf8(literal.body).expect("ASCII-only literal body is UTF-8");
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
        for source in ["x = \"a\\\\b\"\n", "x = \"a\\\"b\"\n"] {
            let offenses = run_single_cop(Box::new(StringLiterals), source);
            assert!(offenses.is_empty(), "{source:?}");
        }
    }
}
