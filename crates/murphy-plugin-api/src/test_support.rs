//! Parser-driven cop test harness, gated by the `test-support` feature.
//!
//! Any plugin pack — `murphy-std`, `murphy-example-pack`,
//! `murphy-rspec`, third-party packs — can enable this feature in its
//! `[dev-dependencies]` and write `#[cfg(test)] mod tests` against
//! its own cops without rebuilding the `CxRaw` + offense-sink
//! plumbing every time.
//!
//! Production plugin binaries never touch this module — `murphy-translate`
//! (the runtime parser) is an optional dep activated only by the
//! feature. With the feature off, this file is `#[cfg]`-gated out of
//! compilation entirely.
//!
//! # Example
//!
//! ```ignore
//! use murphy_plugin_api::test_support::run_cop;
//! use my_pack::MyCop;
//!
//! #[test]
//! fn flags_the_thing() {
//!     let offenses = run_cop::<MyCop>("def foo; end\n");
//!     assert_eq!(offenses.len(), 1);
//!     assert_eq!(offenses[0].cop_name, "Plugin/MyCop");
//! }
//! ```
//!
//! # Dispatch
//!
//! For per-kind cops (`KINDS = &[..]`) every `NodeId` in the arena is
//! handed to `check`; the macro-generated dispatch routes only matching
//! kinds. For file-visit / investigation cops (`KINDS = &[]`) `check`
//! is called once with `cx.root()`, matching the
//! `murphy-core::dispatch::run_cops` contract.

use std::cell::RefCell;

use murphy_ast::Ast;

use crate::{
    Cop, Cx, CxRaw, FnTable, NodeCop, NodeId, Range, RawEdit, RawOffense, RawSlice, Severity,
};

/// Re-export of [`indoc::indoc!`] so plugin packs writing
/// `#[cfg(test)] mod tests` can lift their Ruby fixture strings out of
/// the surrounding Rust indentation without re-declaring the dep. The
/// macro strips the common leading whitespace at compile time.
pub use indoc::indoc;

/// Assert that `Cop` emits no offenses against `src`. Companion to
/// [`expect_offense!`]; see the module docs for the annotation grammar.
#[macro_export]
macro_rules! expect_no_offenses {
    ($cop:ty, $src:expr) => {{
        $crate::test_support::__assert_no_offenses::<$cop>($src);
    }};
}

pub use crate::expect_no_offenses;

/// `expect_no_offenses!`'s inner assertion. Not part of the public API.
#[track_caller]
pub fn __assert_no_offenses<T: NodeCop + Default>(src: &str) {
    let (_cleaned, expected) = parse_annotated(src);
    if !expected.is_empty() {
        panic!("expect_no_offenses! must not contain annotations; use expect_offense! instead");
    }
    let offenses = run_cop::<T>(src);
    if !offenses.is_empty() {
        panic!(
            "expect_no_offenses! found {} offense(s) for {}",
            offenses.len(),
            <T as Cop>::NAME,
        );
    }
}

/// Assert that `Cop` emits exactly the set of offenses described by the
/// caret annotations in `src`. See the module docs for the grammar.
#[macro_export]
macro_rules! expect_offense {
    ($cop:ty, $src:expr) => {{
        $crate::test_support::__assert_offenses_match::<$cop>($src);
    }};
}

pub use crate::expect_offense;

/// One annotation parsed out of an `expect_offense!` input.
#[derive(Debug, Clone)]
struct Expected {
    range: Range,
    /// `None` means "carets only" — the comparator matches on range
    /// and ignores the cop's emitted message.
    message: Option<String>,
}

/// Parse `annotated` into (cleaned source, expected items).
///
/// Annotation lines (first non-whitespace char is `^`) are stripped and
/// converted to expected ranges against the source line directly above.
/// Caret columns are interpreted as **char indices** of the source
/// line, then translated to bytes via `char_indices`. Non-ASCII source
/// lines are supported.
fn parse_annotated(annotated: &str) -> (String, Vec<Expected>) {
    let mut cleaned_lines: Vec<&str> = Vec::new();
    let mut expected: Vec<Expected> = Vec::new();
    let mut byte_offset: u32 = 0;
    let mut last_source_line_start: Option<u32> = None;
    let mut last_source_line: Option<&str> = None;
    let mut annotations_for_current_line: u32 = 0;

    for line in annotated.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('^') {
            let leading_ws = line.len() - trimmed.len();
            let caret_len = trimmed.chars().take_while(|c| *c == '^').count();
            let after_carets = &trimmed[caret_len..]; // '^' is 1 byte = 1 char
            let msg_raw = after_carets.trim();
            let message = if msg_raw.is_empty() {
                None
            } else {
                Some(msg_raw.to_string())
            };

            let line_start = last_source_line_start
                .expect("expect_offense!: annotation precedes any source line");
            let src_line = last_source_line.unwrap();

            if annotations_for_current_line >= 1 {
                panic!(
                    "expect_offense!: multiple annotations per source line not yet supported (line: {:?})",
                    src_line
                );
            }
            annotations_for_current_line += 1;

            let s_byte = nth_char_byte(src_line, leading_ws);
            let e_byte = nth_char_byte(src_line, leading_ws + caret_len);
            expected.push(Expected {
                range: Range {
                    start: line_start + s_byte as u32,
                    end: line_start + e_byte as u32,
                },
                message,
            });
        } else {
            last_source_line_start = Some(byte_offset);
            last_source_line = Some(line);
            cleaned_lines.push(line);
            byte_offset += line.len() as u32 + 1; // +1 for the joined '\n'
            annotations_for_current_line = 0;
        }
    }

    let mut cleaned = cleaned_lines.join("\n");
    if annotated.ends_with('\n') {
        cleaned.push('\n');
    }
    (cleaned, expected)
}

/// Byte offset of the `n`-th char of `line`, or `line.len()` if `n` is
/// past the end. ASCII fast path is the common case; multibyte is
/// supported transparently via `char_indices`.
fn nth_char_byte(line: &str, n: usize) -> usize {
    line.char_indices()
        .nth(n)
        .map(|(b, _)| b)
        .unwrap_or(line.len())
}

/// `expect_offense!`'s inner assertion. Not part of the public API —
/// call sites go through the macro so `#[track_caller]` makes the
/// panic point at the test line, not this helper.
#[track_caller]
pub fn __assert_offenses_match<T: NodeCop + Default>(annotated: &str) {
    let (cleaned, expected) = parse_annotated(annotated);
    if expected.is_empty() {
        panic!(
            "expect_offense! must contain at least one annotation; use expect_no_offenses! instead"
        );
    }
    let actuals = run_cop::<T>(&cleaned);

    let mut exp_sorted: Vec<Expected> = expected.clone();
    exp_sorted.sort_by(|a, b| {
        (a.range.start, a.range.end, &a.message).cmp(&(b.range.start, b.range.end, &b.message))
    });
    let mut act_sorted: Vec<CapturedOffense> = actuals.clone();
    act_sorted.sort_by(|a, b| {
        (a.range.start, a.range.end, &a.message).cmp(&(b.range.start, b.range.end, &b.message))
    });

    let mut ok = exp_sorted.len() == act_sorted.len();
    if ok {
        for (e, a) in exp_sorted.iter().zip(act_sorted.iter()) {
            if e.range != a.range {
                ok = false;
                break;
            }
            if let Some(em) = &e.message
                && em != &a.message
            {
                ok = false;
                break;
            }
        }
    }
    if !ok {
        let exp_items: Vec<(Range, Option<&str>)> = exp_sorted
            .iter()
            .map(|e| (e.range, e.message.as_deref()))
            .collect();
        let act_items: Vec<(Range, Option<&str>)> = act_sorted
            .iter()
            .map(|a| (a.range, Some(a.message.as_str())))
            .collect();
        panic!(
            "expect_offense! mismatch\n\nexpected:\n{}\nactual:\n{}",
            indent_block(&render(&cleaned, &exp_items)),
            indent_block(&render(&cleaned, &act_items)),
        );
    }
}

/// Render `src` with caret annotations under each source line carrying
/// items. Format matches the macro's input grammar (round-trippable).
///
/// Multi-line ranges (`range.end` past the source line's end) emit
/// carets only over the first line and append ` (+ N more chars)` to
/// the message to flag the truncation.
fn render(src: &str, items: &[(Range, Option<&str>)]) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let line_byte_starts = compute_line_byte_starts(&lines);

    let mut by_line: Vec<Vec<&(Range, Option<&str>)>> = vec![Vec::new(); lines.len()];
    for item in items {
        let start = item.0.start as usize;
        let line_idx = line_byte_starts
            .iter()
            .rposition(|&s| s <= start)
            .unwrap_or(0)
            .min(lines.len().saturating_sub(1));
        if !lines.is_empty() {
            by_line[line_idx].push(item);
        }
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        out.push_str(line);
        out.push('\n');
        for item in &by_line[i] {
            let (range, msg) = (item.0, item.1);
            let line_start = line_byte_starts[i];
            let line_end = line_start + line.len();
            let caret_start_byte = (range.start as usize).saturating_sub(line_start);
            let caret_end_byte = (range.end as usize).min(line_end) - line_start;
            let caret_col = byte_to_char_col(line, caret_start_byte);
            let caret_chars = line[caret_start_byte..caret_end_byte].chars().count();
            for _ in 0..caret_col {
                out.push(' ');
            }
            for _ in 0..caret_chars.max(1) {
                out.push('^');
            }
            let overflow = (range.end as usize).saturating_sub(line_end);
            if let Some(m) = msg {
                out.push(' ');
                out.push_str(m);
            }
            if overflow > 0 {
                out.push_str(&format!(" (+ {} more chars)", overflow));
            }
            out.push('\n');
        }
    }
    out
}

fn compute_line_byte_starts(lines: &[&str]) -> Vec<usize> {
    let mut acc = 0usize;
    let mut starts = Vec::with_capacity(lines.len());
    for line in lines {
        starts.push(acc);
        acc += line.len() + 1; // +1 for the joined '\n'
    }
    starts
}

fn byte_to_char_col(line: &str, byte_offset: usize) -> usize {
    line[..byte_offset.min(line.len())].chars().count()
}

fn indent_block(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One offense captured by [`run_cop`]. Fields are owned `String`s so
/// callers can inspect them after the underlying `Ast` / `CxRaw` are
/// dropped (the cop receives a borrowed `&Cx<'_>`; we copy out at
/// emission time).
#[derive(Debug, Clone)]
pub struct CapturedOffense {
    pub cop_name: String,
    pub message: String,
    pub range: Range,
    /// `None` when the cop didn't override (host applies its default);
    /// otherwise the cop's declared severity.
    pub severity: Option<Severity>,
}

/// Mutable scratch the FFI callbacks borrow through a `*mut c_void`.
struct Sink {
    offenses: Vec<CapturedOffense>,
}

unsafe extern "C" fn record_offense(sink_ptr: *mut std::ffi::c_void, o: *const RawOffense) {
    let sink = unsafe { &*(sink_ptr as *const RefCell<Sink>) };
    let o = unsafe { &*o };
    let cop_name = String::from_utf8(unsafe { o.cop_name.as_bytes() }.to_vec())
        .expect("cop_name must be UTF-8");
    let message =
        String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).expect("message must be UTF-8");
    sink.borrow_mut().offenses.push(CapturedOffense {
        cop_name,
        message,
        range: o.range,
        severity: Severity::from_wire(o.severity),
    });
}

unsafe extern "C" fn ignore_edit(_sink: *mut std::ffi::c_void, _e: *const RawEdit) {
    // Autocorrect edits are not captured by the basic harness. Cops
    // that emit edits should write a richer test that records them;
    // this default keeps the FnTable valid.
}

/// Parse `source` as Ruby, drive `T::check` over every relevant node,
/// and return the captured offenses in emission order.
///
/// The cop is instantiated via `T::default()` — matches the stateless
/// `#[derive(Default)]` shape every Murphy cop uses (ADR 0035).
pub fn run_cop<T: NodeCop + Default>(source: &str) -> Vec<CapturedOffense> {
    let ast = murphy_translate::translate(source, "t.rb");
    let cop = T::default();
    let cop_name = RawSlice::from_str(<T as Cop>::NAME);
    let sink = RefCell::new(Sink {
        offenses: Vec::new(),
    });
    let fns = FnTable {
        emit_offense: record_offense,
        emit_edit: ignore_edit,
    };
    let raw = cx_raw_for(&ast, &fns, cop_name, &sink);
    let cx = unsafe { Cx::from_raw(&raw) };

    if T::KINDS.is_empty() {
        // File-visit / investigation dispatch — single call with root,
        // matching the host's `KINDS = &[]` contract.
        cop.check(ast.root(), &cx);
    } else {
        // Per-kind dispatch — feed every node; the macro-generated
        // `check` filters by tag.
        let node_count = ast.raw_parts().nodes.len();
        for i in 0..node_count {
            cop.check(NodeId(i as u32), &cx);
        }
    }

    sink.into_inner().offenses
}

/// Build a `CxRaw` borrowing from `ast`, `fns`, and `sink`. The
/// returned value contains raw pointers; the caller keeps all three
/// alive for the duration of the dispatch.
fn cx_raw_for(ast: &Ast, fns: &FnTable, cop_name: RawSlice, sink: &RefCell<Sink>) -> CxRaw {
    let p = ast.raw_parts();
    CxRaw {
        nodes: p.nodes.as_ptr(),
        nodes_len: p.nodes.len(),
        lists: p.node_lists.as_ptr(),
        lists_len: p.node_lists.len(),
        interner_blob: p.interner_blob.as_ptr(),
        interner_blob_len: p.interner_blob.len(),
        interner_offsets: p.interner_offsets.as_ptr(),
        interner_offsets_len: p.interner_offsets.len(),
        comments: p.comments.as_ptr(),
        comments_len: p.comments.len(),
        source: p.source.as_ptr(),
        source_len: p.source.len(),
        root: p.root,
        cop_name,
        fns: fns as *const FnTable,
        sink: sink as *const _ as *mut std::ffi::c_void,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_annotated, render};
    use crate::{Cop, Cx, NoOptions, NodeCop, NodeKindTag, Range};
    use murphy_ast::NodeId;

    /// Fixture: emits nothing.
    #[derive(Default)]
    struct NoopCop;
    impl Cop for NoopCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/Noop";
    }
    impl NodeCop for NoopCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
    }

    /// Fixture: emits one offense over byte range `[0, 3)` with message
    /// `"fixed"`. The 3-byte window matches a 3-char ASCII source like
    /// `"abc"` or `"def"` — fully predictable for caret-grammar tests.
    #[derive(Default)]
    struct FixedRangeCop;
    impl Cop for FixedRangeCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/FixedRange";
    }
    impl NodeCop for FixedRangeCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 0, end: 3 }, "fixed", None);
        }
    }

    /// Fixture: emits two offenses on the same source — `[0, 3)` and
    /// `[4, 7)`. Pairs with a "abc\ndef\n" fixture so the second range
    /// lands cleanly on the second line. Used to exercise the count
    /// mismatch path in the comparator.
    #[derive(Default)]
    struct TwoEmitCop;
    impl Cop for TwoEmitCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/TwoEmit";
    }
    impl NodeCop for TwoEmitCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 0, end: 3 }, "one", None);
            cx.emit_offense(Range { start: 4, end: 7 }, "two", None);
        }
    }

    #[test]
    fn expect_no_offenses_passes_when_cop_emits_nothing() {
        expect_no_offenses!(NoopCop, "x = 1\n");
    }

    #[test]
    #[should_panic(expected = "expect_no_offenses! found 1 offense(s)")]
    fn expect_no_offenses_panics_when_cop_emits() {
        expect_no_offenses!(FixedRangeCop, "abc\n");
    }

    #[test]
    fn expect_offense_matches_single_annotation() {
        expect_offense!(
            FixedRangeCop,
            "abc\n\
             ^^^ fixed\n"
        );
    }

    #[test]
    fn expect_offense_empty_message_is_range_only_match() {
        // No message after the carets: range matches, message ignored.
        expect_offense!(
            FixedRangeCop,
            "abc\n\
             ^^^\n"
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense! mismatch")]
    fn expect_offense_panics_on_extra_emit() {
        // TwoEmitCop emits 2 offenses; assert only 1. The comparator's
        // count-mismatch branch fires (this case is not intercepted by
        // the zero-annotation guard because the input has 1 annotation).
        expect_offense!(
            TwoEmitCop,
            "abc\n\
             ^^^ one\n\
             def\n"
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense! mismatch")]
    fn expect_offense_panics_on_missing_emit() {
        expect_offense!(
            NoopCop,
            "abc\n\
             ^^^ wanted\n"
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense! mismatch")]
    fn expect_offense_panics_on_range_mismatch() {
        // FixedRangeCop emits [0, 3); test asserts [0, 2).
        expect_offense!(
            FixedRangeCop,
            "abc\n\
             ^^ fixed\n"
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense! mismatch")]
    fn expect_offense_panics_on_message_mismatch() {
        expect_offense!(
            FixedRangeCop,
            "abc\n\
             ^^^ wrong message\n"
        );
    }

    #[test]
    fn render_round_trips_single_annotation() {
        // Render is the inverse of parse_annotated for single-line
        // annotations: feed (cleaned, expected items) back in and the
        // output must match the original annotated input.
        let annotated = "abc\n^^^ fixed\n";
        let (cleaned, expected) = parse_annotated(annotated);
        let items: Vec<(Range, Option<&str>)> = expected
            .iter()
            .map(|e| (e.range, e.message.as_deref()))
            .collect();
        let rendered = render(&cleaned, &items);
        assert_eq!(rendered, annotated);
    }

    #[test]
    fn failure_panic_includes_rendered_expected_and_actual() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let result = catch_unwind(AssertUnwindSafe(|| {
            expect_offense!(
                FixedRangeCop,
                "abc\n\
                 ^^ wrong\n"
            );
        }));
        let err = result.expect_err("expected a panic");
        let msg: String = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&'static str>().map(|s| s.to_string()))
            .expect("panic payload was neither String nor &'static str");
        // Header
        assert!(msg.contains("expect_offense! mismatch"), "msg: {msg}");
        // Both sections rendered
        assert!(msg.contains("expected:"), "msg: {msg}");
        assert!(msg.contains("actual:"), "msg: {msg}");
        // Expected side preserves the 2-caret annotation; actual side
        // re-renders the cop's true 3-caret range with the real msg.
        assert!(
            msg.contains("^^ wrong"),
            "expected-side carets missing\nmsg: {msg}"
        );
        assert!(
            msg.contains("^^^ fixed"),
            "actual-side carets missing\nmsg: {msg}"
        );
    }

    #[test]
    fn render_marks_multi_line_range_with_overflow_suffix() {
        // A range that crosses a newline gets carets only on the first
        // line and ` (+ N more chars)` appended to the message. Source
        // "abc\ndef\n" has line "abc" at bytes [0..3]; a range of
        // [0..6) covers 3 bytes on line 0 plus 3 more on the next line
        // (the newline, 'd', 'e').
        let src = "abc\ndef\n";
        let items = vec![(Range { start: 0, end: 6 }, Some("spans"))];
        let rendered = render(src, &items);
        assert_eq!(rendered, "abc\n^^^ spans (+ 3 more chars)\ndef\n");
    }

    /// Fixture: emits one offense over the byte range covering the
    /// first **char** (not byte) of the source. Used to verify
    /// multibyte handling in the caret-to-byte conversion.
    #[derive(Default)]
    struct FirstCharCop;
    impl Cop for FirstCharCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/FirstChar";
    }
    impl NodeCop for FirstCharCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            // Span the first char of the file by char count, translated
            // to bytes via char_indices.
            let src = cx.source();
            let end_byte = src
                .char_indices()
                .nth(1)
                .map(|(b, _)| b)
                .unwrap_or(src.len());
            cx.emit_offense(
                Range {
                    start: 0,
                    end: end_byte as u32,
                },
                "first char",
                None,
            );
        }
    }

    #[test]
    fn expect_offense_handles_multibyte_source_line() {
        // Japanese 'あ' is 3 bytes / 1 char in UTF-8. A single caret
        // (1 char) over 'あ' must map to the 3-byte range [0, 3).
        expect_offense!(
            FirstCharCop,
            "あい\n\
             ^ first char\n"
        );
    }

    #[test]
    #[should_panic(expected = "annotation precedes any source line")]
    fn expect_offense_panics_when_annotation_has_no_source_above() {
        expect_offense!(FixedRangeCop, "^^^ orphan\nabc\n");
    }

    #[test]
    #[should_panic(expected = "multiple annotations per source line not yet supported")]
    fn expect_offense_panics_on_two_consecutive_annotations() {
        expect_offense!(
            FixedRangeCop,
            "abc\n\
             ^^^ first\n\
             ^^^ second\n"
        );
    }

    #[test]
    #[should_panic(
        expected = "expect_offense! must contain at least one annotation; use expect_no_offenses! instead"
    )]
    fn expect_offense_panics_when_input_has_no_annotations() {
        // Symmetric guard to expect_no_offenses_panics_on_caret_input:
        // the user picked the wrong macro. Catching the typo prevents
        // a silent pass when the cop happens to emit nothing.
        expect_offense!(NoopCop, "abc\n");
    }

    #[test]
    #[should_panic(
        expected = "expect_no_offenses! must not contain annotations; use expect_offense! instead"
    )]
    fn expect_no_offenses_panics_on_caret_input() {
        // Misuse guard: annotations in expect_no_offenses! are a typo
        // for the wrong macro. Catching this saves silent test passes.
        expect_no_offenses!(
            NoopCop,
            "abc\n\
             ^^^ stray\n"
        );
    }
}
