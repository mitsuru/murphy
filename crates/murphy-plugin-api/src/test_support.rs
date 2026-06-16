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
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::{fs, vec::Vec};

use murphy_ast::Ast;

use crate::{
    Cop, CopOptions, Cx, CxRaw, FnTable, NodeCop, NodeId, OptionSpec, PluginCopV1, Range, RawEdit,
    RawOffense, RawSlice, Severity,
};

/// Re-export of [`indoc::indoc!`] so plugin packs writing
/// `#[cfg(test)] mod tests` can lift their Ruby fixture strings out of
/// the surrounding Rust indentation without re-declaring the dep. The
/// macro strips the common leading whitespace at compile time.
pub use indoc::indoc;

/// Assert that every `#[cop(name = "...")]` registration in a plugin
/// pack's `src/` tree has a matching source-near `murphy-parity` block.
///
/// The block must name the cop exactly with either `upstream_cop: ...`
/// for RuboCop-derived cops or `cop: ...` for Murphy-native/custom cops.
/// RuboCop-derived blocks must include upstream identity/version fields,
/// and non-verified statuses must keep an explicit gap issue list.
#[track_caller]
pub fn assert_cop_parity_metadata_for_crate(manifest_dir: impl AsRef<Path>) {
    let manifest_dir = manifest_dir.as_ref();
    let mut failures = Vec::new();

    for file in cop_source_files(manifest_dir) {
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        let rel = file
            .strip_prefix(manifest_dir)
            .unwrap_or(&file)
            .display()
            .to_string();
        collect_parity_failures(&source, &rel, &mut failures);
    }

    assert!(
        failures.is_empty(),
        "missing parity metadata:\n{}",
        failures.join("\n")
    );
}

/// Assert that every cop option's config key in `cops` is RuboCop-style
/// PascalCase (`^[A-Z][A-Za-z0-9]*$`, e.g. `Max`, `EnforcedStyle`,
/// `MaximumRangeSize`, `CountAsOne`).
///
/// A `#[derive(CopOptions)]` field declared without `#[option(name = "...")]`
/// falls back to its snake_case Rust identifier (`max`, `enforced_style`),
/// which never matches the PascalCase key users write in `.murphy.yml`, so the
/// option is silently ignored. Each pack passes its `PACK_COPS` slice; this
/// guard fails the build the moment a snake_case key sneaks in. See
/// `murphy-pj12`.
#[track_caller]
pub fn assert_pack_option_keys_pascal_case(cops: &[PluginCopV1]) {
    fn is_pascal_case(key: &str) -> bool {
        let mut chars = key.chars();
        // The first char is verified ASCII-uppercase (a subset of
        // alphanumeric), so reuse the same iterator for the remaining chars
        // rather than re-scanning the key from the start.
        chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_alphanumeric())
    }

    let mut offenders = Vec::new();
    for cop in cops {
        let cop_name = std::str::from_utf8(unsafe { cop.name.as_bytes() }).unwrap_or("<bad utf8>");
        if cop.options_ptr.is_null() {
            continue;
        }
        // Safety: `options_ptr`/`options_len` describe the cop's `OptionSpec`
        // table, a `'static` slice emitted by the `#[derive(CopOptions)]` macro.
        let specs: &[OptionSpec] =
            unsafe { std::slice::from_raw_parts(cop.options_ptr, cop.options_len) };
        for spec in specs {
            let key = std::str::from_utf8(unsafe { spec.name.as_bytes() }).unwrap_or("<bad utf8>");
            if !is_pascal_case(key) {
                offenders.push(format!("  {cop_name}: `{key}`"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "option keys must be RuboCop-style PascalCase (add `#[option(name = \"...\")]`):\n{}",
        offenders.join("\n")
    );
}

#[cfg(test)]
mod option_key_pascal_case_guard_tests {
    use super::assert_pack_option_keys_pascal_case;
    use crate::{CxRaw, NodeId, NodeKindTag, OptionSpec, PluginCopV1, RawSlice, SEVERITY_UNSET};

    unsafe extern "C" fn noop_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        0
    }

    static KINDS: &[NodeKindTag] = &[NodeKindTag(1)];
    static PASCAL_OPTS: &[OptionSpec] = &[opt("Max"), opt("EnforcedStyle")];
    static SNAKE_OPTS: &[OptionSpec] = &[opt("max"), opt("enforced_style")];

    const fn opt(name: &'static str) -> OptionSpec {
        OptionSpec {
            name: RawSlice::from_str(name),
            ty: RawSlice::from_str("bool"),
            default_json: RawSlice::EMPTY,
            description: RawSlice::EMPTY,
            enum_values_json: RawSlice::EMPTY,
            replacement: RawSlice::EMPTY,
            reason: RawSlice::EMPTY,
        }
    }

    /// Minimal cop carrying only the fields the guard reads (`name`,
    /// `options_ptr`, `options_len`); the rest are inert placeholders. An
    /// empty `options` slice yields a null `options_ptr` to exercise the
    /// "cop has no options" skip path.
    fn cop(name: &'static str, options: &'static [OptionSpec]) -> PluginCopV1 {
        PluginCopV1 {
            size: std::mem::size_of::<PluginCopV1>(),
            name: RawSlice::from_str(name),
            description: RawSlice::EMPTY,
            default_severity: SEVERITY_UNSET,
            default_enabled: 255,
            options_ptr: if options.is_empty() {
                std::ptr::null()
            } else {
                options.as_ptr()
            },
            options_len: options.len(),
            kinds_ptr: KINDS.as_ptr(),
            kinds_len: KINDS.len(),
            dispatch: noop_dispatch,
            send_methods_ptr: std::ptr::null(),
            send_methods_len: 0,
            safe: 255,
            safe_autocorrect: 255,
            minimum_target_ruby_version: 0,
        }
    }

    #[test]
    fn passes_on_pascal_case_keys() {
        assert_pack_option_keys_pascal_case(&[cop("Style/Foo", PASCAL_OPTS)]);
    }

    #[test]
    fn passes_on_empty_pack() {
        assert_pack_option_keys_pascal_case(&[]);
    }

    #[test]
    fn skips_cop_with_no_options() {
        // Null `options_ptr` must be skipped, not dereferenced.
        assert_pack_option_keys_pascal_case(&[cop("Style/Bar", &[])]);
    }

    #[test]
    #[should_panic(expected = "PascalCase")]
    fn panics_on_snake_case_keys() {
        assert_pack_option_keys_pascal_case(&[cop("Style/Baz", SNAKE_OPTS)]);
    }
}

/// Validate the parity metadata of a single source file, pushing a
/// human-readable message per problem onto `failures`.
///
/// Every registered `#[cop(name = "...")]` cop must carry a matching parity
/// block. Any parity block that matches no registered cop — whether it is the
/// only block in an `#[cop]`-less ABI-blocker placeholder (`Style/RedundantParentheses`)
/// or a stray block sitting alongside real cops — is validated as an *orphan*,
/// so a `status: blocked` placeholder stays visible to validation instead of
/// being silently skipped.
fn collect_parity_failures(source: &str, file: &str, failures: &mut Vec<String>) {
    let cop_names = cop_names_in_source(source);
    let parity_blocks = parity_blocks_in_source(source);

    for name in &cop_names {
        match parity_blocks
            .iter()
            .find(|block| block_matches_cop(block, name))
        {
            Some(block) => validate_parity_block(block, name, file, failures),
            None => failures.push(format!("{file}: missing murphy-parity block for {name}")),
        }
    }

    // Any block that no registered cop claims — including the sole block of an
    // `#[cop]`-less placeholder — is validated as an orphan.
    for block in &parity_blocks {
        if !cop_names.iter().any(|name| block_matches_cop(block, name)) {
            validate_orphan_parity_block(block, file, failures);
        }
    }
}

/// Entry point for the tester-builder API. Cop type comes in as a
/// generic parameter, options are added via `with_options`, and one or
/// more expectations chain off the resulting `Tester`.
///
/// ```ignore
/// use murphy_plugin_api::test_support::test;
///
/// // No options: every field of `Cop::Options` falls back to default.
/// test::<MyCop>()
///     .expect_offense(indoc! {r#"
///         x==0
///          ^^ Surrounding space missing for operator `==`.
///     "#})
///     .expect_correction(indoc! {r#"
///         a+b
///          ^ Surrounding space missing for operator `+`.
///     "#}, "a + b\n");
///
/// // Typed options:
/// test::<MyCop>()
///     .with_options(&MyOpts { foo: true, ..Default::default() })
///     .expect_offense(indoc! {r#"
///         …
///     "#});
/// ```
///
/// Each `expect_*` method returns `&Self` so multiple expectations
/// chain without an intermediate `let`.
pub fn test<T: NodeCop + Default>() -> Tester<T> {
    Tester {
        options_json: DEFAULT_OPTIONS_JSON.to_string(),
        context: crate::AllCopsContext::default(),
        _phantom: PhantomData,
    }
}

/// Cop-tester returned by [`test`]. Holds the per-test options JSON and
/// dispatches every expectation through the shared internal assertion
/// helpers.
///
/// `PhantomData<fn() -> T>` makes the phantom type both covariant and
/// `Send + Sync` regardless of `T`; the tester itself owns no `T`
/// value.
pub struct Tester<T: NodeCop + Default> {
    options_json: String,
    context: crate::AllCopsContext,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: NodeCop + Default> Tester<T> {
    /// Attach typed options to the tester. Consumes and returns `Self`
    /// so it can sit at the start of the method chain
    /// (`test::<T>().with_options(&opts).expect_offense(…)`).
    /// Subsequent calls overwrite any previously-stored options.
    pub fn with_options(mut self, opts: &<T as Cop>::Options) -> Self {
        self.options_json = opts.to_config_json();
        self
    }

    /// Set `AllCops.TargetRailsVersion` for this cop test.
    pub fn with_target_rails_version(mut self, major: u16, minor: u16) -> Self {
        self.context.target_rails_version = Some(crate::RubyVersion::new(major, minor));
        self
    }

    /// Set `AllCops.TargetRubyVersion` for this cop test. Without this, the
    /// default context leaves the Ruby target unset (`None`), so a cop under
    /// test resolves its own default floor (murphy's is Ruby 3.1).
    pub fn with_target_ruby_version(mut self, major: u16, minor: u16) -> Self {
        self.context.target_ruby_version = Some(crate::RubyVersion::new(major, minor));
        self
    }

    /// Set `AllCops.ActiveSupportExtensionsEnabled` for this cop test.
    pub fn with_active_support_extensions_enabled(mut self, enabled: bool) -> Self {
        self.context.active_support_extensions_enabled = enabled;
        self
    }

    /// Set the run-wide resolved `Layout/IndentationWidth.Width` for this cop
    /// test — what `Cx::indentation_width()` returns (murphy-bgd8). Use this to
    /// exercise a cop's cross-cop fallback to the shared indentation width.
    pub fn with_indentation_width(mut self, width: i64) -> Self {
        self.context.indentation_width = width;
        self
    }

    /// Set the run-wide `Naming/BlockForwarding.EnforcedStyle == "explicit"`
    /// flag — what `Cx::block_forwarding_explicit()` returns. Use this to
    /// exercise `Style/ArgumentsForwarding`'s explicit-block-name parity.
    pub fn with_block_forwarding_explicit(mut self, explicit: bool) -> Self {
        self.context.block_forwarding_explicit = explicit;
        self
    }

    /// Assert the cop emits exactly the offenses described by the caret
    /// annotations in `annotated`. See the module docs for the
    /// annotation grammar.
    #[track_caller]
    pub fn expect_offense(&self, annotated: &str) -> &Self {
        assert_offenses_match_inner::<T>(annotated, &self.options_json, self.context);
        self
    }

    /// Assert the cop emits no offenses against `src`.
    #[track_caller]
    pub fn expect_no_offenses(&self, src: &str) -> &Self {
        assert_no_offenses_inner::<T>(src, &self.options_json, self.context);
        self
    }

    /// Assert the cop emits the annotated offenses against `annotated`
    /// and that applying its autocorrect edits produces `after`.
    #[track_caller]
    pub fn expect_correction(&self, annotated: &str, after: &str) -> &Self {
        assert_correction_match_inner::<T>(annotated, after, &self.options_json, self.context);
        self
    }

    /// Assert the cop emits no autocorrect edits against `src`. The
    /// offense set is not constrained — pair with
    /// [`Tester::expect_offense`] when both must hold.
    #[track_caller]
    pub fn expect_no_corrections(&self, src: &str) -> &Self {
        assert_no_corrections_inner::<T>(src, &self.options_json, self.context);
        self
    }
}

#[track_caller]
fn assert_no_offenses_inner<T: NodeCop + Default>(
    src: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) {
    let (_cleaned, expected) = parse_annotated(src);
    if !expected.is_empty() {
        panic!("expect_no_offenses must not contain annotations; use expect_offense instead");
    }
    let offenses = run_cop_with_options_json_and_context::<T>(src, options_json, ctx);
    if !offenses.is_empty() {
        panic!(
            "expect_no_offenses found {} offense(s) for {}",
            offenses.len(),
            <T as Cop>::NAME,
        );
    }
}

/// One annotation parsed out of an `expect_offense` input.
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
/// converted to expected ranges against the **most recent source line**
/// above. Multiple consecutive annotation lines under the same source
/// line are allowed — each describes one expected offense on that line.
/// Annotations across multiple source lines work as before; the rule is
/// just "an annotation always anchors to the nearest preceding source
/// line".
///
/// Caret columns are interpreted as **char indices** of the source
/// line, then translated to bytes via `char_indices`. Non-ASCII source
/// lines are supported.
///
/// # Strict caret bounds (RuboCop parity)
///
/// A caret run must not reach past the source line's last char —
/// `nth_char_byte` would otherwise clamp the overshoot to `line.len()` and
/// silently pass an over-padded annotation. Such overshoots panic. The
/// sole exempt past-EOL shape is a *single* caret exactly one column past
/// the last char, which marks a zero-width insertion point; multi-caret or
/// further-past-EOL "insertion" carets are rejected (they would clamp to
/// the same zero-width range and otherwise slip through).
#[track_caller]
fn parse_annotated(annotated: &str) -> (String, Vec<Expected>) {
    let mut cleaned_lines: Vec<&str> = Vec::new();
    let mut expected: Vec<Expected> = Vec::new();
    let mut byte_offset: u32 = 0;
    let mut last_source_line_start: Option<u32> = None;
    let mut last_source_line: Option<&str> = None;

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
                .expect("expect_offense: annotation precedes any source line");
            let src_line = last_source_line.unwrap();

            // Strict, insertion-aware bounds (RuboCop parity). A caret run
            // must not reach past the source line's last char; without this
            // guard `nth_char_byte` clamps the overshoot to `line.len()` and
            // a too-wide annotation passes silently. Exactly two shapes are
            // valid:
            //   * in-bounds run:    end_char <= line_char_count
            //   * EOL zero-width insertion: a *single* caret one column past
            //     the last char (start_char == line_char_count, caret_len 1)
            // Anything else over-pads — including multi-caret or far-past-EOL
            // "insertion" carets, which would otherwise clamp to the same
            // zero-width range and still pass.
            let line_char_count = src_line.chars().count();
            let start_char = leading_ws;
            let end_char = start_char + caret_len;
            let in_bounds = end_char <= line_char_count;
            let eol_insertion = start_char == line_char_count && caret_len == 1;
            if !in_bounds && !eol_insertion {
                let detail = if start_char > line_char_count {
                    format!(
                        "carets start at col {start_char}, past the line's last char \
                         (the only valid past-EOL caret is a single one at col {line_char_count})"
                    )
                } else if start_char == line_char_count {
                    format!(
                        "{caret_len} carets at the end-of-line insertion point; a \
                         zero-width insertion offense takes exactly 1 caret"
                    )
                } else {
                    let correct_len = line_char_count - start_char;
                    format!(
                        "carets start at col {start_char}, run {caret_len} caret(s) \
                         reaching char {end_char} (over by {}); use {correct_len} \
                         caret(s) so the run ends at the last source char",
                        end_char - line_char_count,
                    )
                };
                panic!(
                    "expect_offense: caret annotation overshoots source line\n  \
                     source ({line_char_count} chars): {src_line:?}\n  {detail}"
                );
            }

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

#[track_caller]
fn assert_offenses_match_inner<T: NodeCop + Default>(
    annotated: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) {
    let (cleaned, expected) = parse_annotated(annotated);
    if expected.is_empty() {
        panic!(
            "expect_offense must contain at least one annotation; use expect_no_offenses instead"
        );
    }
    let actuals = run_cop_with_options_json_and_context::<T>(&cleaned, options_json, ctx);
    assert_offenses_match("expect_offense", &cleaned, &expected, &actuals);
}

fn assert_offenses_match(
    macro_name: &str,
    cleaned: &str,
    expected: &[Expected],
    actuals: &[CapturedOffense],
) {
    let mut exp_sorted: Vec<Expected> = expected.to_vec();
    exp_sorted.sort_by(|a, b| {
        (a.range.start, a.range.end, &a.message).cmp(&(b.range.start, b.range.end, &b.message))
    });
    let mut act_sorted: Vec<CapturedOffense> = actuals.to_vec();
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
            "{} mismatch\n\nexpected:\n{}\nactual:\n{}",
            macro_name,
            indent_block(&render(cleaned, &exp_items)),
            indent_block(&render(cleaned, &act_items)),
        );
    }
}

#[track_caller]
fn assert_correction_match_inner<T: NodeCop + Default>(
    annotated: &str,
    expected_after: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) {
    let (cleaned, expected) = parse_annotated(annotated);
    if expected.is_empty() {
        panic!(
            "expect_correction must contain at least one annotation; use expect_no_offenses instead"
        );
    }

    let captured =
        run_cop_with_options_and_edits_json_and_context::<T>(&cleaned, options_json, ctx);
    assert_offenses_match("expect_correction", &cleaned, &expected, &captured.offenses);

    let actual_after = apply_captured_edits(&cleaned, &captured.edits);
    if actual_after != expected_after {
        panic!(
            "expect_correction corrected source mismatch\n\nexpected:\n{}\nactual:\n{}",
            indent_block(expected_after),
            indent_block(&actual_after),
        );
    }
}

#[track_caller]
fn assert_no_corrections_inner<T: NodeCop + Default>(
    src: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) {
    let (_cleaned, expected) = parse_annotated(src);
    if !expected.is_empty() {
        panic!("expect_no_corrections must not contain annotations; use expect_correction instead");
    }

    let captured = run_cop_with_options_and_edits_json_and_context::<T>(src, options_json, ctx);
    if !captured.edits.is_empty() {
        panic!(
            "expect_no_corrections found {} edit(s) for {}",
            captured.edits.len(),
            <T as Cop>::NAME,
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

/// One autocorrect edit captured by [`run_cop_with_edits`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedEdit {
    pub range: Range,
    pub replacement: String,
}

/// The complete output captured from one cop run.
#[derive(Debug, Clone)]
pub struct CapturedRun {
    pub offenses: Vec<CapturedOffense>,
    pub edits: Vec<CapturedEdit>,
}

/// Mutable scratch the FFI callbacks borrow through a `*mut c_void`.
struct Sink {
    offenses: Vec<CapturedOffense>,
    edits: Vec<CapturedEdit>,
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

unsafe extern "C" fn record_edit(sink_ptr: *mut std::ffi::c_void, e: *const RawEdit) {
    let sink = unsafe { &*(sink_ptr as *const RefCell<Sink>) };
    let e = unsafe { &*e };
    let replacement = String::from_utf8(unsafe { e.replacement.as_bytes() }.to_vec())
        .expect("edit replacement must be UTF-8");
    sink.borrow_mut().edits.push(CapturedEdit {
        range: e.range,
        replacement,
    });
}

/// Parse `source` as Ruby, drive `T::check` over every relevant node,
/// and return the captured offenses in emission order. The cop sees an
/// empty options JSON blob — every field of its `Options` struct falls
/// back to `Default::default()`. Use [`run_cop_with_options`] when a
/// test needs to exercise a non-default `[cops.rules."…"]` value.
///
/// The cop is instantiated via `T::default()` — matches the stateless
/// `#[derive(Default)]` shape every Murphy cop uses (ADR 0035).
pub fn run_cop<T: NodeCop + Default>(source: &str) -> Vec<CapturedOffense> {
    run_cop_with_options_json::<T>(source, DEFAULT_OPTIONS_JSON)
}

/// `run_cop` companion that hands the cop a typed `Options` value
/// instead of a raw JSON blob. Serialization to the host wire format
/// (the `[cops.rules."Cop/Name"]` table) goes through
/// `CopOptions::to_config_json` — test code never has to assemble JSON
/// by hand.
pub fn run_cop_with_options<T: NodeCop + Default>(
    source: &str,
    opts: &<T as Cop>::Options,
) -> Vec<CapturedOffense> {
    run_cop_with_options_and_edits::<T>(source, opts).offenses
}

/// Parse `source` as Ruby, drive `T::check`, and return both captured
/// offenses and autocorrect edits in emission order. The cop sees an
/// empty options JSON blob — see [`run_cop_with_options_and_edits`] for
/// the non-default variant.
pub fn run_cop_with_edits<T: NodeCop + Default>(source: &str) -> CapturedRun {
    run_cop_with_options_and_edits_json::<T>(source, DEFAULT_OPTIONS_JSON)
}

/// `run_cop_with_edits` companion that hands the cop a typed `Options`
/// value.
pub fn run_cop_with_options_and_edits<T: NodeCop + Default>(
    source: &str,
    opts: &<T as Cop>::Options,
) -> CapturedRun {
    let json = opts.to_config_json();
    run_cop_with_options_and_edits_json::<T>(source, &json)
}

/// Internal raw-JSON entry point shared by every other dispatcher in
/// this module. Not exposed: production tests should go through the
/// typed wrappers so JSON shape stays an implementation detail.
fn run_cop_with_options_json<T: NodeCop + Default>(
    source: &str,
    options_json: &str,
) -> Vec<CapturedOffense> {
    run_cop_with_options_json_and_context::<T>(
        source,
        options_json,
        crate::AllCopsContext::default(),
    )
}

fn run_cop_with_options_json_and_context<T: NodeCop + Default>(
    source: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) -> Vec<CapturedOffense> {
    run_cop_with_options_and_edits_json_and_context::<T>(source, options_json, ctx).offenses
}

fn run_cop_with_options_and_edits_json<T: NodeCop + Default>(
    source: &str,
    options_json: &str,
) -> CapturedRun {
    run_cop_with_options_and_edits_json_and_context::<T>(
        source,
        options_json,
        crate::AllCopsContext::default(),
    )
}

fn run_cop_with_options_and_edits_json_and_context<T: NodeCop + Default>(
    source: &str,
    options_json: &str,
    ctx: crate::AllCopsContext,
) -> CapturedRun {
    let ast = murphy_translate::translate(source, "t.rb");
    let var_model = crate::var_semantic_model::VarSemanticModel::build(&ast);
    let cop = T::default();
    let cop_name = RawSlice::from_str(<T as Cop>::NAME);
    let sink = RefCell::new(Sink {
        offenses: Vec::new(),
        edits: Vec::new(),
    });
    let fns = FnTable {
        emit_offense: record_offense,
        emit_edit: record_edit,
    };
    // The `RawSlice` borrows the caller's `&str`; we keep that borrow
    // alive on the stack until the dispatch loop finishes below.
    // `RawSlice::from_str` only accepts `&'static str`, but here the
    // input is a runtime parameter — assemble the slice by hand so the
    // pointer + length stay tethered to the caller's `&str`.
    let options_slice = RawSlice {
        ptr: options_json.as_ptr(),
        len: options_json.len(),
    };
    let raw = cx_raw_for(&ast, &fns, cop_name, &sink, options_slice, &var_model, ctx);
    let cx = unsafe { Cx::from_raw(&raw) };

    if T::KINDS.is_empty() {
        // File-visit / investigation dispatch — single call with root,
        // matching the host's `KINDS = &[]` contract.
        cop.check(ast.root(), &cx);
    } else {
        // Per-kind dispatch — feed every node; the macro-generated
        // `check` filters by tag. `SEND_METHODS` mirrors the host's
        // pre-dispatch filter so cops using `methods = [...]` see the
        // same call pattern in tests as in production.
        let node_count = ast.raw_parts().nodes.len();
        for i in 0..node_count {
            let node = NodeId(i as u32);
            if send_method_filter_passes::<T>(node, &cx) {
                cop.check(node, &cx);
            }
        }
    }

    let sink = sink.into_inner();
    CapturedRun {
        offenses: sink.offenses,
        edits: sink.edits,
    }
}

/// Mirror the host's `Send`-method pre-dispatch filter (see
/// `murphy-core::dispatch::send_method_passes`) so cops declared with
/// `#[on_node(kind = "send", methods = [...])]` get the same per-node
/// filtering in tests as they do in production. Non-`Send` nodes pass
/// through unchanged; cops without an allow-list pass through too.
fn send_method_filter_passes<T: NodeCop>(node: NodeId, cx: &Cx<'_>) -> bool {
    if T::SEND_METHODS.is_empty() {
        return true;
    }
    let murphy_ast::NodeKind::Send { method, .. } = *cx.kind(node) else {
        return true;
    };
    let method = cx.symbol_str(method).as_bytes();
    T::SEND_METHODS
        .iter()
        .any(|allowed| unsafe { allowed.as_bytes() } == method)
}

fn apply_captured_edits(source: &str, edits: &[CapturedEdit]) -> String {
    let mut ordered: Vec<&CapturedEdit> = edits.iter().collect();
    ordered.sort_by(|a, b| {
        b.range
            .start
            .cmp(&a.range.start)
            .then(b.range.end.cmp(&a.range.end))
            .then(a.replacement.cmp(&b.replacement))
    });

    let mut accepted: Vec<&CapturedEdit> = Vec::new();
    for edit in &ordered {
        let start = edit.range.start as usize;
        let end = edit.range.end as usize;
        if start > end {
            panic!("expect_correction: edit has invalid range {:?}", edit.range);
        }
        if start > source.len() || end > source.len() {
            panic!(
                "expect_correction: edit range {:?} is outside source length {}",
                edit.range,
                source.len()
            );
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            panic!(
                "expect_correction: edit range {:?} does not fall on UTF-8 char boundaries",
                edit.range
            );
        }
        if accepted.iter().any(|accepted| {
            let accepted_start = accepted.range.start as usize;
            let accepted_end = accepted.range.end as usize;
            accepted_start < end && start < accepted_end
        }) {
            panic!("expect_correction: overlapping edit range {:?}", edit.range);
        }
        accepted.push(edit);
    }

    let mut corrected = source.to_owned();
    for edit in accepted {
        corrected.replace_range(
            edit.range.start as usize..edit.range.end as usize,
            &edit.replacement,
        );
    }
    corrected
}

/// Build a `CxRaw` borrowing from `ast`, `fns`, `sink`, `var_model`, and the
/// caller's per-test options JSON blob. The returned value contains raw
/// pointers; the caller keeps all five alive for the duration of the dispatch.
fn cx_raw_for(
    ast: &Ast,
    fns: &FnTable,
    cop_name: RawSlice,
    sink: &RefCell<Sink>,
    options_json: RawSlice,
    var_model: &crate::var_semantic_model::VarSemanticModel,
    ctx: crate::AllCopsContext,
) -> CxRaw {
    let p = ast.raw_parts();
    let file_path = ast.path().to_str().unwrap_or("");
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
        sorted_tokens: p.sorted_tokens.as_ptr(),
        sorted_tokens_len: p.sorted_tokens.len(),
        options_json,
        call_closing_locs: p.call_closing_locs.as_ptr(),
        call_closing_locs_len: p.call_closing_locs.len(),
        call_operator_locs: p.call_operator_locs.as_ptr(),
        call_operator_locs_len: p.call_operator_locs.len(),
        var_model: var_model as *const crate::var_semantic_model::VarSemanticModel,
        node_slice_arena: std::ptr::null_mut(),
        alloc_node_slice: crate::unavailable_alloc_node_slice,
        file_path: RawSlice {
            ptr: file_path.as_ptr(),
            len: file_path.len(),
        },
        target_rails_version: crate::RubyVersion::to_wire(ctx.target_rails_version),
        active_support_extensions_enabled: ctx.active_support_extensions_enabled,
        indentation_width: ctx.indentation_width_wire(),
        target_ruby_version: crate::RubyVersion::to_wire(ctx.target_ruby_version),
        config_disabled_cops: std::ptr::null(),
        config_disabled_cops_len: 0,
        block_forwarding_explicit: ctx.block_forwarding_explicit,
    }
}

fn cop_source_files(manifest_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files(&manifest_dir.join("src"), &mut files);
    files.sort();
    files
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read_dir {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn cop_names_in_source(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_cop_attr = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cop(") {
            in_cop_attr = true;
        }
        if !in_cop_attr {
            continue;
        }
        if let Some(name) = parse_name_literal(trimmed) {
            names.push(name.to_string());
        }
        if trimmed == ")]" || trimmed == ")" || trimmed.ends_with(")]") {
            in_cop_attr = false;
        }
    }

    names
}

fn parse_name_literal(line: &str) -> Option<&str> {
    let start = line.find("name = \"")?;
    let rest = &line[start + "name = \"".len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn parity_blocks_in_source(source: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = source;

    while let Some(start) = rest.find("```murphy-parity") {
        rest = &rest[start + "```murphy-parity".len()..];
        let Some(end) = rest.find("```") else {
            blocks.push(rest);
            break;
        };
        blocks.push(&rest[..end]);
        rest = &rest[end + "```".len()..];
    }

    blocks
}

fn block_matches_cop(block: &str, name: &str) -> bool {
    value_after_key(block, "upstream_cop") == Some(name)
        || value_after_key(block, "cop") == Some(name)
}

fn validate_parity_block(block: &str, name: &str, file: &str, failures: &mut Vec<String>) {
    let Some(status) = value_after_key(block, "status") else {
        failures.push(format!(
            "{file}: murphy-parity block for {name} is missing status"
        ));
        return;
    };

    if !is_known_parity_status(status) {
        failures.push(format!(
            "{file}: murphy-parity block for {name} has unknown status {status:?}"
        ));
    }

    if value_after_key(block, "cop") == Some(name) {
        if status != "custom" {
            failures.push(format!(
                "{file}: custom murphy-parity block for {name} must use status: custom"
            ));
        }
        return;
    }

    for key in ["upstream", "upstream_cop", "upstream_version_checked"] {
        if value_after_key(block, key).is_none() {
            failures.push(format!(
                "{file}: murphy-parity block for {name} is missing {key}"
            ));
        }
    }

    if status_requires_gap_issues(status) && !block.contains("gap_issues:") {
        failures.push(format!(
            "{file}: {status} murphy-parity block for {name} must list gap_issues"
        ));
    }

    if status == "verified" && !block.contains("gap_issues: []") {
        failures.push(format!(
            "{file}: verified murphy-parity block for {name} must use gap_issues: []"
        ));
    }

    if block.contains("Arena-migration stub registered") && status != "stub" {
        failures.push(format!(
            "{file}: Arena-migration registration for {name} must use status: stub"
        ));
    }
}

/// Validate a parity block that belongs to no registered cop — an ABI-blocker
/// placeholder (`status: blocked`) or similar. The displayed cop name is taken
/// from `upstream_cop`/`cop`, falling back to `<orphan>`.
///
/// Orphans are validated for status validity and the gap-issue invariant only;
/// the upstream identity fields are not required, since an orphan has no cop to
/// dispatch and exists mainly to document why an implementation is absent.
fn validate_orphan_parity_block(block: &str, file: &str, failures: &mut Vec<String>) {
    let name = value_after_key(block, "upstream_cop")
        .or_else(|| value_after_key(block, "cop"))
        .unwrap_or("<orphan>");

    let Some(status) = value_after_key(block, "status") else {
        failures.push(format!(
            "{file}: orphan murphy-parity block for {name} is missing status"
        ));
        return;
    };

    if !is_known_parity_status(status) {
        failures.push(format!(
            "{file}: orphan murphy-parity block for {name} has unknown status {status:?}"
        ));
    }

    if status_requires_gap_issues(status) && !block.contains("gap_issues:") {
        failures.push(format!(
            "{file}: {status} murphy-parity block for {name} must list gap_issues"
        ));
    }
}

/// The set of recognised `status:` values. `blocked` marks a parity target whose
/// implementation is held back by an infra/ABI change — the cop is intentionally
/// not registered (see `Style/RedundantParentheses`).
fn is_known_parity_status(status: &str) -> bool {
    matches!(
        status,
        "custom" | "partial" | "stub" | "verified" | "blocked"
    )
}

/// Statuses that must carry a `gap_issues` list (anything short of full parity).
fn status_requires_gap_issues(status: &str) -> bool {
    matches!(status, "blocked" | "partial" | "stub")
}

fn value_after_key<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    block.lines().find_map(|line| {
        metadata_line(line)
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn metadata_line(line: &str) -> &str {
    line.trim()
        .strip_prefix("//!")
        .or_else(|| line.trim().strip_prefix("///"))
        .unwrap_or_else(|| line.trim())
        .trim()
}

/// Default options JSON used by [`run_cop`] / [`run_cop_with_edits`] and
/// the no-`_with_options` macro variants — the empty object causes every
/// field of a cop's `Options` struct to fall back to its declared
/// `Default::default()`.
const DEFAULT_OPTIONS_JSON: &str = "{}";

#[cfg(test)]
mod tests {
    use super::{collect_parity_failures, cop_names_in_source, parse_annotated, render};
    use crate::{Cop, Cx, NoOptions, NodeCop, NodeKind, NodeKindTag, Range, RawSlice};
    use murphy_ast::NodeId;

    #[test]
    fn cop_names_in_source_reads_single_line_cop_attribute() {
        let source = r#"
            #[cop(name = "Example/Inline", severity = "warning")]
            pub struct Inline;
        "#;

        assert_eq!(cop_names_in_source(source), ["Example/Inline"]);
    }

    // ----- Parity metadata validation (murphy-yyjl) -----

    /// A parity block with no surrounding `#[cop(...)]` registration —
    /// an ABI-blocker placeholder like `Style/RedundantParentheses`.
    const ORPHAN_BLOCKED_NO_GAP: &str = "\
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Foo
//! status: blocked
//! notes: blah
//! ```
";

    const ORPHAN_BLOCKED_WITH_GAP: &str = "\
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Foo
//! status: blocked
//! gap_issues:
//!   - murphy-wojl
//! notes: blah
//! ```
";

    #[test]
    fn orphan_blocked_block_without_gap_issues_is_flagged() {
        // An orphan parity block (no registered cop) must still be validated,
        // not silently skipped — a `blocked` placeholder needs its gap_issues.
        let mut failures = Vec::new();
        collect_parity_failures(
            ORPHAN_BLOCKED_NO_GAP,
            "redundant_parentheses.rs",
            &mut failures,
        );
        assert!(
            failures.iter().any(|f| f.contains("gap_issues")),
            "expected a gap_issues failure for a blocked orphan, got {failures:?}"
        );
    }

    #[test]
    fn orphan_blocked_block_with_gap_issues_is_accepted() {
        // `blocked` is a recognised status and, with gap_issues present, the
        // orphan placeholder validates cleanly.
        let mut failures = Vec::new();
        collect_parity_failures(
            ORPHAN_BLOCKED_WITH_GAP,
            "redundant_parentheses.rs",
            &mut failures,
        );
        assert!(
            failures.is_empty(),
            "blocked orphan with gap_issues should pass, got {failures:?}"
        );
    }

    #[test]
    fn orphan_block_with_unknown_status_is_flagged() {
        let source = "\
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Foo
//! status: nonsense
//! gap_issues:
//!   - murphy-wojl
//! ```
";
        let mut failures = Vec::new();
        collect_parity_failures(source, "x.rs", &mut failures);
        assert!(
            failures.iter().any(|f| f.contains("unknown status")),
            "expected an unknown-status failure, got {failures:?}"
        );
    }

    #[test]
    fn registered_blocked_status_is_known_but_needs_gap_issues() {
        // A registered cop using `status: blocked` is recognised (not flagged
        // as "unknown status") but, like partial/stub, must list gap_issues.
        let source = "\
#[cop(name = \"Style/Foo\")]
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Foo
//! upstream_version_checked: 1.86.2
//! status: blocked
//! notes: blah
//! ```
";
        let mut failures = Vec::new();
        collect_parity_failures(source, "x.rs", &mut failures);
        assert_eq!(
            failures,
            ["x.rs: blocked murphy-parity block for Style/Foo must list gap_issues"],
            "`blocked` must be a known status that still requires gap_issues"
        );
    }

    #[test]
    fn stray_block_alongside_registered_cop_is_validated_as_orphan() {
        // A parity block matching no registered cop, in a file that DOES
        // register a cop, must still be validated — not silently skipped.
        let source = "\
#[cop(name = \"Style/Foo\")]
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Foo
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! ```
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Bar
//! status: blocked
//! ```
";
        let mut failures = Vec::new();
        collect_parity_failures(source, "x.rs", &mut failures);
        assert_eq!(
            failures,
            ["x.rs: blocked murphy-parity block for Style/Bar must list gap_issues"],
            "the stray Style/Bar block must be validated as an orphan"
        );
    }

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

    /// Fixture: emits the same offense as `FixedRangeCop` and replaces
    /// that range with `xyz`.
    #[derive(Default)]
    struct CorrectingCop;
    impl Cop for CorrectingCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/Correcting";
    }
    impl NodeCop for CorrectingCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 0, end: 3 }, "fixed", None);
            cx.emit_edit(Range { start: 0, end: 3 }, "xyz");
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

    /// Fixture: emits one offense per visited `Send` node, gated by
    /// `SEND_METHODS = ["target"]`. Exercises the test harness's
    /// `send_method_filter_passes` path so dispatch in tests matches the
    /// host pre-filter for cops authored as
    /// `#[on_node(kind = "send", methods = [...])]`.
    #[derive(Default)]
    struct SendMethodFilteredCop;
    impl Cop for SendMethodFilteredCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/SendMethodFiltered";
    }
    impl NodeCop for SendMethodFilteredCop {
        const KINDS: &'static [NodeKindTag] = &[NodeKindTag(17)];
        const SEND_METHODS: &'static [RawSlice] = &[RawSlice::from_str("target")];
        fn check(&self, node: NodeId, cx: &Cx<'_>) {
            if matches!(*cx.kind(node), NodeKind::Send { .. }) {
                cx.emit_offense(cx.range(node), "called", None);
            }
        }
    }

    /// Fixture: emits an offense on every Send/Csend whose
    /// `call_operator_loc` resolves — covering the M1 accessor end-to-end
    /// against real prism-parsed byte offsets (not the synthetic ranges
    /// in `cx::tests`).
    #[derive(Default)]
    struct ReportCallOperatorCop;
    impl Cop for ReportCallOperatorCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/ReportCallOperator";
    }
    impl NodeCop for ReportCallOperatorCop {
        const KINDS: &'static [NodeKindTag] = &[NodeKindTag(17), NodeKindTag(18)];
        fn check(&self, node: NodeId, cx: &Cx<'_>) {
            if let Some(range) = cx.call_operator_loc(node) {
                cx.emit_offense(range, cx.raw_source(range), None);
            }
        }
    }

    #[test]
    fn call_operator_loc_resolves_against_real_prism_offsets() {
        // Plain dot: receiver `foo` → selector `bar`, operator `.` at 3..4.
        let offenses = super::run_cop::<ReportCallOperatorCop>("foo.bar\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 3, end: 4 });
        assert_eq!(offenses[0].message, ".");

        // Safe navigation: `foo&.bar`, operator `&.` at 3..5.
        let offenses = super::run_cop::<ReportCallOperatorCop>("foo&.bar\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 3, end: 5 });
        assert_eq!(offenses[0].message, "&.");

        // Multi-line chain: `foo\n  .bar` — leading-dot style.
        let offenses = super::run_cop::<ReportCallOperatorCop>("foo\n  .bar\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 6, end: 7 });

        // Trailing-dot style: `foo.\n  bar` — dot stays on first line.
        let offenses = super::run_cop::<ReportCallOperatorCop>("foo.\n  bar\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 3, end: 4 });

        // Operator method (`+`) and bracket method (`[]`) do not emit.
        let offenses = super::run_cop::<ReportCallOperatorCop>("a + b\n");
        assert_eq!(offenses.len(), 0);
        let offenses = super::run_cop::<ReportCallOperatorCop>("a[b]\n");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn expect_no_offenses_passes_when_cop_emits_nothing() {
        super::test::<NoopCop>().expect_no_offenses("x = 1\n");
    }

    #[test]
    #[should_panic(expected = "expect_no_offenses found 1 offense(s)")]
    fn expect_no_offenses_panics_when_cop_emits() {
        super::test::<FixedRangeCop>().expect_no_offenses("abc\n");
    }

    #[test]
    fn run_cop_honors_send_method_filter() {
        let offenses = super::run_cop::<SendMethodFilteredCop>("target\nother\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 0, end: 6 });
    }

    #[test]
    fn expect_offense_matches_single_annotation() {
        super::test::<FixedRangeCop>().expect_offense(
            "abc\n\
             ^^^ fixed\n",
        );
    }

    #[test]
    fn expect_offense_empty_message_is_range_only_match() {
        // No message after the carets: range matches, message ignored.
        super::test::<FixedRangeCop>().expect_offense(
            "abc\n\
             ^^^\n",
        );
    }

    #[test]
    fn expect_correction_matches_offenses_and_corrected_source() {
        super::test::<CorrectingCop>().expect_correction(
            "abc\n\
             ^^^ fixed\n",
            "xyz\n",
        );
    }

    #[test]
    fn expect_no_corrections_passes_when_cop_emits_no_edits() {
        super::test::<FixedRangeCop>().expect_no_corrections("abc\n");
    }

    #[test]
    #[should_panic(expected = "expect_no_corrections found 1 edit(s)")]
    fn expect_no_corrections_panics_when_cop_emits_edits() {
        super::test::<CorrectingCop>().expect_no_corrections("abc\n");
    }

    #[test]
    fn run_cop_with_edits_captures_autocorrect_edits() {
        let captured = super::run_cop_with_edits::<CorrectingCop>("abc\n");
        assert_eq!(captured.offenses.len(), 1);
        assert_eq!(
            captured.edits,
            vec![super::CapturedEdit {
                range: Range { start: 0, end: 3 },
                replacement: "xyz".to_string(),
            }]
        );
    }

    #[test]
    #[should_panic(expected = "expect_correction corrected source mismatch")]
    fn expect_correction_panics_on_corrected_source_mismatch() {
        super::test::<CorrectingCop>().expect_correction(
            "abc\n\
             ^^^ fixed\n",
            "abc\n",
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense mismatch")]
    fn expect_offense_panics_on_extra_emit() {
        // TwoEmitCop emits 2 offenses; assert only 1. The comparator's
        // count-mismatch branch fires (this case is not intercepted by
        // the zero-annotation guard because the input has 1 annotation).
        super::test::<TwoEmitCop>().expect_offense(
            "abc\n\
             ^^^ one\n\
             def\n",
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense mismatch")]
    fn expect_offense_panics_on_missing_emit() {
        super::test::<NoopCop>().expect_offense(
            "abc\n\
             ^^^ wanted\n",
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense mismatch")]
    fn expect_offense_panics_on_range_mismatch() {
        // FixedRangeCop emits [0, 3); test asserts [0, 2).
        super::test::<FixedRangeCop>().expect_offense(
            "abc\n\
             ^^ fixed\n",
        );
    }

    #[test]
    #[should_panic(expected = "expect_offense mismatch")]
    fn expect_offense_panics_on_message_mismatch() {
        super::test::<FixedRangeCop>().expect_offense(
            "abc\n\
             ^^^ wrong message\n",
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
            super::test::<FixedRangeCop>().expect_offense(
                "abc\n\
                 ^^ wrong\n",
            );
        }));
        let err = result.expect_err("expected a panic");
        let msg: String = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&'static str>().map(|s| s.to_string()))
            .expect("panic payload was neither String nor &'static str");
        // Header
        assert!(msg.contains("expect_offense mismatch"), "msg: {msg}");
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
    fn render_stacks_multiple_annotations_under_same_source_line() {
        // The render path is symmetric to parse_annotated for the new
        // multi-annotation grammar: feeding two ranges that anchor to
        // the same source line produces two stacked `^...` lines under
        // it, in input order.
        let src = "abc\n";
        let items = vec![
            (Range { start: 0, end: 1 }, Some("first")),
            (Range { start: 2, end: 3 }, Some("second")),
        ];
        let rendered = render(src, &items);
        assert_eq!(rendered, "abc\n^ first\n  ^ second\n");
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
        super::test::<FirstCharCop>().expect_offense(
            "あい\n\
             ^ first char\n",
        );
    }

    #[test]
    #[should_panic(expected = "annotation precedes any source line")]
    fn expect_offense_panics_when_annotation_has_no_source_above() {
        super::test::<FixedRangeCop>().expect_offense("^^^ orphan\nabc\n");
    }

    /// Fixture: emits two offenses on the same source line — `[0, 3)`
    /// twice (overlapping on purpose). Pairs with the multi-annotation
    /// parser test: under a single source line, two `^^^` annotations
    /// must both be reported.
    #[derive(Default)]
    struct SameLineTwoEmitCop;
    impl Cop for SameLineTwoEmitCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/SameLineTwoEmit";
    }
    impl NodeCop for SameLineTwoEmitCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 0, end: 3 }, "first", None);
            cx.emit_offense(Range { start: 0, end: 3 }, "second", None);
        }
    }

    #[test]
    fn parse_annotated_accepts_multiple_annotations_per_source_line() {
        // Two consecutive `^^^` lines under one source line — the parser
        // anchors both to that line. This was previously rejected; it is
        // now the supported shape for cops that fire multiple offenses
        // on the same row.
        super::test::<SameLineTwoEmitCop>().expect_offense(
            "abc\n\
             ^^^ first\n\
             ^^^ second\n",
        );
    }

    /// Fixture: emits two offenses at *different* columns on the same
    /// source line — `[0, 1)` ('a') and `[2, 3)` ('c'). Motivating
    /// case: a cop like SpaceAroundOperators flagging both `+` and `*`
    /// in `a+b*c` as separate offenses on one row.
    #[derive(Default)]
    struct TwoColOnLineCop;
    impl Cop for TwoColOnLineCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/TwoColOnLine";
    }
    impl NodeCop for TwoColOnLineCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 0, end: 1 }, "left", None);
            cx.emit_offense(Range { start: 2, end: 3 }, "right", None);
        }
    }

    #[test]
    fn expect_offense_two_annotations_at_different_columns_same_line() {
        // Two offenses at distinct columns on one source line: the
        // sort+compare pipeline must handle them independently and the
        // annotations must stack correctly under the source.
        // Use a plain string (not `\` continuation) so leading whitespace
        // in the annotation lines is preserved literally.
        super::test::<TwoColOnLineCop>().expect_offense("abc\n^ left\n  ^ right\n");
    }

    // ---------- strict caret bounds (RuboCop parity) ----------
    //
    // RuboCop's `expect_offense` rejects caret runs that extend past the
    // last char of the source line. murphy historically clamped the caret
    // end to `line.len()` (lenient), silently passing over-padded
    // annotations. These tests pin the strict, insertion-aware bounds.

    /// Fixture: emits a zero-width offense at the end of the first source
    /// line's content (byte 3 of "abc"). Models an EOL insertion point
    /// (trailing comma, missing newline) annotated with a caret one column
    /// past the last char.
    #[derive(Default)]
    struct EolInsertionCop;
    impl Cop for EolInsertionCop {
        type Options = NoOptions;
        const NAME: &'static str = "Test/EolInsertion";
    }
    impl NodeCop for EolInsertionCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(Range { start: 3, end: 3 }, "insert here", None);
        }
    }

    #[test]
    #[should_panic(expected = "caret annotation overshoots source line")]
    fn expect_offense_panics_when_carets_overshoot_line() {
        // "abc" is 3 chars; 4 carets reach char 4, one past the last
        // char. The clamp used to hide this — RuboCop fails it, so must we.
        super::test::<FixedRangeCop>().expect_offense(
            "abc\n\
             ^^^^ fixed\n",
        );
    }

    #[test]
    #[should_panic(expected = "caret annotation overshoots source line")]
    fn expect_offense_panics_when_carets_overshoot_multibyte_line() {
        // "あい" is 2 chars / 6 bytes. 3 carets overshoot by one *char*;
        // a byte-based check would miss it. Guards char-unit counting.
        super::test::<FirstCharCop>().expect_offense(
            "あい\n\
             ^^^ first char\n",
        );
    }

    #[test]
    fn expect_offense_allows_eol_insertion_caret() {
        // A single caret one column past the last char marks a zero-width
        // EOL insertion point (e.g. "add trailing comma"). The strict
        // overshoot guard exempts exactly `start_char == line_char_count`
        // with `caret_len == 1` — nothing wider.
        // Plain string (not `\` continuation) to preserve leading spaces.
        super::test::<EolInsertionCop>().expect_offense("abc\n   ^ insert here\n");
    }

    #[test]
    #[should_panic(expected = "caret annotation overshoots source line")]
    fn expect_offense_panics_on_multi_caret_eol_insertion() {
        // Two carets at the EOL insertion point: a zero-width insertion is
        // a single caret, so width > 1 is over-padding. Both carets used to
        // clamp to [3, 3] and silently match EolInsertionCop's [3, 3].
        super::test::<EolInsertionCop>().expect_offense("abc\n   ^^ insert here\n");
    }

    #[test]
    #[should_panic(expected = "caret annotation overshoots source line")]
    fn expect_offense_panics_when_caret_starts_past_line_end() {
        // Carets beginning well past the last char (col 6 on a 3-char line)
        // used to clamp start AND end to [3, 3] and silently match an EOL
        // insertion offense — the same over-padding the guard removes.
        super::test::<EolInsertionCop>().expect_offense("abc\n      ^^^ insert here\n");
    }

    #[test]
    #[should_panic(
        expected = "expect_offense must contain at least one annotation; use expect_no_offenses instead"
    )]
    fn expect_offense_panics_when_input_has_no_annotations() {
        // Symmetric guard to expect_no_offenses_panics_on_caret_input:
        // the user picked the wrong expectation. Catching the typo prevents
        // a silent pass when the cop happens to emit nothing.
        super::test::<NoopCop>().expect_offense("abc\n");
    }

    #[test]
    #[should_panic(
        expected = "expect_no_offenses must not contain annotations; use expect_offense instead"
    )]
    fn expect_no_offenses_panics_on_caret_input() {
        // Misuse guard: annotations in expect_no_offenses are a typo
        // for the wrong expectation. Catching this saves silent test passes.
        super::test::<NoopCop>().expect_no_offenses(
            "abc\n\
             ^^^ stray\n",
        );
    }

    // ---------- per-test options JSON ----------
    //
    // The fixture below is the smallest cop that observably branches on
    // its `Options` struct so the `_with_options` family can pin the
    // end-to-end wiring: caller passes JSON → `cx_raw_for` parks it on
    // `CxRaw::options_json` → `cx.options::<T>()` decodes via serde →
    // cop branches.

    /// Minimal options struct hand-implemented (not via
    /// `#[derive(CopOptions)]`) because the derive emits absolute paths
    /// like `::murphy_plugin_api::CopOptions`, which do not resolve from
    /// inside the lib's own `#[cfg(test)]` module. Real cops are
    /// external crates and use the derive normally.
    #[derive(Default, Debug)]
    struct ToggleOptions {
        emit: bool,
    }

    impl crate::CopOptions for ToggleOptions {
        fn from_config_json(bytes: &[u8]) -> Result<Self, crate::ConfigError> {
            // Tiny hand-written decoder — `{"emit": true}` flips on, any
            // other shape falls back to the default. The production path
            // (via the derive) goes through serde_json field-by-field.
            let v: serde_json::Value =
                serde_json::from_slice(bytes).map_err(crate::ConfigError::parse)?;
            let emit = v
                .as_object()
                .and_then(|obj| obj.get("emit"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            Ok(ToggleOptions { emit })
        }

        fn to_config_json(&self) -> String {
            // Mirrors `from_config_json` — emits `{"emit": <self.emit>}`
            // so the typed-value test path round-trips through the same
            // wire shape the production derive would produce.
            format!("{{\"emit\":{}}}", self.emit)
        }
    }

    /// Fixture: emits one offense per visited node only when the
    /// `emit = true` option is set. Default behaviour is silent so the
    /// no-options test path stays no-offense.
    #[derive(Default)]
    struct OptionAwareCop;
    impl Cop for OptionAwareCop {
        type Options = ToggleOptions;
        const NAME: &'static str = "Test/OptionAware";
    }
    impl NodeCop for OptionAwareCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, cx: &Cx<'_>) {
            let opts = cx.options::<ToggleOptions>().unwrap_or_default();
            if opts.emit {
                cx.emit_offense(Range { start: 0, end: 3 }, "toggle", None);
                cx.emit_edit(Range { start: 0, end: 3 }, "xyz");
            }
        }
    }

    #[test]
    fn run_cop_default_options_yields_default_behavior() {
        let offenses = super::run_cop::<OptionAwareCop>("abc\n");
        assert!(offenses.is_empty(), "default options must produce no emit");
    }

    #[test]
    fn run_cop_with_options_typed_value_flips_behavior() {
        let offenses =
            super::run_cop_with_options::<OptionAwareCop>("abc\n", &ToggleOptions { emit: true });
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range, Range { start: 0, end: 3 });
        assert_eq!(offenses[0].message, "toggle");
    }

    #[test]
    fn expect_no_offenses_with_typed_options_keeps_default_branch_silent() {
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: false })
            .expect_no_offenses("abc\n");
    }

    #[test]
    fn expect_offense_with_typed_options_drives_non_default_branch() {
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .expect_offense(
                "abc\n\
                 ^^^ toggle\n",
            );
    }

    #[test]
    fn expect_correction_with_typed_options_applies_non_default_edits() {
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .expect_correction(
                "abc\n\
                 ^^^ toggle\n",
                "xyz\n",
            );
    }

    #[test]
    fn expect_no_corrections_with_typed_options_pins_silent_default() {
        // `emit = false` -> the cop emits nothing, so the edit list is
        // empty even though the caller could have asked for the alternate
        // behaviour.
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: false })
            .expect_no_corrections("abc\n");
    }

    // ---------- tester-builder API ----------

    #[test]
    fn tester_default_options_silent_when_emit_is_false() {
        super::test::<OptionAwareCop>().expect_no_offenses("abc\n");
    }

    #[test]
    fn tester_with_options_drives_non_default_branch() {
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .expect_offense(
                "abc\n\
                 ^^^ toggle\n",
            );
    }

    #[test]
    fn tester_chain_threads_options_through_multiple_expectations() {
        // Pins the chaining contract: with_options is set once and
        // every subsequent expectation observes the same options.
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .expect_offense(
                "abc\n\
                 ^^^ toggle\n",
            )
            .expect_correction(
                "abc\n\
                 ^^^ toggle\n",
                "xyz\n",
            );
    }

    #[test]
    fn tester_with_options_can_be_overwritten() {
        // Set `emit: true`, then immediately overwrite with `emit: false`
        // — the later call wins, and the no-offense expectation holds.
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .with_options(&ToggleOptions { emit: false })
            .expect_no_offenses("abc\n")
            .expect_no_corrections("abc\n");
    }

    #[test]
    #[should_panic(expected = "expect_offense must contain at least one annotation")]
    fn tester_expect_offense_without_annotations_panics() {
        // The tester must not silently pass on a malformed fixture.
        super::test::<OptionAwareCop>()
            .with_options(&ToggleOptions { emit: true })
            .expect_offense("abc\n");
    }
}
