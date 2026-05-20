//! Shared support for native cop implementations.

#![allow(dead_code)]

pub(crate) fn line_ranges(source: &[u8]) -> Vec<crate::Range> {
    let mut ranges = Vec::new();
    let mut start = 0usize;

    for (idx, byte) in source.iter().enumerate() {
        if *byte == b'\n' {
            ranges.push(byte_range(start, idx + 1));
            start = idx + 1;
        }
    }

    if start < source.len() {
        ranges.push(byte_range(start, source.len()));
    }

    ranges
}

pub(crate) fn replace_edit(start: u32, end: u32, replacement: &str) -> crate::Edit {
    crate::Edit {
        range: crate::Range {
            start_offset: start,
            end_offset: end,
        },
        replacement: replacement.into(),
    }
}

pub(crate) fn offense_with_edit(
    file: &str,
    cop_name: &str,
    range: crate::Range,
    message: &str,
    edit: crate::Edit,
) -> crate::Offense {
    crate::Offense::new(file, cop_name, range, crate::Severity::Warning, message)
        .with_autocorrect(crate::Autocorrect { edits: vec![edit] })
}

pub(crate) fn simple_receiver_name(mut receiver: &[u8]) -> Option<&[u8]> {
    while receiver.len() >= 2 && receiver[0] == b'(' && receiver[receiver.len() - 1] == b')' {
        receiver = &receiver[1..receiver.len() - 1];
    }

    while receiver.starts_with(b"::") {
        receiver = &receiver[2..];
    }

    if receiver.is_empty()
        || !receiver
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }

    Some(receiver)
}

pub(crate) fn percent_literal_end(source: &[u8], start: usize) -> Option<usize> {
    if source.get(start) != Some(&b'%') {
        return None;
    }

    let mut delimiter_idx = start + 1;
    if source
        .get(delimiter_idx)
        .is_some_and(u8::is_ascii_alphabetic)
    {
        delimiter_idx += 1;
    }
    let delimiter = *source.get(delimiter_idx)?;
    if delimiter.is_ascii_whitespace() || delimiter.is_ascii_alphanumeric() {
        return None;
    }

    let closing = match delimiter {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    };
    let mut idx = delimiter_idx + 1;
    while idx < source.len() {
        match source[idx] {
            b'\\' => idx += 2,
            byte if byte == closing => return Some(idx + 1),
            _ => idx += 1,
        }
    }
    None
}

pub(crate) fn slash_literal_end(source: &[u8], start: usize) -> Option<usize> {
    if source.get(start) != Some(&b'/') {
        return None;
    }

    let mut idx = start + 1;
    while idx < source.len() {
        match source[idx] {
            b'\n' => return None,
            b'\\' => idx += 2,
            b'/' => return Some(idx + 1),
            _ => idx += 1,
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn run_single_cop(cop: Box<dyn crate::Cop>, source: &str) -> Vec<crate::Offense> {
    let ast = crate::parse(source).expect("parse source");
    let mut sink = Vec::new();
    let cops = vec![cop];
    crate::run_cops(&ast, "test.rb", &cops, &mut sink);
    crate::aggregate(sink)
}

fn byte_range(start: usize, end: usize) -> crate::Range {
    crate::Range {
        start_offset: u32::try_from(start).expect("source offset fits in u32"),
        end_offset: u32::try_from(end).expect("source offset fits in u32"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ranges_are_byte_offsets_for_multibyte_source() {
        let ranges = line_ranges("# 日本語\nputs 1\n".as_bytes());
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_offset, 0);
        assert_eq!(ranges[0].end_offset, "# 日本語\n".len() as u32);
    }

    #[test]
    fn replace_edit_uses_byte_range() {
        let edit = replace_edit(2, 5, "x");
        assert_eq!(edit.range.start_offset, 2);
        assert_eq!(edit.range.end_offset, 5);
        assert_eq!(edit.replacement, "x");
    }

    #[test]
    fn offense_with_edit_defaults_to_warning_and_attaches_autocorrect() {
        let range = crate::Range {
            start_offset: 0,
            end_offset: 4,
        };
        let edit = replace_edit(0, 4, "warn");

        let offense = offense_with_edit("test.rb", "Test/Cop", range, "message", edit.clone());

        assert_eq!(offense.file, "test.rb");
        assert_eq!(offense.cop_name, "Test/Cop");
        assert_eq!(offense.range, range);
        assert_eq!(offense.severity, crate::Severity::Warning);
        assert_eq!(offense.message, "message");
        assert_eq!(offense.autocorrect.unwrap().edits, vec![edit]);
    }

    #[test]
    fn run_single_cop_returns_aggregated_offenses() {
        struct StubCop;

        impl crate::Cop for StubCop {
            fn name(&self) -> &str {
                "Test/Stub"
            }

            fn on_call_node(
                &self,
                node: &ruby_prism::CallNode<'_>,
                ctx: &crate::CopContext<'_>,
                sink: &mut Vec<crate::Offense>,
            ) {
                let Some(loc) = node.message_loc() else {
                    return;
                };
                sink.push(crate::Offense::new(
                    ctx.file,
                    self.name(),
                    crate::Range::from_prism_location(&loc),
                    crate::Severity::Warning,
                    "stub",
                ));
            }
        }

        let offenses = run_single_cop(Box::new(StubCop), "bar; foo\n");

        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].range.start_offset, 0);
        assert_eq!(offenses[1].range.start_offset, 5);
    }
}
