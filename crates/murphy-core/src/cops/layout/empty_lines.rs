use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct EmptyLines;

impl Cop for EmptyLines {
    fn name(&self) -> &str {
        "Layout/EmptyLines"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let mut line_start = 0usize;
        let mut blank_run_len = 0usize;

        for (idx, byte) in ctx.source.iter().enumerate() {
            if *byte == b'\n' {
                inspect_line(ctx, sink, line_start, idx + 1, idx, &mut blank_run_len);
                line_start = idx + 1;
            }
        }

        if line_start < ctx.source.len() {
            inspect_line(
                ctx,
                sink,
                line_start,
                ctx.source.len(),
                ctx.source.len(),
                &mut blank_run_len,
            );
        }
    }
}

fn inspect_line(
    ctx: &CopContext<'_>,
    sink: &mut Vec<Offense>,
    line_start: usize,
    line_end: usize,
    content_end: usize,
    blank_run_len: &mut usize,
) {
    let is_blank = ctx.source[line_start..content_end]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t'));
    if !is_blank {
        *blank_run_len = 0;
        return;
    }

    *blank_run_len += 1;
    if *blank_run_len <= 1 {
        return;
    }

    let range = Range {
        start_offset: u32::try_from(line_start).expect("source offset fits in u32"),
        end_offset: u32::try_from(line_end).expect("source offset fits in u32"),
    };
    sink.push(offense_with_edit(
        ctx.file,
        "Layout/EmptyLines",
        range,
        "Extra blank line detected.",
        replace_edit(range.start_offset, range.end_offset, ""),
    ));
}

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::layout::EmptyLines;
    use crate::cops::support::run_single_cop;

    #[test]
    fn deletes_extra_blank_line_from_blank_line_run() {
        let source = "class A\n\n\n  def x\n  end\nend\n";
        let offenses = run_single_cop(Box::new(EmptyLines), source);

        assert_eq!(offenses.len(), 1);
        let offense = &offenses[0];
        assert_eq!(offense.cop_name, "Layout/EmptyLines");
        assert_eq!(offense.range.start_offset, 9);
        assert_eq!(offense.range.end_offset, 10);
        let edits = &offense.autocorrect.as_ref().expect("autocorrect").edits;
        assert_eq!(
            apply_edits(source, edits),
            "class A\n\n  def x\n  end\nend\n"
        );
    }
}
