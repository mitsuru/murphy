use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
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
        for (idx, byte) in ctx.source.iter().enumerate() {
            match *byte {
                b'(' => open_parens.push(idx),
                b')' => {
                    if let Some(open) = open_parens.pop() {
                        inspect_paren_pair(ctx, sink, open, idx);
                    }
                }
                b'\n' => open_parens.clear(),
                _ => {}
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
}
