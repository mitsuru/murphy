use crate::cop::{Cop, CopContext};
use crate::cops::support::{
    offense_with_edit, percent_literal_end, replace_edit, slash_literal_end,
};
use crate::{Offense, Range};

pub struct SpaceInsideParens;

impl Cop for SpaceInsideParens {
    fn name(&self) -> &str {
        "Layout/SpaceInsideParens"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let mut open_parens = Vec::new();
        let mut idx = 0;
        while idx < ctx.source.len() {
            match ctx.source[idx] {
                b'(' => {
                    open_parens.push(idx);
                    idx += 1;
                }
                b')' => {
                    if let Some(open) = open_parens.pop() {
                        inspect_paren_pair(ctx, sink, open, idx);
                    }
                    idx += 1;
                }
                b'\n' => {
                    open_parens.clear();
                    idx += 1;
                }
                b'#' => idx = skip_until_newline(ctx.source, idx),
                b'\'' => idx = skip_quoted(ctx.source, idx, b'\''),
                b'"' => idx = skip_quoted(ctx.source, idx, b'"'),
                b'`' => idx = skip_quoted(ctx.source, idx, b'`'),
                b'/' => {
                    if let Some(end) = slash_literal_end(ctx.source, idx) {
                        idx = end;
                    } else {
                        idx += 1;
                    }
                }
                b'%' => {
                    if let Some(end) = percent_literal_end(ctx.source, idx) {
                        idx = end;
                    } else {
                        idx += 1;
                    }
                }
                b'<' => {
                    if let Some(end) = skip_heredoc(ctx.source, idx) {
                        idx = end;
                    } else {
                        idx += 1;
                    }
                }
                _ => idx += 1,
            }
        }
    }
}

fn inspect_paren_pair(ctx: &CopContext<'_>, sink: &mut Vec<Offense>, open: usize, close: usize) {
    if !ctx.source[open + 1..close]
        .iter()
        .any(|byte| !matches!(byte, b' ' | b'\t'))
    {
        return;
    }

    let after_open_start = open + 1;
    let mut after_open_end = after_open_start;
    while after_open_end < close && matches!(ctx.source[after_open_end], b' ' | b'\t') {
        after_open_end += 1;
    }
    if after_open_end > after_open_start {
        push_delete(ctx, sink, after_open_start, after_open_end);
    }

    let before_close_end = close;
    let mut before_close_start = before_close_end;
    while before_close_start > open + 1
        && matches!(ctx.source[before_close_start - 1], b' ' | b'\t')
    {
        before_close_start -= 1;
    }
    if before_close_start < before_close_end {
        push_delete(ctx, sink, before_close_start, before_close_end);
    }
}

fn skip_until_newline(source: &[u8], start: usize) -> usize {
    source[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(source.len(), |offset| start + offset + 1)
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

fn skip_heredoc(source: &[u8], start: usize) -> Option<usize> {
    let (delim, strip_indent, mut idx) = heredoc_delimiter(source, start)?;
    if idx >= source.len() {
        return None;
    }

    while idx < source.len() {
        let line_end = idx
            + source[idx..]
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(source.len() - idx);

        let line = &source[idx..line_end];
        if is_heredoc_terminator(line, &delim, strip_indent) {
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

fn previous_significant_byte(source: &[u8], before: usize) -> Option<u8> {
    source[..before]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
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
    let delim = if source.get(idx) == Some(&b'\'') || source.get(idx) == Some(&b'"') {
        let quote = source[idx];
        let end = skip_quoted(source, idx, quote);
        if end <= idx + 1 {
            return None;
        }
        idx = end;
        source[delimiter_start + 1..end - 1].to_vec()
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

    Some((delim, strip_indent, idx + 1))
}

fn heredoc_prefix_allows(source: &[u8], start: usize) -> bool {
    let Some(prev) = previous_significant_byte(source, start) else {
        return true;
    };
    !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b')' && prev != b']' && prev != b'}'
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

    if delimiter.is_empty() {
        return false;
    }

    if line.len() < idx + delimiter.len() {
        return false;
    }

    if !line[idx..].starts_with(delimiter) {
        return false;
    }

    let terminator_end = idx + delimiter.len();
    line[terminator_end..]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

fn push_delete(ctx: &CopContext<'_>, sink: &mut Vec<Offense>, start: usize, end: usize) {
    let range = Range {
        start_offset: u32::try_from(start).expect("source offset fits in u32"),
        end_offset: u32::try_from(end).expect("source offset fits in u32"),
    };
    sink.push(offense_with_edit(
        ctx.file,
        "Layout/SpaceInsideParens",
        range,
        "Remove space inside parentheses.",
        replace_edit(range.start_offset, range.end_offset, ""),
    ));
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::layout::SpaceInsideParens;
    use crate::cops::support::run_single_cop;

    #[test]
    fn removes_spaces_inside_non_empty_parens() {
        let source = "foo( 1, 2 )\n";
        let offenses = run_single_cop(Box::new(SpaceInsideParens), source);
        let edits = offenses
            .iter()
            .flat_map(|offense| offense.autocorrect.as_ref().unwrap().edits.clone())
            .collect::<Vec<_>>();

        assert!(!offenses.is_empty());
        assert!(
            offenses
                .iter()
                .all(|offense| offense.cop_name == "Layout/SpaceInsideParens")
        );
        assert_eq!(apply_edits(source, &edits), "foo(1, 2)\n");
    }

    #[test]
    fn ignores_spaces_in_parentheses_inside_strings_and_comments() {
        let source = "x = \"( 1 )\" # (( 2 ))\nputs ( 3 )\n";
        let offenses = run_single_cop(Box::new(SpaceInsideParens), source);
        let edits = offenses
            .iter()
            .flat_map(|offense| offense.autocorrect.as_ref().unwrap().edits.clone())
            .collect::<Vec<_>>();

        assert_eq!(offenses.len(), 2);
        assert_eq!(
            apply_edits(source, &edits),
            "x = \"( 1 )\" # (( 2 ))\nputs (3)\n"
        );
    }

    #[test]
    fn ignores_spaces_in_parentheses_in_double_quoted_string_alone() {
        let source = "x = \"( 1 )\"\n";
        let offenses = run_single_cop(Box::new(SpaceInsideParens), source);

        assert!(offenses.is_empty());
    }

    #[test]
    fn ignores_spaces_in_parentheses_in_heredoc_body_for_suffixes() {
        let source = "puts(<<TEXT, 1)\n( 1 )\nTEXT\n";
        let offenses = run_single_cop(Box::new(SpaceInsideParens), source);

        assert!(offenses.is_empty());
    }
}
