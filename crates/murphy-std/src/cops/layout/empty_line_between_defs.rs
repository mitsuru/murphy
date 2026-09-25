//! `Layout/EmptyLineBetweenDefs` — require blank line(s) between consecutive
//! class / module / method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineBetweenDefs
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_begin`: walk each `begin` body's children in
//!   consecutive pairs; when both members are definition candidates
//!   (`def`/`defs`/`class`/`module`, gated by `EmptyLineBetweenMethodDefs` /
//!   `EmptyLineBetweenClassDefs` / `EmptyLineBetweenModuleDefs`, plus
//!   `DefLikeMacros` block/send macros), count the blank lines between the
//!   first def's end line and the second def's start line. If the count is
//!   outside `NumberOfEmptyLines` (an integer or a `[min, max]` allowance
//!   range), flag the second def unless `multiple_blank_lines_groups?`
//!   (blank lines split by a comment) or `AllowAdjacentOneLineDefs` (both
//!   single-line) applies. Offense location is `keyword..name` of the second
//!   def, or the whole node for macro sends/blocks (RuboCop's `def_location`).
//!
//!   Autocorrect inserts (up to the minimum) or removes (down to the maximum)
//!   blank lines at the first newline after the previous def's end (RuboCop's
//!   `autocorrect`), handling the same-line one-liner case by anchoring before
//!   the second def instead. Surplus removal deletes whole blank lines so
//!   trailing whitespace and intervening comments are preserved.
//! ```

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct EmptyLineBetweenDefs;

/// Options for [`EmptyLineBetweenDefs`].
///
/// Hand-rolled `CopOptions` impl because `NumberOfEmptyLines` accepts two
/// shapes (an integer or a `[min, max]` array), which
/// `#[derive(CopOptions)]` does not model. Follows the
/// `Layout/HashAlignment` precedent (see `hash_alignment.rs`).
#[derive(Clone, Debug)]
pub struct EmptyLineBetweenDefsOptions {
    pub method_defs: bool,
    pub class_defs: bool,
    pub module_defs: bool,
    pub allow_adjacent_one_line_defs: bool,
    pub minimum_empty_lines: i64,
    pub maximum_empty_lines: i64,
    pub def_like_macros: Vec<String>,
}

impl Default for EmptyLineBetweenDefsOptions {
    fn default() -> Self {
        Self {
            method_defs: true,
            class_defs: true,
            module_defs: true,
            allow_adjacent_one_line_defs: true,
            minimum_empty_lines: 1,
            maximum_empty_lines: 1,
            def_like_macros: Vec::new(),
        }
    }
}

fn decode_bool(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    default: bool,
) -> Result<bool, ConfigError> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ConfigError::type_mismatch(key, "bool")),
    }
}

fn decode_number_of_empty_lines(value: &serde_json::Value) -> Result<(i64, i64), ConfigError> {
    const KEY: &str = "NumberOfEmptyLines";
    if let Some(n) = value.as_i64() {
        return Ok((n, n));
    }
    if let Some(arr) = value.as_array() {
        if arr.is_empty() {
            return Err(ConfigError::type_mismatch(KEY, "int or array of ints"));
        }
        let mut nums = Vec::with_capacity(arr.len());
        for (i, elem) in arr.iter().enumerate() {
            let n = elem
                .as_i64()
                .ok_or_else(|| ConfigError::type_mismatch(format!("{KEY}[{i}]"), "int"))?;
            nums.push(n);
        }
        let min = nums[0];
        let max = nums[nums.len() - 1];
        return Ok((min, max));
    }
    Err(ConfigError::type_mismatch(KEY, "int or array of ints"))
}

impl CopOptions for EmptyLineBetweenDefsOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;
        let method_defs = decode_bool(obj, "EmptyLineBetweenMethodDefs", true)?;
        let class_defs = decode_bool(obj, "EmptyLineBetweenClassDefs", true)?;
        let module_defs = decode_bool(obj, "EmptyLineBetweenModuleDefs", true)?;
        let allow_adjacent_one_line_defs = decode_bool(obj, "AllowAdjacentOneLineDefs", true)?;
        let (minimum_empty_lines, maximum_empty_lines) = match obj.get("NumberOfEmptyLines") {
            None | Some(serde_json::Value::Null) => (1, 1),
            Some(v) => decode_number_of_empty_lines(v)?,
        };
        let def_like_macros = match obj.get("DefLikeMacros") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(v) => {
                let arr = v.as_array().ok_or_else(|| {
                    ConfigError::type_mismatch("DefLikeMacros", "array of strings")
                })?;
                let mut out = Vec::with_capacity(arr.len());
                for (i, elem) in arr.iter().enumerate() {
                    let s = elem.as_str().ok_or_else(|| {
                        ConfigError::type_mismatch(format!("DefLikeMacros[{i}]"), "string")
                    })?;
                    out.push(s.to_string());
                }
                out
            }
        };
        Ok(Self {
            method_defs,
            class_defs,
            module_defs,
            allow_adjacent_one_line_defs,
            minimum_empty_lines,
            maximum_empty_lines,
            def_like_macros,
        })
    }

    fn to_config_json(&self) -> String {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "EmptyLineBetweenMethodDefs".to_string(),
            serde_json::Value::Bool(self.method_defs),
        );
        obj.insert(
            "EmptyLineBetweenClassDefs".to_string(),
            serde_json::Value::Bool(self.class_defs),
        );
        obj.insert(
            "EmptyLineBetweenModuleDefs".to_string(),
            serde_json::Value::Bool(self.module_defs),
        );
        obj.insert(
            "AllowAdjacentOneLineDefs".to_string(),
            serde_json::Value::Bool(self.allow_adjacent_one_line_defs),
        );
        let number: serde_json::Value = if self.minimum_empty_lines == self.maximum_empty_lines {
            serde_json::Value::Number(serde_json::Number::from(self.minimum_empty_lines))
        } else {
            serde_json::Value::Array(vec![
                serde_json::Value::Number(serde_json::Number::from(self.minimum_empty_lines)),
                serde_json::Value::Number(serde_json::Number::from(self.maximum_empty_lines)),
            ])
        };
        obj.insert("NumberOfEmptyLines".to_string(), number);
        obj.insert(
            "DefLikeMacros".to_string(),
            serde_json::Value::Array(
                self.def_like_macros
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            ),
        );
        serde_json::Value::Object(obj).to_string()
    }
}

#[cop(
    name = "Layout/EmptyLineBetweenDefs",
    description = "Use empty lines between method, class, and module definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLineBetweenDefsOptions
)]
impl EmptyLineBetweenDefs {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) {
        let NodeKind::Begin(list) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(list);
        // RuboCop: `node.children.each_cons(2)`.
        for pair in children.windows(2) {
            let prev = pair[0];
            let cur = pair[1];
            if candidate(prev, cx, options) && candidate(cur, cx, options) {
                check_defs(prev, cur, cx, options);
            }
        }
    }
}

/// RuboCop `candidate?`: a method/class/module definition gated by the
/// corresponding enable flag, or a `DefLikeMacros` macro call.
fn candidate(node: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => options.method_defs,
        NodeKind::Class { .. } => options.class_defs,
        NodeKind::Module { .. } => options.module_defs,
        NodeKind::Send { .. }
        | NodeKind::Block { .. }
        | NodeKind::Numblock { .. }
        | NodeKind::Itblock { .. } => macro_candidate(node, cx, options),
        _ => false,
    }
}

/// RuboCop `macro_candidate?`: a receiverless macro call whose method name is
/// listed in `DefLikeMacros`. Block-family nodes unwrap to their send.
fn macro_candidate(node: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) -> bool {
    if options.def_like_macros.is_empty() {
        return false;
    }
    let send = if cx.is_any_block_type(node) {
        match cx.block_call(node).get() {
            Some(call) => call,
            None => return false,
        }
    } else if matches!(*cx.kind(node), NodeKind::Send { .. }) {
        node
    } else {
        return false;
    };
    if !cx.is_macro(send) {
        return false;
    }
    match cx.method_name(send) {
        Some(name) => options.def_like_macros.iter().any(|m| m == name),
        None => false,
    }
}

/// RuboCop `check_defs`.
fn check_defs(prev: NodeId, cur: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) {
    let count = blank_lines_count_between(prev, cur, cx);
    let min = options.minimum_empty_lines.max(0) as usize;
    let max = options.maximum_empty_lines.max(0) as usize;
    // RuboCop orders the range as written; a reversed `[2, 0]` still covers
    // `min..=max` after normalising.
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };

    // RuboCop: `return if line_count_allowed?(count)`.
    if (lo..=hi).contains(&count) {
        return;
    }
    // RuboCop: `return if multiple_blank_lines_groups?(*nodes)`.
    if multiple_blank_lines_groups(prev, cur, cx) {
        return;
    }
    // RuboCop: `return if nodes.all?(&:single_line?) && AllowAdjacentOneLineDefs`.
    if options.allow_adjacent_one_line_defs && is_single_line(prev, cx) && is_single_line(cur, cx) {
        return;
    }

    let location = def_location(cur, cx);
    let message = format!(
        "Expected {} between {} definitions; found {count}.",
        expected_lines(lo, hi),
        node_type(cur, cx),
    );
    cx.emit_offense(location, &message, None);
    autocorrect(prev, cur, count, lo, hi, cx);
}

/// RuboCop `def_location`: `loc.keyword.join(loc.name)` for def-like nodes;
/// the whole node for macro sends/blocks.
fn def_location(node: NodeId, cx: &Cx<'_>) -> Range {
    if matches!(
        *cx.kind(node),
        NodeKind::Send { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
    ) {
        return cx.range(node);
    }
    let loc = cx.loc(node);
    let name_end = loc.name.end;
    let start = cx.range(node).start;
    if name_end > start {
        Range {
            start,
            end: name_end,
        }
    } else {
        // Fallback: keyword token only (e.g. anonymous shape with no name loc).
        let keyword = loc.keyword();
        if keyword != Range::ZERO {
            keyword
        } else {
            Range { start, end: start }
        }
    }
}

/// RuboCop `node_type`: defs map to `method`, numblock/itblock/block to
/// `block`, send to `send`; everything else uses its type.
fn node_type(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => "method",
        NodeKind::Class { .. } => "class",
        NodeKind::Module { .. } => "module",
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => "block",
        NodeKind::Send { .. } | NodeKind::Csend { .. } => "send",
        _ => "definition",
    }
}

/// RuboCop `expected_lines`: a range renders `min..max`, otherwise a fixed
/// count with singular/plural handling.
fn expected_lines(min: usize, max: usize) -> String {
    if min != max {
        return format!("{min}..{max} empty lines");
    }
    let lines = if max == 1 { "line" } else { "lines" };
    format!("{max} empty {lines}")
}

/// True iff the node occupies a single source line.
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let src = cx.source().as_bytes();
    !src[range.start as usize..range.end as usize].contains(&b'\n')
}

/// 1-based source line number containing byte `offset`.
fn line_of(src: &str, offset: usize) -> usize {
    let off = offset.min(src.len());
    src.as_bytes()[..off]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// RuboCop `def_start`: the first line of the definition. Block-family nodes
/// start at their send; sends start at their own line; everything else starts
/// at the keyword line (== range start line).
fn def_start_line(node: NodeId, cx: &Cx<'_>) -> usize {
    let src = cx.source();
    if cx.is_any_block_type(node)
        && let Some(call) = cx.block_call(node).get()
    {
        return line_of(src, cx.range(call).start as usize);
    }
    line_of(src, cx.range(node).start as usize)
}

/// RuboCop `def_end` (`end_loc(node).line`): the last line of the definition.
fn def_end_line(node: NodeId, cx: &Cx<'_>) -> usize {
    let src = cx.source();
    let end = cx.range(node).end as usize;
    if end == 0 {
        return 1;
    }
    // `range.end` is exclusive; the last byte of the node decides its line.
    line_of(src, end.saturating_sub(1))
}

/// RuboCop `lines_between_defs`: physical lines strictly between the two defs.
fn lines_between_defs<'a>(prev: NodeId, cur: NodeId, cx: &Cx<'_>, src: &'a str) -> Vec<&'a str> {
    let all: Vec<&str> = src.lines().collect();
    if all.is_empty() {
        return Vec::new();
    }
    let begin = def_end_line(prev, cx);
    let end_def_start = def_start_line(cur, cx);
    if end_def_start < 2 {
        return Vec::new();
    }
    // 0-based slice: start just after `prev`'s last line, end just before
    // `cur`'s first line. `begin` is 1-based, so it doubles as the 0-based
    // start index; `end_def_start - 1` is the exclusive 0-based end.
    let start_idx = begin.min(all.len());
    let end_idx = (end_def_start - 1).min(all.len());
    if end_idx <= start_idx {
        return Vec::new();
    }
    all[start_idx..end_idx].to_vec()
}

/// RuboCop `multiple_blank_lines_groups?`: skip when blank lines between the
/// defs are split by a comment (the last blank comes after the first
/// non-blank line).
fn multiple_blank_lines_groups(prev: NodeId, cur: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.source();
    let lines = lines_between_defs(prev, cur, cx, src);
    if lines.is_empty() {
        return false;
    }
    let is_blank = |line: &str| line.bytes().all(crate::cops::util::is_ruby_blank_byte);
    let blank_start = lines.iter().rposition(|l| is_blank(l));
    let non_blank_end = lines.iter().position(|l| !is_blank(l));
    match (blank_start, non_blank_end) {
        (Some(b), Some(n)) => b > n,
        _ => false,
    }
}

/// RuboCop `blank_lines_count_between`: blank lines strictly between the first
/// def's end line and the second def's start line.
fn blank_lines_count_between(prev: NodeId, cur: NodeId, cx: &Cx<'_>) -> usize {
    let src = cx.source().as_bytes();
    // The region between `prev`'s end and `cur`'s start.
    let between_start = cx.range(prev).end as usize;
    let cur_start = cx.range(cur).start as usize;
    if cur_start <= between_start {
        return 0;
    }

    // RuboCop counts whole physical lines that lie strictly between the two
    // definitions and are blank. The first newline after `prev` ends `prev`'s
    // last line; the line containing `cur`'s start is `cur`'s first line.
    // Lines in between are the candidates.
    let region = &src[between_start..cur_start];
    // The slice starts mid-line (right after `prev`'s last token). Skip to the
    // first newline so we begin counting on the line *after* `prev`.
    let Some(first_nl) = region.iter().position(|&b| b == b'\n') else {
        return 0;
    };
    let mut line_start = first_nl + 1;
    let mut count = 0usize;
    while line_start < region.len() {
        let line_end = region[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(region.len(), |i| line_start + i);
        // The final partial line (no trailing newline before `cur`) is `cur`'s
        // own line and must not be counted.
        if line_end >= region.len() {
            break;
        }
        if region[line_start..line_end]
            .iter()
            .all(|&b| crate::cops::util::is_ruby_blank_byte(b))
        {
            count += 1;
        }
        line_start = line_end + 1;
    }
    count
}

/// RuboCop `autocorrect`: anchor at the first newline after `prev`'s end, then
/// remove surplus (down to the maximum) or insert missing (up to the minimum)
/// blank lines.
fn autocorrect(prev: NodeId, cur: NodeId, count: usize, min: usize, max: usize, cx: &Cx<'_>) {
    let src = cx.source().as_bytes();
    let end_pos = cx.range(prev).end as usize;
    let Some(rel) = src[end_pos..].iter().position(|&b| b == b'\n') else {
        return;
    };
    let newline_pos = end_pos + rel;
    let begin_pos = cx.range(cur).start as usize;
    // The blank-line region begins just after the newline that terminates
    // `prev`'s last line. For same-line one-liners (`def a; end; def b; end`)
    // the first newline lies *after* `cur` starts, so anchor at `cur`'s start
    // instead. This is the line below which blank lines are removed / above
    // which they are inserted.
    let same_line = newline_pos > begin_pos;
    let region_start = if same_line {
        begin_pos
    } else {
        newline_pos + 1
    };

    if count > max {
        // Remove `count - max` *blank* physical lines that lie between the
        // two definitions. Each removed line is deleted whole (line start to
        // next line start, including any spaces/tabs), so trailing whitespace is
        // never merged onto an adjacent line. Non-blank lines (e.g. comments
        // between the defs) are skipped, so only surplus blank lines are
        // removed — a contiguous range removal would otherwise eat an
        // intervening comment line.
        let difference = count - max;
        // Blank lines start at `region_start`; stop at `cur`'s line.
        let mut pos = region_start;
        let cur_start = begin_pos;
        let mut removed = 0usize;
        while removed < difference && pos < cur_start {
            let line_end = src[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(src.len(), |i| pos + i);
            let next_line_start = if line_end < src.len() {
                line_end + 1
            } else {
                line_end
            };
            let is_blank = src[pos..line_end]
                .iter()
                .all(|&b| crate::cops::util::is_ruby_blank_byte(b));
            if is_blank {
                cx.emit_edit(
                    Range {
                        start: pos as u32,
                        end: next_line_start as u32,
                    },
                    "",
                );
                removed += 1;
            }
            pos = next_line_start;
        }
    } else {
        // Insert missing blank lines at the blank-line region start. If both
        // definitions share a line, an additional newline is needed to move the
        // second definition onto its own line before adding blank lines.
        let difference = min.saturating_sub(count);
        let newlines = difference + usize::from(same_line);
        let anchor = Range {
            start: region_start as u32,
            end: region_start as u32,
        };
        cx.emit_edit(anchor, &"\n".repeat(newlines));
    }
}

murphy_plugin_api::submit_cop!(EmptyLineBetweenDefs);

#[cfg(test)]
mod tests {
    use super::{EmptyLineBetweenDefs, EmptyLineBetweenDefsOptions};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{
        indoc, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits, test,
    };

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        // Apply edits right-to-left so earlier offsets stay valid.
        let mut sorted: Vec<_> = edits.iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut out = source.to_string();
        for edit in sorted {
            out.replace_range(
                edit.range.start as usize..edit.range.end as usize,
                &edit.replacement,
            );
        }
        out
    }

    fn opts_with_number(value: serde_json::Value) -> EmptyLineBetweenDefsOptions {
        let json = serde_json::json!({
            "EmptyLineBetweenMethodDefs": true,
            "EmptyLineBetweenClassDefs": true,
            "EmptyLineBetweenModuleDefs": true,
            "AllowAdjacentOneLineDefs": true,
            "NumberOfEmptyLines": value,
            "DefLikeMacros": [],
        });
        EmptyLineBetweenDefsOptions::from_config_json(json.to_string().as_bytes()).unwrap()
    }

    fn opts_with_macros(macros: &[&str], allow_adjacent: bool) -> EmptyLineBetweenDefsOptions {
        EmptyLineBetweenDefsOptions {
            def_like_macros: macros.iter().map(|s| s.to_string()).collect(),
            allow_adjacent_one_line_defs: allow_adjacent,
            ..Default::default()
        }
    }

    // ── Clean ────────────────────────────────────────────────────────────────

    #[test]
    fn accepts_blank_line_between_methods() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a
            end

            def b
            end
        "#});
    }

    #[test]
    fn accepts_single_method() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a
            end
        "#});
    }

    #[test]
    fn accepts_blank_line_between_classes() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            class A
            end

            class B
            end
        "#});
    }

    #[test]
    fn accepts_adjacent_one_line_defs() {
        // AllowAdjacentOneLineDefs default true.
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a; end
            def b; end
        "#});
    }

    // ── Offenses ─────────────────────────────────────────────────────────────

    #[test]
    fn flags_missing_blank_line_between_methods() {
        let offenses =
            murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            def a
            end
            def b
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between method definitions; found 0."
        );
    }

    #[test]
    fn corrects_missing_blank_line_between_methods() {
        let src = "def a\nend\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\ndef b\nend\n");
    }

    #[test]
    fn corrects_same_line_one_line_defs_when_adjacency_is_disabled() {
        let src = "def a; end; def b; end\n";
        let options = EmptyLineBetweenDefsOptions {
            allow_adjacent_one_line_defs: false,
            ..Default::default()
        };
        let run = run_cop_with_options_and_edits::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(run.offenses.len(), 1);
        let corrected = apply(src, &run.edits);
        assert_eq!(corrected, "def a; end; \n\ndef b; end\n");
        assert!(run_cop_with_options::<EmptyLineBetweenDefs>(&corrected, &options).is_empty());
    }

    #[test]
    fn flags_missing_blank_line_between_classes() {
        let offenses =
            murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            class A
            end
            class B
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between class definitions; found 0."
        );
    }

    #[test]
    fn flags_missing_blank_line_between_modules() {
        let offenses =
            murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            module A
            end
            module B
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between module definitions; found 0."
        );
    }

    #[test]
    fn flags_too_many_blank_lines() {
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(
            "def a\nend\n\n\ndef b\nend\n",
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between method definitions; found 2."
        );
    }

    #[test]
    fn corrects_too_many_blank_lines() {
        let src = "def a\nend\n\n\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\ndef b\nend\n");
    }

    #[test]
    fn corrects_too_many_blank_lines_with_whitespace() {
        // The excess blank line contains spaces. Removal must drop the whole
        // line (whitespace + newline), not merge the spaces onto an adjacent
        // line and leave trailing whitespace behind.
        let src = "def a\nend\n  \n\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1);
        let corrected = apply(src, &run.edits);
        assert_eq!(corrected, "def a\nend\n\ndef b\nend\n");
        // Sanity: no line carries stray trailing whitespace.
        assert!(
            !corrected.contains("  \n") && !corrected.contains("end  "),
            "corrected output has stray trailing whitespace: {corrected:?}"
        );
    }

    #[test]
    fn corrects_too_many_blank_lines_preserves_comment() {
        // Surplus blanks before a comment (single blank group) still flag.
        // The autocorrect must remove only the surplus blank line and keep the
        // comment line intact.
        let src = "def a\nend\n\n\n# c\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        let corrected = apply(src, &run.edits);
        assert_eq!(corrected, "def a\nend\n\n# c\ndef b\nend\n");
        assert!(corrected.contains("# c"), "comment must be preserved");
    }

    // ── NumberOfEmptyLines allowance range ───────────────────────────────────

    #[test]
    fn allows_zero_or_one_blank_lines_with_range() {
        let options = opts_with_number(serde_json::json!([0, 1]));
        assert!(
            run_cop_with_options::<EmptyLineBetweenDefs>("def a\nend\ndef b\nend\n", &options)
                .is_empty()
        );
        assert!(
            run_cop_with_options::<EmptyLineBetweenDefs>("def a\nend\n\ndef b\nend\n", &options)
                .is_empty()
        );
    }

    #[test]
    fn flags_two_blank_lines_with_range_and_reports_range() {
        let options = opts_with_number(serde_json::json!([0, 1]));
        let offenses =
            run_cop_with_options::<EmptyLineBetweenDefs>("def a\nend\n\n\ndef b\nend\n", &options);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 0..1 empty lines between method definitions; found 2."
        );
    }

    #[test]
    fn corrects_two_blank_lines_down_to_maximum() {
        let options = opts_with_number(serde_json::json!([0, 1]));
        let src = "def a\nend\n\n\ndef b\nend\n";
        let run = run_cop_with_options_and_edits::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\ndef b\nend\n");
    }

    #[test]
    fn requires_two_blank_lines_with_fixed_count() {
        let options = opts_with_number(serde_json::json!(2));
        let offenses =
            run_cop_with_options::<EmptyLineBetweenDefs>("def a\nend\n\ndef b\nend\n", &options);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 2 empty lines between method definitions; found 1."
        );
        let src = "def a\nend\n\ndef b\nend\n";
        let run = run_cop_with_options_and_edits::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\n\ndef b\nend\n");
    }

    #[test]
    fn number_of_empty_lines_roundtrips_through_json() {
        let options = opts_with_number(serde_json::json!([0, 1]));
        let json = options.to_config_json();
        let back = EmptyLineBetweenDefsOptions::from_config_json(json.as_bytes()).unwrap();
        assert_eq!(back.minimum_empty_lines, 0);
        assert_eq!(back.maximum_empty_lines, 1);
        let single = opts_with_number(serde_json::json!(2));
        let json = single.to_config_json();
        let back = EmptyLineBetweenDefsOptions::from_config_json(json.as_bytes()).unwrap();
        assert_eq!((back.minimum_empty_lines, back.maximum_empty_lines), (2, 2));
    }

    // ── multiple_blank_lines_groups ──────────────────────────────────────────

    #[test]
    fn skips_offense_when_blanks_are_split_by_comment() {
        // Two blank lines with a comment between them: the blank groups are
        // split, so RuboCop skips (autocorrect would be ambiguous).
        let src = "def a\nend\n\n# c\n\ndef b\nend\n";
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn still_flags_comment_only_gap() {
        // Only a comment between the defs (no split blank groups) still flags.
        let src = "def a\nend\n# c\ndef b\nend\n";
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    // ── DefLikeMacros ────────────────────────────────────────────────────────

    #[test]
    fn flags_missing_blank_line_between_macro_blocks() {
        let options = opts_with_macros(&["foo"], true);
        let src = "foo \'first\' do\nend\nfoo \'second\' do\nend\n";
        let offenses = run_cop_with_options::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between block definitions; found 0."
        );
    }

    #[test]
    fn corrects_missing_blank_line_between_macro_blocks() {
        let options = opts_with_macros(&["foo"], true);
        let src = "foo \'first\' do\nend\nfoo \'second\' do\nend\n";
        let run = run_cop_with_options_and_edits::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "foo \'first\' do\nend\n\nfoo \'second\' do\nend\n"
        );
    }

    #[test]
    fn ignores_unlisted_macro_names() {
        let options = opts_with_macros(&["foo"], true);
        let src = "bar \'a\' do\nend\nbar \'b\' do\nend\n";
        assert!(run_cop_with_options::<EmptyLineBetweenDefs>(src, &options).is_empty());
    }

    #[test]
    fn flags_bare_macro_sends_when_adjacency_disabled() {
        let options = opts_with_macros(&["foo"], false);
        let src = "foo :a\nfoo :b\n";
        let offenses = run_cop_with_options::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between send definitions; found 0."
        );
    }

    #[test]
    fn allows_adjacent_one_line_macro_sends_by_default() {
        let options = opts_with_macros(&["foo"], true);
        let src = "foo :a\nfoo :b\n";
        assert!(run_cop_with_options::<EmptyLineBetweenDefs>(src, &options).is_empty());
    }

    #[test]
    fn flags_mixed_def_and_macro() {
        let options = opts_with_macros(&["foo"], true);
        let src = "def a\nend\nfoo \'b\' do\nend\n";
        let offenses = run_cop_with_options::<EmptyLineBetweenDefs>(src, &options);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between block definitions; found 0."
        );
    }
}
