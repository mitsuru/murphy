use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct TrailingWhitespace;

impl Cop for TrailingWhitespace {
    fn name(&self) -> &str {
        "Layout/TrailingWhitespace"
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
        for (idx, byte) in ctx.source.iter().enumerate() {
            if *byte == b'\n' {
                inspect_line(ctx, sink, line_start, idx);
                line_start = idx + 1;
            }
        }

        if line_start < ctx.source.len() {
            inspect_line(ctx, sink, line_start, ctx.source.len());
        }
    }
}

fn inspect_line(ctx: &CopContext<'_>, sink: &mut Vec<Offense>, line_start: usize, line_end: usize) {
    let mut trim_start = line_end;
    while trim_start > line_start && matches!(ctx.source[trim_start - 1], b' ' | b'\t') {
        trim_start -= 1;
    }

    if trim_start == line_end {
        return;
    }

    let range = Range {
        start_offset: u32::try_from(trim_start).expect("source offset fits in u32"),
        end_offset: u32::try_from(line_end).expect("source offset fits in u32"),
    };
    sink.push(offense_with_edit(
        ctx.file,
        "Layout/TrailingWhitespace",
        range,
        "Remove trailing whitespace.",
        replace_edit(range.start_offset, range.end_offset, ""),
    ));
}

#[cfg(test)]
mod tests {
    use crate::cops::layout::TrailingWhitespace;
    use crate::cops::support::run_single_cop;

    #[test]
    fn flags_and_deletes_trailing_spaces_before_newline() {
        let offenses = run_single_cop(Box::new(TrailingWhitespace), "x = 1  \n");

        assert_eq!(offenses.len(), 1);
        let offense = &offenses[0];
        assert_eq!(offense.cop_name, "Layout/TrailingWhitespace");
        assert_eq!(offense.range.start_offset, 5);
        assert_eq!(offense.range.end_offset, 7);
        let autocorrect = offense.autocorrect.as_ref().expect("autocorrect");
        assert_eq!(autocorrect.edits.len(), 1);
        assert_eq!(autocorrect.edits[0].range, offense.range);
        assert_eq!(autocorrect.edits[0].replacement, "");
    }
}
