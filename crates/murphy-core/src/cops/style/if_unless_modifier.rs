use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct IfUnlessModifier;

impl Cop for IfUnlessModifier {
    fn name(&self) -> &str {
        "Style/IfUnlessModifier"
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
        if node.end_keyword_loc().is_none() {
            return;
        }

        if node.subsequent().is_some() {
            return;
        }

        let Some(statements) = node.statements() else {
            return;
        };
        let body = statements.body();
        if body.len() != 1 {
            return;
        }
        let Some(statement) = body.first() else {
            return;
        };

        detect_block_to_modifier(
            &node.location(),
            &node.predicate().location(),
            &statement,
            "if",
            "Use modifier `if` form.",
            ctx,
            sink,
        );
    }

    fn on_unless_node(
        &self,
        node: &ruby_prism::UnlessNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        if node.end_keyword_loc().is_none() {
            return;
        }

        if node.else_clause().is_some() {
            return;
        }

        let Some(statements) = node.statements() else {
            return;
        };
        let body = statements.body();
        if body.len() != 1 {
            return;
        }
        let Some(statement) = body.first() else {
            return;
        };

        detect_block_to_modifier(
            &node.location(),
            &node.predicate().location(),
            &statement,
            "unless",
            "Use modifier `unless` form.",
            ctx,
            sink,
        );
    }
}

fn detect_block_to_modifier(
    block: &ruby_prism::Location<'_>,
    condition: &ruby_prism::Location<'_>,
    statement: &ruby_prism::Node<'_>,
    keyword: &str,
    message: &str,
    ctx: &CopContext<'_>,
    sink: &mut Vec<Offense>,
) {
    let Some(condition_range) = range_from_location(ctx.source, condition) else {
        return;
    };

    if has_trailing_comment(ctx.source, condition_range.end_offset as usize) {
        return;
    }

    let Some(statement_range) = range_from_location(ctx.source, &statement.location()) else {
        return;
    };

    let statement_slice = statement_bytes(ctx.source, statement_range);
    if has_any_comment(statement_slice) {
        return;
    }

    let Some(statement_text) = std::str::from_utf8(statement_slice).ok() else {
        return;
    };
    let Some(condition_slice) = ctx
        .source
        .get(condition_range.start_offset as usize..condition_range.end_offset as usize)
    else {
        return;
    };
    let Some(condition_text) = std::str::from_utf8(condition_slice).ok() else {
        return;
    };

    if contains_newline(statement_text.as_bytes()) || contains_newline(condition_text.as_bytes()) {
        return;
    }

    let replacement = format!(
        "{} {} {}",
        statement_text.trim(),
        keyword,
        condition_text.trim()
    );
    let block_range = range_from_location(ctx.source, block).expect("block range already checked");

    sink.push(offense_with_edit(
        ctx.file,
        "Style/IfUnlessModifier",
        block_range,
        message,
        replace_edit(
            block_range.start_offset,
            block_range.end_offset,
            &replacement,
        ),
    ));
}

fn range_from_location(source: &[u8], location: &ruby_prism::Location<'_>) -> Option<Range> {
    let start_offset = location.start_offset() as u32;
    let end_offset = location.end_offset() as u32;
    source
        .get(start_offset as usize..end_offset as usize)
        .map(|_| Range {
            start_offset,
            end_offset,
        })
}

fn statement_bytes(source: &[u8], range: Range) -> &[u8] {
    &source[range.start_offset as usize..range.end_offset as usize]
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

fn has_any_comment(bytes: &[u8]) -> bool {
    bytes.contains(&b'#')
}

fn contains_newline(bytes: &[u8]) -> bool {
    bytes.contains(&b'\n')
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::style::IfUnlessModifier;
    use crate::cops::support::run_single_cop;

    #[test]
    fn autocorrects_simple_if_body_to_modifier_form() {
        let source = "if ok\n  run\nend\n";
        let offenses = run_single_cop(Box::new(IfUnlessModifier), source);

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use modifier `if` form.");
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "run if ok\n"
        );
    }

    #[test]
    fn autocorrects_simple_unless_body_to_modifier_form() {
        let source = "unless ok\n  run\nend\n";
        let offenses = run_single_cop(Box::new(IfUnlessModifier), source);

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use modifier `unless` form.");
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "run unless ok\n"
        );
    }

    #[test]
    fn commented_condition_remains_clean_in_v1() {
        let offenses = run_single_cop(Box::new(IfUnlessModifier), "if ok # comment\n  run\nend\n");

        assert!(offenses.is_empty());
    }
}
