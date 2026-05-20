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

        let Some(end_keyword_loc) = node.end_keyword_loc() else {
            return;
        };
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
            end_keyword_loc.start_offset() as u32,
            end_keyword_loc.end_offset() as u32,
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

        let Some(end_keyword_loc) = node.end_keyword_loc() else {
            return;
        };
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
            end_keyword_loc.start_offset() as u32,
            end_keyword_loc.end_offset() as u32,
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
    block_end_keyword_offset: u32,
    block_end_keyword_end_offset: u32,
    condition: &ruby_prism::Location<'_>,
    statement: &ruby_prism::Node<'_>,
    replacement_keyword: &str,
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

    let Some(block_range) = range_from_location(ctx.source, block) else {
        return;
    };
    let is_elsif = ctx
        .source
        .get(
            block_range.start_offset as usize
                ..std::cmp::min(block_range.start_offset as usize + 6, ctx.source.len()),
        )
        .is_some_and(|bytes| bytes.starts_with(b"elsif"));

    let block_range = if is_elsif {
        Range {
            start_offset: block_range.start_offset,
            end_offset: block_end_keyword_offset,
        }
    } else {
        Range {
            start_offset: block_range.start_offset,
            end_offset: block_end_keyword_end_offset,
        }
    };

    let mut replacement = format!(
        "{} {} {}",
        statement_text.trim(),
        replacement_keyword,
        condition_text.trim()
    );
    if is_elsif {
        let replacement_range = ctx
            .source
            .get(block_range.start_offset as usize..block_range.end_offset as usize);
        if replacement_range.is_some_and(|bytes| bytes.ends_with(b"\n")) {
            replacement.push('\n');
        }
    }

    if block_range.start_offset >= block_range.end_offset {
        return;
    };

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

    #[test]
    fn keeps_end_with_elsif_chain() {
        let source = "if ok\n  run\nelsif cond\n  run2\nelsif cond2\n  run3\nend\n";
        let offenses = run_single_cop(Box::new(IfUnlessModifier), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "if ok\n  run\nelsif cond\n  run2\nrun3 if cond2\nend\n"
        );
    }
}
