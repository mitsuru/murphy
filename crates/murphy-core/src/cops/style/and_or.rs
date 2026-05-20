use crate::Offense;
use crate::Range;
use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};

pub struct AndOr;

impl Cop for AndOr {
    fn name(&self) -> &str {
        "Style/AndOr"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn on_if_node(
        &self,
        node: &ruby_prism::IfNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        inspect_condition(self.name(), node.predicate().location(), ctx, sink);
    }

    fn on_unless_node(
        &self,
        node: &ruby_prism::UnlessNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        inspect_condition(self.name(), node.predicate().location(), ctx, sink);
    }
}

fn inspect_condition(
    name: &str,
    predicate: ruby_prism::Location<'_>,
    ctx: &CopContext<'_>,
    sink: &mut Vec<Offense>,
) {
    let condition_range = Range::from_prism_location(&predicate);
    let Some(condition) = ctx
        .source
        .get(condition_range.start_offset as usize..condition_range.end_offset as usize)
    else {
        return;
    };

    if has_ambiguous_content(condition) {
        return;
    }
    if has_assignment(condition) {
        return;
    }
    if has_trailing_comment(ctx.source, condition_range.end_offset as usize) {
        return;
    }

    for operator in and_or_tokens(condition) {
        let start = condition_range.start_offset + operator.start as u32;
        let end = condition_range.start_offset + operator.end as u32;
        let (message, replacement) = if operator.word == b"and" {
            ("Use `&&` instead of `and`.", "&&")
        } else {
            ("Use `||` instead of `or`.", "||")
        };
        sink.push(offense_with_edit(
            ctx.file,
            name,
            Range {
                start_offset: start,
                end_offset: end,
            },
            message,
            replace_edit(start, end, replacement),
        ));
    }
}

struct Operator<'a> {
    start: usize,
    end: usize,
    word: &'a [u8],
}

fn and_or_tokens(condition: &[u8]) -> Vec<Operator<'_>> {
    let mut operators = Vec::new();
    let mut idx = 0;
    while idx < condition.len() {
        if let Some(word) = operator_at(condition, idx) {
            operators.push(Operator {
                start: idx,
                end: idx + word.len(),
                word,
            });
            idx += word.len();
        } else {
            idx += 1;
        }
    }
    operators
}

fn operator_at(condition: &[u8], idx: usize) -> Option<&'static [u8]> {
    for word in [b"and".as_slice(), b"or".as_slice()] {
        if condition[idx..].starts_with(word)
            && is_boundary_before(condition, idx)
            && is_boundary_after(condition, idx + word.len())
        {
            return Some(word);
        }
    }
    None
}

fn is_boundary_before(condition: &[u8], idx: usize) -> bool {
    condition
        .get(idx.wrapping_sub(1))
        .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn is_boundary_after(condition: &[u8], idx: usize) -> bool {
    condition
        .get(idx)
        .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_ambiguous_content(condition: &[u8]) -> bool {
    condition
        .iter()
        .any(|byte| matches!(*byte, b'#' | b'\'' | b'"' | b'`' | b'%' | b'/'))
}

fn has_assignment(condition: &[u8]) -> bool {
    let mut idx = 0;
    while idx < condition.len() {
        if condition[idx] != b'=' {
            idx += 1;
            continue;
        }

        if condition.get(idx + 1) == Some(&b'=') {
            idx += 2;
            continue;
        }
        if condition.get(idx + 1) == Some(&b'~') {
            idx += 1;
            continue;
        }

        if has_prev_non_whitespace(condition, idx, is_assignment_target_char)
            && condition
                .get(idx + 1..)
                .and_then(|slice| slice.iter().find(|byte| !byte.is_ascii_whitespace()))
                .is_some_and(|next| {
                    is_word_like(*next) || matches!(*next, b'(' | b'[' | b'.' | b'{' | b'"' | b'\'')
                })
        {
            return true;
        }

        if has_prev_non_whitespace(condition, idx, is_assignment_operator_char)
            || has_next_non_whitespace(condition, idx, is_word_like)
        {
            return true;
        }

        idx += 1;
    }

    false
}

fn has_prev_non_whitespace<F>(condition: &[u8], idx: usize, pred: F) -> bool
where
    F: Fn(u8) -> bool,
{
    condition[..idx]
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| pred(*byte))
}

fn has_next_non_whitespace<F>(condition: &[u8], idx: usize, pred: F) -> bool
where
    F: Fn(u8) -> bool,
{
    condition
        .get(idx + 1..)
        .and_then(|slice| slice.iter().find(|byte| !byte.is_ascii_whitespace()))
        .is_some_and(|byte| pred(*byte))
}

fn is_assignment_target_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b')' || byte == b']'
}

fn is_assignment_operator_char(byte: u8) -> bool {
    matches!(
        byte,
        b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'<' | b'>'
    )
}

fn is_word_like(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_trailing_comment(source: &[u8], start: usize) -> bool {
    let Some(rest) = source.get(start..) else {
        return false;
    };
    let line_end = rest
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(rest.len());
    rest[..line_end].contains(&b'#')
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::style::AndOr;
    use crate::cops::support::run_single_cop;

    #[test]
    fn autocorrects_and_in_if_condition() {
        let source = "if a and b\nend\n";
        let offenses = run_single_cop(Box::new(AndOr), source);

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use `&&` instead of `and`.");
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 5);
        assert_eq!(edit.range.end_offset, 8);
        assert_eq!(edit.replacement, "&&");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "if a && b\nend\n"
        );
    }

    #[test]
    fn autocorrects_or_in_if_condition() {
        let source = "if a or b\nend\n";
        let offenses = run_single_cop(Box::new(AndOr), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 5);
        assert_eq!(edit.range.end_offset, 7);
        assert_eq!(edit.replacement, "||");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "if a || b\nend\n"
        );
    }

    #[test]
    fn non_conditional_and_remains_clean() {
        let offenses = run_single_cop(Box::new(AndOr), "foo and return\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn identifiers_containing_operator_words_remain_clean() {
        for source in ["if candy_andy\nend\n", "if order\nend\n"] {
            let offenses = run_single_cop(Box::new(AndOr), source);

            assert!(offenses.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn commented_if_condition_remains_clean() {
        let offenses = run_single_cop(Box::new(AndOr), "if a and b # comment\nend\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn string_literal_in_if_condition_remains_clean() {
        let offenses = run_single_cop(Box::new(AndOr), "if \"a and b\"\nend\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn assignment_in_if_condition_remains_clean() {
        for source in ["if x = foo or bar\nend\n", "if foo ||= bar or baz\nend\n"] {
            let offenses = run_single_cop(Box::new(AndOr), source);

            assert!(offenses.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn autocorrects_or_in_unless_condition() {
        let source = "unless a or b\nend\n";
        let offenses = run_single_cop(Box::new(AndOr), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 9);
        assert_eq!(edit.range.end_offset, 11);
        assert_eq!(edit.replacement, "||");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "unless a || b\nend\n"
        );
    }

    #[test]
    fn commented_unless_condition_remains_clean() {
        let offenses = run_single_cop(Box::new(AndOr), "unless a or b # comment\nend\n");

        assert!(offenses.is_empty());
    }
}
