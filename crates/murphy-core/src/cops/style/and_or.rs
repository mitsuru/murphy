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
        let condition_range = Range::from_prism_location(&node.predicate().location());
        let Some(condition) = ctx
            .source
            .get(condition_range.start_offset as usize..condition_range.end_offset as usize)
        else {
            return;
        };
        if has_ambiguous_content(condition) {
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
                self.name(),
                Range {
                    start_offset: start,
                    end_offset: end,
                },
                message,
                replace_edit(start, end, replacement),
            ));
        }
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
}
