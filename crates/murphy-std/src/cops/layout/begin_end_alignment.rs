//! `Layout/BeginEndAlignment` — flags the `end` keyword of a `begin…end`
//! (`kwbegin`) block that is not aligned with the configured anchor.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/BeginEndAlignment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Port of RuboCop's `BeginEndAlignment` (which mixes in
//!   `EndKeywordAlignment`). Fires on `on_kwbegin` — the keyword `begin…end`
//!   form. Murphy's translator lowers `begin…end` to `NodeKind::Begin` (the
//!   same variant used for a parenthesised `( … )`); the keyword form is the
//!   one whose leading token text is `begin` and whose node ends at an `end`
//!   keyword, so the handler filters on those. The `align_with`
//!   map RuboCop builds is `{ begin: node.loc.begin, start_of_line:
//!   start_line_range(node) }` and `check_end_kw_alignment` checks the `end`
//!   against the range named by `EnforcedStyleAlignWith`:
//!
//!   - `start_of_line` (default): anchor on the trimmed text of the first line
//!     of the `begin` (RuboCop's `start_line_range`), whose column is the
//!     line's indentation and whose `source` is the full trimmed first line
//!     (e.g. `var << begin`).
//!   - `begin`: anchor on the `begin` keyword itself.
//!
//!   Acceptance (`matching_ranges`): the `end` is aligned when it is on the
//!   same line as the anchor OR shares the anchor's (0-based, char-counted)
//!   column. Otherwise an offense is reported on the `end` keyword with
//!   RuboCop's message ``\`end\` at L, C is not aligned with \`<src>\` at L,
//!   C.`` and an autocorrect re-indents the `end` line to the anchor column.
//!
//!   ABI note: `Kwbegin` is keyword-bearing, so `LocRef::keyword()` returns the
//!   `begin` token and `LocRef::end_keyword()` returns the `end` token.
//! ```
//!
//! ## Matched shapes
//!
//! `begin…end` (`kwbegin`) blocks whose `end` keyword is misaligned with the
//! configured anchor.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct BeginEndAlignment;

/// Options for [`BeginEndAlignment`]. `EnforcedStyleAlignWith` matches RuboCop
/// (`SupportedStylesAlignWith: [start_of_line, begin]`, default
/// `start_of_line`).
#[derive(CopOptions)]
pub struct BeginEndAlignmentOptions {
    #[option(
        name = "EnforcedStyleAlignWith",
        default = "start_of_line",
        description = "Whether `end` aligns with the start of the line where `begin` appears, or the `begin` keyword."
    )]
    pub enforced_style_align_with: AlignWith,
}

/// `SupportedStylesAlignWith: [start_of_line, begin]`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignWith {
    #[option(value = "start_of_line")]
    StartOfLine,
    #[option(value = "begin")]
    Begin,
}

#[cop(
    name = "Layout/BeginEndAlignment",
    description = "Align ends corresponding to begins correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = BeginEndAlignmentOptions
)]
impl BeginEndAlignment {
    #[on_node(kind = "kwbegin")]
    fn check_kwbegin(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// The `begin` keyword token of a keyword `begin…end` block, or `None` if
/// `node` is not the keyword form (e.g. a parenthesised `( … )` or a bare
/// statement sequence). Murphy lowers both `begin…end` and `( … )` to
/// `NodeKind::Begin`, so we additionally require the leading token to be the
/// literal `begin` keyword.
fn begin_keyword(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let kw = cx.loc(node).keyword();
    if kw == Range::ZERO {
        return None;
    }
    (cx.raw_source(kw) == "begin").then_some(kw)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(begin_kw) = begin_keyword(node, cx) else {
        return;
    };
    let end_kw = cx.loc(node).end_keyword();
    if end_kw == Range::ZERO {
        return;
    }

    let opts = cx.options_or_default::<BeginEndAlignmentOptions>();
    // The anchor range and the message `source` text differ by style.
    let (anchor, source) = match opts.enforced_style_align_with {
        AlignWith::Begin => (begin_kw, cx.raw_source(begin_kw).to_owned()),
        AlignWith::StartOfLine => {
            // RuboCop's `start_line_range`: the trimmed first line of the
            // `begin`. Anchor column is the line indentation; the message
            // source is the entire trimmed first line.
            let (range, text) = start_line_range(node, cx);
            (range, text)
        }
    };

    let (anchor_line, anchor_col) = line_and_column(cx, anchor.start);
    let (end_line, end_col) = line_and_column(cx, end_kw.start);

    // `matching_ranges`: aligned if on the same line OR same column.
    if anchor_line == end_line || anchor_col == end_col {
        return;
    }

    let message = format!(
        "`end` at {end_line}, {end_col} is not aligned with `{source}` at {anchor_line}, {anchor_col}."
    );
    cx.emit_offense(end_kw, &message, None);

    // Autocorrect: re-indent the `end` line to the anchor column. Only when
    // `end` is the first non-whitespace on its line (otherwise rewriting the
    // leading whitespace would corrupt inline code). Idempotent.
    if let Some(line_start) = line_start_if_end_leads(end_kw.start, cx) {
        let indent = " ".repeat(anchor_col);
        cx.emit_edit(
            Range {
                start: line_start,
                end: end_kw.start,
            },
            &indent,
        );
    }
}

/// RuboCop's `start_line_range(node)`: the range spanning the first non-blank
/// character of the `begin`'s first line to the start of that line's trailing
/// whitespace. Returns the range and its (trimmed) source text. The range's
/// start column is the line's indentation; the text is the full trimmed line.
fn start_line_range(node: NodeId, cx: &Cx<'_>) -> (Range, String) {
    let src = cx.source();
    let bytes = src.as_bytes();
    let node_start = cx.range(node).start as usize;
    let line_start = src[..node_start].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = bytes[line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(src.len(), |idx| line_start + idx);
    let line = &src[line_start..line_end];
    // First non-whitespace offset on the line.
    let leading_ws = line.len() - line.trim_start().len();
    let content_start = line_start + leading_ws;
    // End at the start of the trailing whitespace (RuboCop's `/\s*\z/`).
    let trimmed_len = line.trim_end().len();
    let content_end = line_start + trimmed_len;
    let range = Range {
        start: content_start as u32,
        end: content_end as u32,
    };
    (range, src[content_start..content_end].to_owned())
}

/// 1-based line and 0-based character column of `offset`.
fn line_and_column(cx: &Cx<'_>, offset: u32) -> (usize, usize) {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line = src[..upper].bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = src[..upper].rfind('\n').map_or(0, |pos| pos + 1);
    let col = src[line_start..upper].chars().count();
    (line, col)
}

/// If the `end` keyword at `end_start` is the first non-whitespace character on
/// its line, return the byte offset of that line's start; otherwise `None`.
fn line_start_if_end_leads(end_start: u32, cx: &Cx<'_>) -> Option<u32> {
    let src = cx.source();
    let end_start = end_start as usize;
    let line_start = src[..end_start].rfind('\n').map_or(0, |pos| pos + 1);
    if src[line_start..end_start]
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
    {
        Some(line_start as u32)
    } else {
        None
    }
}

murphy_plugin_api::submit_cop!(BeginEndAlignment);

#[cfg(test)]
mod tests {
    use super::{AlignWith, BeginEndAlignment as Cop, BeginEndAlignmentOptions};
    use murphy_plugin_api::test_support::{CapturedEdit, run_cop, run_cop_with_edits, run_cop_with_options, test};

    fn begin_style() -> BeginEndAlignmentOptions {
        BeginEndAlignmentOptions {
            enforced_style_align_with: AlignWith::Begin,
        }
    }

    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut sorted: Vec<&CapturedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.range.start);
        let mut out = String::new();
        let mut cursor = 0usize;
        for e in sorted {
            out.push_str(&source[cursor..e.range.start as usize]);
            out.push_str(&e.replacement);
            cursor = e.range.end as usize;
        }
        out.push_str(&source[cursor..]);
        out
    }

    #[test]
    fn accepts_aligned_begin_at_line_start() {
        test::<Cop>().expect_no_offenses("begin\nend\n");
    }

    #[test]
    fn flags_misaligned_end_start_of_line() {
        let src = "begin\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `begin` at 1, 0."
        );
    }

    #[test]
    fn start_of_line_uses_full_line_source() {
        // `var << begin` — start_of_line wants `end` at column 0, message
        // source is the whole trimmed first line.
        let src = "var << begin\n       end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 7 is not aligned with `var << begin` at 1, 0."
        );
    }

    #[test]
    fn start_of_line_accepts_end_at_line_start() {
        // `end` aligned with the line start (column 0), even though `begin`
        // is not at column 0.
        test::<Cop>().expect_no_offenses("var = begin\nend\n");
    }

    #[test]
    fn start_of_line_accepts_chained_begin_end_at_line_start() {
        test::<Cop>().expect_no_offenses("puts 1; begin\nend\n");
    }

    #[test]
    fn begin_style_accepts_end_aligned_with_begin() {
        let src = "puts 1; begin\n        end\n";
        let offenses = run_cop_with_options::<Cop>(src, &begin_style());
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn begin_style_flags_misaligned() {
        let src = "begin\n  end\n";
        let offenses = run_cop_with_options::<Cop>(src, &begin_style());
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `begin` at 1, 0."
        );
    }

    #[test]
    fn corrects_misaligned_end() {
        let src = "begin\n  end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "begin\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "begin\n      end\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn accepts_begin_rescue() {
        test::<Cop>().expect_no_offenses("begin\n  foo\nrescue => e\n  nil\nend\n");
    }
}
