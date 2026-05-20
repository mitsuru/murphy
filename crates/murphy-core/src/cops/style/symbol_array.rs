use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct SymbolArray;

impl Cop for SymbolArray {
    fn name(&self) -> &str {
        "Style/SymbolArray"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        for array in simple_symbol_arrays(ctx.source) {
            let range = Range {
                start_offset: array.start as u32,
                end_offset: array.end as u32,
            };
            sink.push(offense_with_edit(
                ctx.file,
                self.name(),
                range,
                "Use %i for arrays of symbols.",
                replace_edit(
                    range.start_offset,
                    range.end_offset,
                    &format!("%i[{}]", array.items.join(" ")),
                ),
            ));
        }
    }
}

struct SymbolArrayCandidate {
    start: usize,
    end: usize,
    items: Vec<String>,
}

fn simple_symbol_arrays(source: &[u8]) -> Vec<SymbolArrayCandidate> {
    simple_array_bodies(source)
        .into_iter()
        .filter_map(|(start, end, body)| {
            let items = parse_symbol_items(body)?;
            Some(SymbolArrayCandidate { start, end, items })
        })
        .collect()
}

fn parse_symbol_items(body: &[u8]) -> Option<Vec<String>> {
    let mut items = Vec::new();
    for raw in body.split(|byte| *byte == b',') {
        let item = trim_ascii(raw);
        if !item.starts_with(b":") {
            return None;
        }
        let name = &item[1..];
        if name.is_empty()
            || !name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return None;
        }
        items.push(std::str::from_utf8(name).ok()?.to_string());
    }
    (items.len() >= 2).then_some(items)
}

fn simple_array_bodies(source: &[u8]) -> Vec<(usize, usize, &[u8])> {
    let mut arrays = Vec::new();
    let mut idx = 0;
    while idx < source.len() {
        match source[idx] {
            b'#' => idx = skip_until_newline(source, idx),
            b'\'' | b'"' => idx = skip_quoted(source, idx, source[idx]),
            b'[' => {
                if let Some(end) = source[idx + 1..].iter().position(|byte| *byte == b']') {
                    let end = idx + 1 + end;
                    let body = &source[idx + 1..end];
                    if !is_receiver_like_bracket(source, idx)
                        && !body.contains(&b'\n')
                        && !body.contains(&b'#')
                        && !body.contains(&b'[')
                        && !body.contains(&b']')
                    {
                        arrays.push((idx, end + 1, body));
                    }
                    idx = end + 1;
                } else {
                    idx += 1;
                }
            }
            _ => idx += 1,
        }
    }
    arrays
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
    use crate::cops::style::SymbolArray;
    use crate::cops::support::run_single_cop;

    #[test]
    fn autocorrects_simple_symbol_arrays() {
        let source = "x = [:foo, :bar]\n";
        let offenses = run_single_cop(Box::new(SymbolArray), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.replacement, "%i[foo bar]");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "x = %i[foo bar]\n"
        );
    }

    #[test]
    fn unsafe_or_non_static_arrays_remain_clean() {
        for source in [
            "x = [:foo, bar]\n",
            "x = [:foo, :'bar baz']\n",
            "x = [:foo, # c\n:bar]\n",
            "obj[:foo, :bar]\n",
        ] {
            let offenses = run_single_cop(Box::new(SymbolArray), source);
            assert!(offenses.is_empty(), "{source:?}");
        }
    }
}
