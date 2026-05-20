use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct FrozenStringLiteralComment;

impl Cop for FrozenStringLiteralComment {
    fn name(&self) -> &str {
        "Style/FrozenStringLiteralComment"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        if has_frozen_string_literal_comment(ctx.source) {
            return;
        }

        let insert_at = shebang_end(ctx.source);
        let range = Range {
            start_offset: insert_at as u32,
            end_offset: insert_at as u32,
        };
        sink.push(offense_with_edit(
            ctx.file,
            self.name(),
            range,
            "Missing frozen string literal comment.",
            replace_edit(
                range.start_offset,
                range.end_offset,
                "# frozen_string_literal: true\n\n",
            ),
        ));
    }
}

fn shebang_end(source: &[u8]) -> usize {
    if !source.starts_with(b"#!") {
        return 0;
    }
    source
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(source.len(), |idx| idx + 1)
}

fn has_frozen_string_literal_comment(source: &[u8]) -> bool {
    let mut pos = shebang_end(source);
    while pos < source.len() {
        let line_end = source[pos..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(source.len(), |idx| pos + idx);
        let line = trim_ascii(&source[pos..line_end]);
        if line.is_empty() {
            pos = (line_end + 1).min(source.len());
            continue;
        }
        if !line.starts_with(b"#") {
            return false;
        }
        if line == b"# frozen_string_literal: true" || line == b"# frozen_string_literal: false" {
            return true;
        }
        pos = (line_end + 1).min(source.len());
    }
    false
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

#[cfg(test)]
mod tests {
    use crate::apply_edits;
    use crate::cops::style::FrozenStringLiteralComment;
    use crate::cops::support::run_single_cop;

    #[test]
    fn inserts_magic_comment_at_start_of_file() {
        let source = "puts 'x'\n";
        let offenses = run_single_cop(Box::new(FrozenStringLiteralComment), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 0);
        assert_eq!(edit.range.end_offset, 0);
        assert_eq!(edit.replacement, "# frozen_string_literal: true\n\n");
        assert_eq!(
            apply_edits(source, std::slice::from_ref(edit)),
            "# frozen_string_literal: true\n\nputs 'x'\n"
        );
    }

    #[test]
    fn inserts_after_shebang() {
        let source = "#!/usr/bin/env ruby\nputs 'x'\n";
        let offenses = run_single_cop(Box::new(FrozenStringLiteralComment), source);

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 20);
        assert_eq!(edit.range.end_offset, 20);
        assert_eq!(edit.replacement, "# frozen_string_literal: true\n\n");
    }

    #[test]
    fn existing_true_or_false_magic_comment_is_clean() {
        for source in [
            "# frozen_string_literal: true\nputs 'x'\n",
            "# frozen_string_literal: false\nputs 'x'\n",
            "#!/usr/bin/env ruby\n# frozen_string_literal: true\nputs 'x'\n",
        ] {
            let offenses = run_single_cop(Box::new(FrozenStringLiteralComment), source);
            assert!(offenses.is_empty(), "{source:?}");
        }
    }
}
