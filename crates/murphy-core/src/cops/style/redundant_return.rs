use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};
use ruby_prism::Visit;

pub struct RedundantReturn;

impl Cop for RedundantReturn {
    fn name(&self) -> &str {
        "Style/RedundantReturn"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let parsed = ruby_prism::parse(ctx.source);
        let comments: Vec<Range> = parsed
            .comments()
            .map(|comment| Range::from_prism_location(&comment.location()))
            .collect();
        let mut visitor = RedundantReturnVisitor {
            cop: self,
            ctx,
            comments,
            sink,
        };
        visitor.visit(&parsed.node());
    }
}

struct RedundantReturnVisitor<'a, 'sink> {
    cop: &'a RedundantReturn,
    ctx: &'a CopContext<'a>,
    comments: Vec<Range>,
    sink: &'sink mut Vec<Offense>,
}

impl<'pr> Visit<'pr> for RedundantReturnVisitor<'_, '_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.inspect_def(node);
        ruby_prism::visit_def_node(self, node);
    }
}

impl RedundantReturnVisitor<'_, '_> {
    fn inspect_def(&mut self, node: &ruby_prism::DefNode<'_>) {
        let Some(body) = node.body() else {
            return;
        };
        let Some(statements) = body.as_statements_node() else {
            return;
        };
        let Some(last) = statements.body().last() else {
            return;
        };
        let Some(return_node) = last.as_return_node() else {
            return;
        };
        let Some(edit) = redundant_return_edit(self.ctx.source, &return_node, &self.comments)
        else {
            return;
        };

        let range = Range::from_prism_location(&return_node.keyword_loc());
        self.sink.push(offense_with_edit(
            self.ctx.file,
            self.cop.name(),
            range,
            "Redundant `return` detected.",
            edit,
        ));
    }
}

fn redundant_return_edit(
    source: &[u8],
    node: &ruby_prism::ReturnNode<'_>,
    comments: &[Range],
) -> Option<crate::Edit> {
    let return_range = Range::from_prism_location(&node.location());
    if range_contains_comment(source, return_range, comments) {
        return None;
    }

    let arguments = node.arguments()?;
    if arguments.arguments().len() != 1 {
        return None;
    }
    let expression = arguments.arguments().first()?;
    let keyword = Range::from_prism_location(&node.keyword_loc());
    let expression_range = Range::from_prism_location(&expression.location());
    let gap = source.get(keyword.end_offset as usize..expression_range.start_offset as usize)?;
    if gap.is_empty() || !gap.iter().all(u8::is_ascii_whitespace) {
        return None;
    }

    Some(replace_edit(
        keyword.start_offset,
        expression_range.start_offset,
        "",
    ))
}

fn range_contains_comment(source: &[u8], range: Range, comments: &[Range]) -> bool {
    source
        .get(range.start_offset as usize..range.end_offset as usize)
        .is_none_or(|bytes| bytes.contains(&b'#'))
        || comments
            .iter()
            .any(|comment| ranges_overlap(range, *comment))
}

fn ranges_overlap(left: Range, right: Range) -> bool {
    left.start_offset < right.end_offset && right.start_offset < left.end_offset
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::style::RedundantReturn;
    use crate::cops::support::run_single_cop;

    #[test]
    fn autocorrects_final_unconditional_return_in_method_body() {
        let source = "def x\n  return 1\nend\n";
        let offenses = run_single_cop(Box::new(RedundantReturn), source);

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Redundant `return` detected.");
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "def x\n  1\nend\n"
        );
    }

    #[test]
    fn modifier_return_remains_clean_in_v1() {
        let offenses = run_single_cop(
            Box::new(RedundantReturn),
            "def x\n  return 1 if cond\nend\n",
        );

        assert!(offenses.is_empty());
    }

    #[test]
    fn return_outside_method_body_remains_clean() {
        let offenses = run_single_cop(Box::new(RedundantReturn), "return 1\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn multi_value_return_remains_clean() {
        let offenses = run_single_cop(Box::new(RedundantReturn), "def x\n  return 1, 2\nend\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn commented_return_range_remains_clean() {
        let offenses = run_single_cop(
            Box::new(RedundantReturn),
            "def x\n  return # keep\n    1\nend\n",
        );

        assert!(offenses.is_empty());
    }
}
