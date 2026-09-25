//! `Layout/HashAlignment` — aligns keys, separators, and values of a multi-line hash.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/HashAlignment
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_hash` + `HashAlignmentStyles` spine. Supports
//!   `EnforcedHashRocketStyle` / `EnforcedColonStyle` (`key`/`separator`/`table`,
//!   each accepting RuboCop's string-or-array `AllowMultipleStyles` shape),
//!   `EnforcedLastArgumentHashStyle`
//!   (`always_inspect`/`always_ignore`/`ignore_explicit`/`ignore_implicit`),
//!   the separator/value column deltas for the `key` style, `KeywordSplatAlignment`
//!   (`**rest` aligns with the first pair's key), and autocorrect via
//!   `AlignmentCorrector`-compatible space insertion/removal before the
//!   key/separator/value ranges.
//!
//!   Single-surface ABI blockers (intentionally NOT bypassed):
//!     * RuboCop's `autocorrect_incompatible_with_other_cops?` reads
//!       `Layout/ArgumentAlignment` `EnforcedStyle == with_fixed_indentation`
//!       to suppress the check when the first pair shares the selector line.
//!       Murphy's per-cop `CopOptions` surface cannot see a sibling cop's
//!       config, so the check always runs (the common case, since
//!       `with_fixed_indentation` is not the ArgumentAlignment default).
//! ```
//!
//! ## Matched shapes
//!
//! Multi-line `hash` nodes with two or more pairs where a later pair (or
//! `kwsplat`) begins its own line at a column inconsistent with the configured
//! style, plus first-pair separator/value spacing for `key`/`table` styles.

use murphy_plugin_api::{ConfigError, CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const KEY_MSG: &str = "Align the keys of a hash literal if they span more than one line.";
const SEPARATOR_MSG: &str =
    "Align the separators of a hash literal if they span more than one line.";
const TABLE_MSG: &str =
    "Align the keys and values of a hash literal if they span more than one line.";
const KWSPLAT_MSG: &str =
    "Align keyword splats with the rest of the hash if it spans more than one line.";

/// Alignment style for `EnforcedHashRocketStyle` / `EnforcedColonStyle`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashAlignmentStyle {
    #[option(value = "key")]
    Key,
    #[option(value = "separator")]
    Separator,
    #[option(value = "table")]
    Table,
}

/// Treatment of a hash passed as a method's last argument.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LastArgumentHashStyle {
    #[option(value = "always_inspect")]
    AlwaysInspect,
    #[option(value = "always_ignore")]
    AlwaysIgnore,
    #[option(value = "ignore_implicit")]
    IgnoreImplicit,
    #[option(value = "ignore_explicit")]
    IgnoreExplicit,
}

/// Options for [`HashAlignment`].
///
/// `EnforcedHashRocketStyle` / `EnforcedColonStyle` accept RuboCop's two
/// shapes: a single string (`"key"`) or an array of strings
/// (`["key", "separator"]`) for `AllowMultipleStyles`. Decoding dedups while
/// preserving order (RuboCop's `formats.uniq`).
#[derive(Clone, Debug)]
pub struct HashAlignmentOptions {
    pub enforced_hash_rocket_style: Vec<HashAlignmentStyle>,
    pub enforced_colon_style: Vec<HashAlignmentStyle>,
    pub enforced_last_argument_hash_style: LastArgumentHashStyle,
}

impl Default for HashAlignmentOptions {
    fn default() -> Self {
        Self {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Key],
            enforced_colon_style: vec![HashAlignmentStyle::Key],
            enforced_last_argument_hash_style: LastArgumentHashStyle::AlwaysInspect,
        }
    }
}

fn decode_style_list(value: &serde_json::Value, field: &str) -> Result<Vec<HashAlignmentStyle>, ConfigError> {
    let strs: Vec<&str> = if let Some(s) = value.as_str() {
        vec![s]
    } else if let Some(arr) = value.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        for (i, elem) in arr.iter().enumerate() {
            let s = elem.as_str().ok_or_else(|| {
                ConfigError::type_mismatch(format!("{field}[{i}]"), "string")
            })?;
            out.push(s);
        }
        out
    } else {
        return Err(ConfigError::type_mismatch(field, "string or array of strings"));
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in strs {
        let style = HashAlignmentStyle::from_str(s).ok_or_else(|| {
            ConfigError::enum_violation(field, s)
        })?;
        // `formats.uniq` — dedup preserving first-seen order.
        let key = style as u8;
        if seen.insert(key) {
            out.push(style);
        }
    }
    Ok(out)
}

impl CopOptions for HashAlignmentOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;
        let mut opts = Self::default();
        if let Some(v) = obj.get("EnforcedHashRocketStyle") {
            if v.is_null() {
                // `~` (null) falls back to default, mirroring IndentationWidth handling.
            } else {
                opts.enforced_hash_rocket_style = decode_style_list(v, "EnforcedHashRocketStyle")?;
            }
        }
        if let Some(v) = obj.get("EnforcedColonStyle") {
            if v.is_null() {
            } else {
                opts.enforced_colon_style = decode_style_list(v, "EnforcedColonStyle")?;
            }
        }
        if let Some(v) = obj.get("EnforcedLastArgumentHashStyle") {
            if v.is_null() {
            } else {
                let s = v.as_str().ok_or_else(|| {
                    ConfigError::type_mismatch("EnforcedLastArgumentHashStyle", "string")
                })?;
                opts.enforced_last_argument_hash_style =
                    LastArgumentHashStyle::from_str(s).ok_or_else(|| {
                        ConfigError::enum_violation("EnforcedLastArgumentHashStyle", s)
                    })?;
            }
        }
        Ok(opts)
    }

    fn to_config_json(&self) -> String {
        fn encode_list(items: &[HashAlignmentStyle]) -> serde_json::Value {
            if items.len() == 1 {
                serde_json::Value::String(items[0].as_str().to_string())
            } else {
                serde_json::Value::Array(
                    items.iter().map(|s| serde_json::Value::String(s.as_str().to_string())).collect(),
                )
            }
        }
        let mut obj = serde_json::Map::new();
        obj.insert(
            "EnforcedHashRocketStyle".to_string(),
            encode_list(&self.enforced_hash_rocket_style),
        );
        obj.insert(
            "EnforcedColonStyle".to_string(),
            encode_list(&self.enforced_colon_style),
        );
        obj.insert(
            "EnforcedLastArgumentHashStyle".to_string(),
            serde_json::Value::String(self.enforced_last_argument_hash_style.as_str().to_string()),
        );
        serde_json::Value::Object(obj).to_string()
    }
}

#[derive(Default)]
pub struct HashAlignment;

#[cop(
    name = "Layout/HashAlignment",
    description = "Align the keys of a multi-line hash literal.",
    default_severity = "warning",
    default_enabled = true,
    options = HashAlignmentOptions,
)]
impl HashAlignment {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Alignment family, mirroring `HashAlignmentStyles::*Alignment` classes.
/// `Kwsplat` is `KeywordSplatAlignment`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum AlignKind {
    Key,
    Separator,
    Table,
    Kwsplat,
}

impl AlignKind {
    fn message(self) -> &'static str {
        match self {
            AlignKind::Key => KEY_MSG,
            AlignKind::Separator => SEPARATOR_MSG,
            AlignKind::Table => TABLE_MSG,
            AlignKind::Kwsplat => KWSPLAT_MSG,
        }
    }
}

/// Column deltas for one pair: positive means "needs spaces inserted before",
/// negative means "needs spaces removed before". All-zero is good alignment.
#[derive(Clone, Copy, Debug, Default)]
struct Deltas {
    key: i32,
    separator: i32,
    value: i32,
}

impl Deltas {
    fn is_good(self) -> bool {
        self.key == 0 && self.separator == 0 && self.value == 0
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<HashAlignmentOptions>();
    let pairs = cx.hash_pairs(node);
    // `return if ... node.pairs.empty? || node.single_line?`.
    if pairs.is_empty() || is_single_line(node, cx) {
        return;
    }
    // `EnforcedLastArgumentHashStyle` — a hash passed as the last argument.
    if is_ignored_last_argument(node, cx, &opts) {
        return;
    }
    let Some(&first_pair) = pairs.first() else {
        return;
    };

    // `return unless alignment_for_hash_rockets.any?(checkable) &&
    // alignment_for_colons.any?(checkable)`.
    let rocket_checkable = opts
        .enforced_hash_rocket_style
        .iter()
        .any(|s| is_checkable(*s, &pairs, cx));
    let colon_checkable = opts
        .enforced_colon_style
        .iter()
        .any(|s| is_checkable(*s, &pairs, cx));
    if !(rocket_checkable && colon_checkable) {
        return;
    }

    // Table metrics are hash-wide (max key/delimiter widths).
    let table_info = TableInfo::compute(&pairs, cx, first_pair);

    // `offenses_by` + `column_deltas`, preserving first-seen order for
    // `min_by` tie-breaking (RuboCop picks the format with fewest offenses).
    let mut order: Vec<AlignKind> = Vec::new();
    let mut offenses: std::collections::HashMap<AlignKind, Vec<NodeId>> =
        std::collections::HashMap::new();
    let mut deltas_map: std::collections::HashMap<(AlignKind, NodeId), Deltas> =
        std::collections::HashMap::new();

    let record = |kind: AlignKind,
                      target: NodeId,
                      d: Deltas,
                      order: &mut Vec<AlignKind>,
                      offenses: &mut std::collections::HashMap<AlignKind, Vec<NodeId>>,
                      deltas_map: &mut std::collections::HashMap<(AlignKind, NodeId), Deltas>| {
        // `offenses_by[klass] ||= []` — even good deltas create an empty entry
        // so `min_by` can pick a zero-offense format (AllowMultipleStyles).
        offenses.entry(kind).or_insert_with(|| {
            order.push(kind);
            Vec::new()
        });
        if d.is_good() {
            return;
        }
        offenses.entry(kind).or_default().push(target);
        deltas_map.insert((kind, target), d);
    };

    // First pair: `deltas_for_first_pair`.
    for &style in styles_for_pair(first_pair, cx, &opts) {
        let kind = align_kind(style);
        let d = deltas_for_first_pair(kind, first_pair, cx, &table_info);
        record(kind, first_pair, d, &mut order, &mut offenses, &mut deltas_map);
    }

    // Remaining children in source order (pairs + kwsplats), skipping the
    // first pair itself to avoid double-counting its separator/value deltas.
    for child in cx.children(node) {
        if child == first_pair {
            continue;
        }
        let is_pair = matches!(cx.kind(child), NodeKind::Pair { .. });
        let is_kwsplat = matches!(cx.kind(child), NodeKind::Kwsplat(_));
        if !is_pair && !is_kwsplat {
            continue;
        }
        if is_kwsplat {
            let d = deltas_kwsplat(first_pair, child, cx);
            record(AlignKind::Kwsplat, child, d, &mut order, &mut offenses, &mut deltas_map);
            continue;
        }
        for &style in styles_for_pair(child, cx, &opts) {
            let kind = align_kind(style);
            let d = deltas(kind, first_pair, child, cx, &table_info);
            record(kind, child, d, &mut order, &mut offenses, &mut deltas_map);
        }
    }

    // Kwsplat-only edge: `offenses_by` may also contain Kwsplat entries.
    // `add_offenses` reports kwsplats first, then the smallest other format.
    if let Some(kw) = offenses.remove(&AlignKind::Kwsplat) {
        for target in kw {
            if let Some(&d) = deltas_map.get(&(AlignKind::Kwsplat, target)) {
                emit_with_correction(target, d, AlignKind::Kwsplat, cx);
            }
        }
    }
    if offenses.is_empty() {
        return;
    }
    // `offenses_by.min_by { |_, v| v.length }` — first minimal wins on ties.
    let mut best: Option<AlignKind> = None;
    let mut best_len = usize::MAX;
    for kind in &order {
        if *kind == AlignKind::Kwsplat {
            continue;
        }
        if let Some(list) = offenses.get(kind)
            && list.len() < best_len
        {
            best_len = list.len();
            best = Some(*kind);
        }
    }
    let Some(kind) = best else {
        return;
    };
    let targets = offenses.remove(&kind).unwrap_or_default();
    for target in targets {
        if let Some(&d) = deltas_map.get(&(kind, target)) {
            emit_with_correction(target, d, kind, cx);
        }
    }
}

fn align_kind(style: HashAlignmentStyle) -> AlignKind {
    match style {
        HashAlignmentStyle::Key => AlignKind::Key,
        HashAlignmentStyle::Separator => AlignKind::Separator,
        HashAlignmentStyle::Table => AlignKind::Table,
    }
}

fn styles_for_pair<'a>(
    pair: NodeId,
    cx: &Cx<'_>,
    opts: &'a HashAlignmentOptions,
) -> &'a [HashAlignmentStyle] {
    if cx.is_hash_rocket(pair) {
        &opts.enforced_hash_rocket_style
    } else {
        &opts.enforced_colon_style
    }
}

/// `checkable_layout?`: `KeyAlignment` is always checkable; `table`/`separator`
/// require `!pairs_on_same_line? && !mixed_delimiters?`.
fn is_checkable(style: HashAlignmentStyle, pairs: &[NodeId], cx: &Cx<'_>) -> bool {
    match style {
        HashAlignmentStyle::Key => true,
        HashAlignmentStyle::Separator | HashAlignmentStyle::Table => {
            !pairs_on_same_line(pairs, cx) && !is_mixed_delimiters(pairs, cx)
        }
    }
}

fn emit_with_correction(target: NodeId, d: Deltas, kind: AlignKind, cx: &Cx<'_>) {
    cx.emit_offense(cx.range(target), kind.message(), None);
    apply_correction(target, d, cx);
}

fn apply_correction(target: NodeId, d: Deltas, cx: &Cx<'_>) {
    // Kwsplat or value-omitted pair: only the key column moves.
    if matches!(cx.kind(target), NodeKind::Kwsplat(_)) || is_value_omission(target, cx) {
        if d.key != 0 {
            adjust_key(target, d.key, cx);
        }
        return;
    }
    let NodeKind::Pair { key, value } = *cx.kind(target) else {
        return;
    };
    let _ = (key, value);
    if d.key != 0 {
        adjust_key(target, d.key, cx);
    }
    if d.separator != 0 {
        let op = cx.pair_operator_loc(target);
        if op != Range::ZERO {
            adjust(op, d.separator, cx);
        }
    }
    if d.value != 0
        && let Some(v) = cx.pair_value(target).get()
    {
        adjust(cx.range(v), d.value, cx);
    }
}

/// Adjust the key column: insert/remove spaces before the key (or kwsplat).
/// Mirrors `AlignmentCorrector#correct_no_value` + the key part of
/// `correct_key_value`, including the `-key_column` clamp.
fn adjust_key(target: NodeId, mut key_delta: i32, cx: &Cx<'_>) {
    let (key_start, key_col) = if matches!(cx.kind(target), NodeKind::Kwsplat(_)) {
        let r = cx.range(target);
        (r.start, column_of(cx.source(), r.start as usize) as i32)
    } else if let NodeKind::Pair { key, .. } = *cx.kind(target) {
        let r = cx.range(key);
        (r.start, column_of(cx.source(), r.start as usize) as i32)
    } else {
        return;
    };
    if key_delta < -key_col {
        key_delta = -key_col;
    }
    if key_delta == 0 {
        return;
    }
    adjust(
        Range {
            start: key_start,
            end: key_start,
        },
        key_delta,
        cx,
    );
}

/// Insert (`delta > 0`) or remove (`delta < 0`) spaces immediately before
/// `range`. Removal only fires when the bytes to delete are spaces/tabs, so a
/// mis-computed delta can never eat code.
fn adjust(range: Range, delta: i32, cx: &Cx<'_>) {
    if delta > 0 {
        cx.emit_edit(
            Range {
                start: range.start,
                end: range.start,
            },
            &" ".repeat(delta as usize),
        );
    } else if delta < 0 {
        let n = (-delta) as u32;
        if range.start < n {
            return;
        }
        let rm = Range {
            start: range.start - n,
            end: range.start,
        };
        let bytes = cx.source().as_bytes();
        let (s, e) = (rm.start as usize, rm.end as usize);
        if e > bytes.len() || s > e {
            return;
        }
        if bytes[s..e].iter().all(|&b| b == b' ' || b == b'\t') {
            cx.emit_edit(rm, "");
        }
    }
}

// ── per-style deltas ─────────────────────────────────────────────────────

fn deltas_for_first_pair(
    kind: AlignKind,
    first: NodeId,
    cx: &Cx<'_>,
    table: &TableInfo,
) -> Deltas {
    match kind {
        AlignKind::Key => Deltas {
            key: 0,
            separator: separator_delta_key_style(first, cx),
            value: value_delta_key_style(first, cx),
        },
        AlignKind::Table => {
            let sep = separator_delta_table(first, first, 0, cx, table);
            let val = value_delta_table(first, first, cx, table) - sep;
            Deltas {
                key: 0,
                separator: sep,
                value: val,
            }
        }
        AlignKind::Separator | AlignKind::Kwsplat => Deltas::default(),
    }
}

fn deltas(
    kind: AlignKind,
    first: NodeId,
    current: NodeId,
    cx: &Cx<'_>,
    table: &TableInfo,
) -> Deltas {
    match kind {
        AlignKind::Key => {
            if !begins_its_line(current, cx) {
                return Deltas::default();
            }
            Deltas {
                key: key_delta_left(first, current, cx),
                separator: separator_delta_key_style(current, cx),
                value: value_delta_key_style(current, cx),
            }
        }
        AlignKind::Table => {
            let key = key_delta_left(first, current, cx);
            let sep = separator_delta_table(first, current, key, cx, table);
            let val = value_delta_table(first, current, cx, table) - key - sep;
            Deltas {
                key,
                separator: sep,
                value: val,
            }
        }
        AlignKind::Separator => {
            let key = key_delta_right(first, current, cx);
            let sep = separator_delta_separator(first, current, key, cx);
            let val = value_delta_separator(first, current, cx) - key - sep;
            Deltas {
                key,
                separator: sep,
                value: val,
            }
        }
        AlignKind::Kwsplat => unreachable!("kwsplat uses deltas_kwsplat"),
    }
}

fn deltas_kwsplat(first: NodeId, current: NodeId, cx: &Cx<'_>) -> Deltas {
    if !begins_its_line(current, cx) {
        return Deltas::default();
    }
    Deltas {
        key: key_delta_left(first, current, cx),
        separator: 0,
        value: 0,
    }
}

// Key style intra-pair deltas (one space before `=>`, one space after operator).
fn separator_delta_key_style(pair: NodeId, cx: &Cx<'_>) -> i32 {
    if !cx.is_hash_rocket(pair) {
        return 0;
    }
    let Some(key_end_col) = pair_key_end_column(pair, cx) else {
        return 0;
    };
    let op = cx.pair_operator_loc(pair);
    if op == Range::ZERO {
        return 0;
    }
    let actual = column_of(cx.source(), op.start as usize) as i32;
    (key_end_col + 1) - actual
}

fn value_delta_key_style(pair: NodeId, cx: &Cx<'_>) -> i32 {
    if is_value_on_new_line(pair, cx) || is_value_omission(pair, cx) {
        return 0;
    }
    let op = cx.pair_operator_loc(pair);
    if op == Range::ZERO {
        return 0;
    }
    let Some(v) = cx.pair_value(pair).get() else {
        return 0;
    };
    let correct = column_of(cx.source(), op.end as usize) as i32 + 1;
    let actual = column_of(cx.source(), cx.range(v).start as usize) as i32;
    correct - actual
}

// Table / separator cross-pair deltas.
#[derive(Clone, Debug)]
struct TableInfo {
    max_key_width: i32,
    max_delimiter_width: i32,
    first_key_col: i32,
}

impl TableInfo {
    fn compute(pairs: &[NodeId], cx: &Cx<'_>, first: NodeId) -> Self {
        let mut max_key = 0i32;
        let mut max_delim = 0i32;
        for &p in pairs {
            max_key = max_key.max(key_source_len(p, cx));
            max_delim = max_delim.max(delimiter_width(p, cx));
        }
        let first_key_col = pair_key_start_column(first, cx).unwrap_or(0);
        Self {
            max_key_width: max_key,
            max_delimiter_width: max_delim,
            first_key_col,
        }
    }
}

fn key_delta_left(first: NodeId, current: NodeId, cx: &Cx<'_>) -> i32 {
    if pairs_same_line(first, current, cx) {
        return 0;
    }
    // Kwsplat right-alignment short-circuit only applies to `:right`; for
    // `:left` a kwsplat compares like a normal key.
    let a = key_start_column(first, cx).unwrap_or(0);
    let b = key_start_column(current, cx).unwrap_or(0);
    a - b
}

fn key_delta_right(first: NodeId, current: NodeId, cx: &Cx<'_>) -> i32 {
    if pairs_same_line(first, current, cx) {
        return 0;
    }
    // `return 0 if keyword_splat? && alignment == :right`.
    if matches!(cx.kind(first), NodeKind::Kwsplat(_))
        || matches!(cx.kind(current), NodeKind::Kwsplat(_))
    {
        return 0;
    }
    let a = key_end_column(first, cx).unwrap_or(0);
    let b = key_end_column(current, cx).unwrap_or(0);
    a - b
}

fn separator_delta_table(
    _first: NodeId,
    current: NodeId,
    key_delta: i32,
    cx: &Cx<'_>,
    table: &TableInfo,
) -> i32 {
    if !cx.is_hash_rocket(current) {
        return 0;
    }
    // No `same_line?` guard here: RuboCop's `TableAlignment#hash_rocket_delta`
    // computes the ideal operator column even for the first pair itself
    // (`deltas_for_first_pair`), where `first.same_line?(first)` is trivially
    // true. The `pairs_on_same_line?` gate already skips hashes where distinct
    // pairs share a line.
    let op = cx.pair_operator_loc(current);
    if op == Range::ZERO {
        return 0;
    }
    let actual = column_of(cx.source(), op.start as usize) as i32;
    let correct = table.first_key_col + table.max_key_width + 1;
    (correct - actual) - key_delta
}

fn value_delta_table(_first: NodeId, current: NodeId, cx: &Cx<'_>, table: &TableInfo) -> i32 {
    if is_value_omission(current, cx) {
        return 0;
    }
    let Some(v) = cx.pair_value(current).get() else {
        return 0;
    };
    let correct = table.first_key_col + table.max_key_width + table.max_delimiter_width;
    let actual = column_of(cx.source(), cx.range(v).start as usize) as i32;
    correct - actual
}

fn hash_rocket_delta(first: NodeId, current: NodeId, cx: &Cx<'_>) -> i32 {
    if pairs_same_line(first, current, cx) {
        return 0;
    }
    // `return 0 if first.delimiter != second.delimiter`.
    if cx.is_hash_rocket(first) != cx.is_hash_rocket(current) {
        return 0;
    }
    let a = cx.pair_operator_loc(first);
    let b = cx.pair_operator_loc(current);
    if a == Range::ZERO || b == Range::ZERO {
        return 0;
    }
    column_of(cx.source(), a.start as usize) as i32
        - column_of(cx.source(), b.start as usize) as i32
}

fn value_delta_separator(first: NodeId, current: NodeId, cx: &Cx<'_>) -> i32 {
    if pairs_same_line(first, current, cx) {
        return 0;
    }
    if matches!(cx.kind(first), NodeKind::Kwsplat(_))
        || matches!(cx.kind(current), NodeKind::Kwsplat(_))
    {
        return 0;
    }
    if is_value_omission(first, cx) || is_value_omission(current, cx) {
        return 0;
    }
    let (Some(a), Some(b)) = (
        cx.pair_value(first).get(),
        cx.pair_value(current).get(),
    ) else {
        return 0;
    };
    column_of(cx.source(), cx.range(a).start as usize) as i32
        - column_of(cx.source(), cx.range(b).start as usize) as i32
}

fn separator_delta_separator(
    first: NodeId,
    current: NodeId,
    key_delta: i32,
    cx: &Cx<'_>,
) -> i32 {
    if !cx.is_hash_rocket(current) {
        return 0;
    }
    hash_rocket_delta(first, current, cx) - key_delta
}

// ── EnforcedLastArgumentHashStyle ──────────────────────────────────────────

fn is_ignored_last_argument(node: NodeId, cx: &Cx<'_>, opts: &HashAlignmentOptions) -> bool {
    use LastArgumentHashStyle as S;
    if opts.enforced_last_argument_hash_style == S::AlwaysInspect {
        return false;
    }
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let is_last = match *cx.kind(parent) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => {
            cx.list(args).last().copied() == Some(node)
        }
        NodeKind::Super(list) | NodeKind::Yield(list) => {
            cx.list(list).last().copied() == Some(node)
        }
        _ => false,
    };
    if !is_last {
        return false;
    }
    match opts.enforced_last_argument_hash_style {
        S::AlwaysInspect => false,
        S::AlwaysIgnore => true,
        S::IgnoreExplicit => is_braced_hash(node, cx),
        S::IgnoreImplicit => !is_braced_hash(node, cx),
    }
}

// ── AST / source helpers ───────────────────────────────────────────────────

/// Whether the hash literal occupies a single source line (`node.single_line?`).
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let r = cx.range(node);
    if r.end <= r.start {
        return true;
    }
    let src = cx.source();
    line_of(r.start, src) == line_of(r.end.saturating_sub(1), src)
}

fn is_braced_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    let r = cx.range(node);
    if r.end <= r.start {
        return false;
    }
    let src = cx.raw_source(r);
    src.starts_with('{') && src.ends_with('}')
}

fn is_mixed_delimiters(pairs: &[NodeId], cx: &Cx<'_>) -> bool {
    let mut colon = false;
    let mut rocket = false;
    for &p in pairs {
        if cx.is_hash_rocket(p) {
            rocket = true;
        } else {
            colon = true;
        }
        if colon && rocket {
            return true;
        }
    }
    false
}

fn pairs_on_same_line(pairs: &[NodeId], cx: &Cx<'_>) -> bool {
    pairs
        .windows(2)
        .any(|w| pairs_same_line(w[0], w[1], cx))
}

/// `HashElementNode#same_line?`: `a.last_line == b.line || a.line == b.last_line`.
fn pairs_same_line(a: NodeId, b: NodeId, cx: &Cx<'_>) -> bool {
    let ra = cx.range(a);
    let rb = cx.range(b);
    if ra.end <= ra.start || rb.end <= rb.start {
        return false;
    }
    let src = cx.source();
    let a_first = line_of(ra.start, src);
    let a_last = line_of(ra.end.saturating_sub(1), src);
    let b_first = line_of(rb.start, src);
    let b_last = line_of(rb.end.saturating_sub(1), src);
    a_last == b_first || a_first == b_last
}

/// `Util.begins_its_line?`: the node's start is the first non-whitespace
/// position on its source line.
fn begins_its_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.source();
    let bytes = src.as_bytes();
    let start = cx.range(node).start as usize;
    if start > bytes.len() {
        return false;
    }
    let line_start = bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    bytes[line_start..start].iter().all(|&b| b == b' ' || b == b'\t')
}

fn is_value_omission(pair: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return false;
    };
    if matches!(cx.kind(value), NodeKind::Unknown) {
        return true;
    }
    // Fallback: `{x:}` — the pair source ends with `:`.
    let r = cx.range(pair);
    if r.end > r.start {
        cx.raw_source(r).trim_end().ends_with(':')
    } else {
        false
    }
}

fn is_value_on_new_line(pair: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    if matches!(cx.kind(value), NodeKind::Unknown) {
        return false;
    }
    let src = cx.source();
    line_of(cx.range(key).start, src) != line_of(cx.range(value).start, src)
}

/// Key source length for `max_key_width` (`key.source.length`): for colon
/// pairs the trailing `:` is excluded (murphy's key range includes it).
fn key_source_len(pair: NodeId, cx: &Cx<'_>) -> i32 {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return 0;
    };
    let r = cx.range(key);
    if r.end <= r.start {
        return 0;
    }
    let len = cx.raw_source(r).chars().count() as i32;
    if cx.is_hash_rocket(pair) {
        len
    } else {
        (len - 1).max(0)
    }
}

/// `pair.delimiter(true).length`: `' => '` (4) for rockets, `': '` (2) for colons.
fn delimiter_width(pair: NodeId, cx: &Cx<'_>) -> i32 {
    if cx.is_hash_rocket(pair) {
        4
    } else {
        2
    }
}

fn key_start_column(node: NodeId, cx: &Cx<'_>) -> Option<i32> {
    match *cx.kind(node) {
        NodeKind::Pair { key, .. } => {
            Some(column_of(cx.source(), cx.range(key).start as usize) as i32)
        }
        NodeKind::Kwsplat(_) => {
            Some(column_of(cx.source(), cx.range(node).start as usize) as i32)
        }
        _ => None,
    }
}

fn pair_key_start_column(pair: NodeId, cx: &Cx<'_>) -> Option<i32> {
    key_start_column(pair, cx)
}

fn key_end_column(node: NodeId, cx: &Cx<'_>) -> Option<i32> {
    match *cx.kind(node) {
        NodeKind::Pair { key, .. } => {
            Some(column_of(cx.source(), cx.range(key).end as usize) as i32)
        }
        NodeKind::Kwsplat(_) => {
            Some(column_of(cx.source(), cx.range(node).end as usize) as i32)
        }
        _ => None,
    }
}

fn pair_key_end_column(pair: NodeId, cx: &Cx<'_>) -> Option<i32> {
    key_end_column(pair, cx)
}

/// 1-based source line number containing byte `offset`.
fn line_of(offset: u32, src: &str) -> usize {
    let off = (offset as usize).min(src.len());
    src.as_bytes()[..off].iter().filter(|&&b| b == b'\n').count() + 1
}

/// 0-based column (char count) of `offset` within its source line.
fn column_of(src: &str, offset: usize) -> usize {
    let off = offset.min(src.len());
    let start = src.as_bytes()[..off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[start..off].chars().count()
}

murphy_plugin_api::submit_cop!(HashAlignment);

#[cfg(test)]
mod tests {
    use super::{HashAlignment, HashAlignmentOptions, HashAlignmentStyle, LastArgumentHashStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn table_opts() -> HashAlignmentOptions {
        HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Table],
            enforced_colon_style: vec![HashAlignmentStyle::Table],
            enforced_last_argument_hash_style: LastArgumentHashStyle::AlwaysInspect,
        }
    }

    fn separator_opts() -> HashAlignmentOptions {
        HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Separator],
            enforced_colon_style: vec![HashAlignmentStyle::Separator],
            enforced_last_argument_hash_style: LastArgumentHashStyle::AlwaysInspect,
        }
    }

    #[test]
    fn flags_misaligned_colon_key() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              foo: 1,
                barbaz: 2,
                ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
            }
        "#});
    }

    #[test]
    fn corrects_misaligned_colon_key() {
        test::<HashAlignment>().expect_correction(
            indoc! {r#"
                h = {
                  foo: 1,
                    barbaz: 2,
                    ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
                }
            "#},
            "h = {\n  foo: 1,\n  barbaz: 2,\n}\n",
        );
    }

    #[test]
    fn flags_misaligned_rocket_key() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              "a" => 1,
                "bb" => 2,
                ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
            }
        "#});
    }

    #[test]
    fn accepts_aligned_keys() {
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo: 1,
              barbaz: 2,
            }
        "#});
    }

    #[test]
    fn accepts_single_line_hash() {
        test::<HashAlignment>().expect_no_offenses("h = { foo: 1, bar: 2 }\n");
    }

    #[test]
    fn accepts_empty_hash() {
        test::<HashAlignment>().expect_no_offenses("h = {}\n");
    }

    #[test]
    fn flags_extra_space_after_colon_key_style() {
        // `foo:  1` — value one column too far right for `key` style.
        test::<HashAlignment>().expect_correction(
            indoc! {r#"
                h = {
                  foo:  1,
                  ^^^^^^^ Align the keys of a hash literal if they span more than one line.
                  bar: 2,
                }
            "#},
            "h = {\n  foo: 1,\n  bar: 2,\n}\n",
        );
    }

    #[test]
    fn flags_extra_space_before_rocket_key_style() {
        // `"a"  =>` — separator one column too far right for `key` style.
        test::<HashAlignment>().expect_correction(
            indoc! {r#"
                h = {
                  "a"  => 1,
                  ^^^^^^^^^ Align the keys of a hash literal if they span more than one line.
                  "bb" => 2,
                }
            "#},
            "h = {\n  \"a\" => 1,\n  \"bb\" => 2,\n}\n",
        );
    }

    #[test]
    fn table_flags_unaligned_value() {
        test::<HashAlignment>()
            .with_options(&table_opts())
            .expect_offense(indoc! {r#"
                h = {
                  foo: 1,
                  ^^^^^^ Align the keys and values of a hash literal if they span more than one line.
                  barbaz: 2,
                }
            "#});
    }

    #[test]
    fn table_corrects_unaligned_value() {
        test::<HashAlignment>()
            .with_options(&table_opts())
            .expect_correction(
                indoc! {r#"
                    h = {
                      foo: 1,
                      ^^^^^^ Align the keys and values of a hash literal if they span more than one line.
                      barbaz: 2,
                    }
                "#},
                "h = {\n  foo:    1,\n  barbaz: 2,\n}\n",
            );
    }

    #[test]
    fn separator_corrects_right_aligned_key() {
        test::<HashAlignment>()
            .with_options(&separator_opts())
            .expect_correction(
                indoc! {r#"
                    h = {
                      barbaz: 2,
                      foo: 1,
                      ^^^^^^ Align the separators of a hash literal if they span more than one line.
                    }
                "#},
                "h = {\n  barbaz: 2,\n     foo: 1,\n}\n",
            );
    }

    #[test]
    fn last_arg_always_ignore_skips() {
        let opts = HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Key],
            enforced_colon_style: vec![HashAlignmentStyle::Key],
            enforced_last_argument_hash_style: LastArgumentHashStyle::AlwaysIgnore,
        };
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            do_something(foo: 1,
              bar: 2)
        "#});
    }

    #[test]
    fn last_arg_always_inspect_flags() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            do_something(foo: 1,
              bar: 2)
              ^^^^^^ Align the keys of a hash literal if they span more than one line.
        "#});
    }

    #[test]
    fn last_arg_ignore_explicit_skips_braced() {
        let opts = HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Key],
            enforced_colon_style: vec![HashAlignmentStyle::Key],
            enforced_last_argument_hash_style: LastArgumentHashStyle::IgnoreExplicit,
        };
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            do_something({foo: 1,
              bar: 2})
        "#});
    }

    #[test]
    fn last_arg_ignore_implicit_skips_braceless() {
        let opts = HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Key],
            enforced_colon_style: vec![HashAlignmentStyle::Key],
            enforced_last_argument_hash_style: LastArgumentHashStyle::IgnoreImplicit,
        };
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            do_something(foo: 1,
              bar: 2)
        "#});
    }

    #[test]
    fn flags_misaligned_kwsplat() {
        test::<HashAlignment>().expect_offense(indoc! {r#"
            h = {
              foo: 1,
                **bar,
                ^^^^^ Align keyword splats with the rest of the hash if it spans more than one line.
            }
        "#});
    }

    #[test]
    fn accepts_aligned_kwsplat() {
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo: 1,
              **bar,
            }
        "#});
    }

    #[test]
    fn table_skips_mixed_delimiters() {
        // `mixed_delimiters?` makes table/separator uncheckable — no offense
        // even though values are unaligned.
        test::<HashAlignment>()
            .with_options(&table_opts())
            .expect_no_offenses(indoc! {r#"
                h = {
                  foo: 1,
                  "bar" => 2,
                }
            "#});
    }

    #[test]
    fn table_skips_pairs_on_same_line() {
        test::<HashAlignment>()
            .with_options(&table_opts())
            .expect_no_offenses(indoc! {r#"
                h = {
                  foo: 1, bar: 2,
                  baz: 3,
                }
            "#});
    }

    #[test]
    fn multiple_styles_accepts_either_layout() {
        use murphy_plugin_api::CopOptions;
        let opts = HashAlignmentOptions::from_config_json(
            br#"{"EnforcedColonStyle":["key","separator"],"EnforcedHashRocketStyle":"key"}"#,
        )
        .expect("array style must decode");
        assert_eq!(opts.enforced_colon_style.len(), 2);
        // Key-aligned layout: key 0 offenses, separator 1+ → min 0 → accept.
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            h = {
              foo: 1,
              barbaz: 2,
            }
        "#});
        // Separator-aligned layout: separator 0, key 1+ → min 0 → accept.
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            h = {
              barbaz: 2,
                 foo: 1,
            }
        "#});
    }

    #[test]
    fn options_decode_string_or_array() {
        use murphy_plugin_api::CopOptions;
        let single = HashAlignmentOptions::from_config_json(
            br#"{"EnforcedColonStyle":"table"}"#,
        )
        .expect("single string must decode");
        assert_eq!(single.enforced_colon_style, vec![HashAlignmentStyle::Table]);
        let multi = HashAlignmentOptions::from_config_json(
            br#"{"EnforcedHashRocketStyle":["key","table"]}"#,
        )
        .expect("array must decode");
        assert_eq!(
            multi.enforced_hash_rocket_style,
            vec![HashAlignmentStyle::Key, HashAlignmentStyle::Table]
        );
        // Null falls back to default.
        let nul = HashAlignmentOptions::from_config_json(
            br#"{"EnforcedColonStyle":null}"#,
        )
        .expect("null must decode");
        assert_eq!(nul.enforced_colon_style, vec![HashAlignmentStyle::Key]);
        // Unknown value is rejected.
        assert!(HashAlignmentOptions::from_config_json(
            br#"{"EnforcedColonStyle":"bogus"}"#,
        )
        .is_err());
    }

    #[test]
    fn table_rocket_corrects() {
        test::<HashAlignment>()
            .with_options(&table_opts())
            .expect_correction(
                indoc! {r#"
                    h = {
                      "a" => 1,
                      ^^^^^^^^ Align the keys and values of a hash literal if they span more than one line.
                      "bb" => 2,
                    }
                "#},
                "h = {\n  \"a\"  => 1,\n  \"bb\" => 2,\n}\n",
            );
    }

    #[test]
    fn separator_rocket_corrects() {
        test::<HashAlignment>()
            .with_options(&separator_opts())
            .expect_correction(
                indoc! {r#"
                    h = {
                      "bb" => 2,
                      "a" => 1,
                      ^^^^^^^^ Align the separators of a hash literal if they span more than one line.
                    }
                "#},
                "h = {\n  \"bb\" => 2,\n   \"a\" => 1,\n}\n",
            );
    }

    #[test]
    fn accepts_value_omission() {
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            x = 1
            h = {
              x:,
              y: 2,
            }
        "#});
    }

    #[test]
    fn accepts_value_on_new_line() {
        // Value on its own line is exempt from value-spacing checks.
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo:
                1,
              bar: 2,
            }
        "#});
    }

    #[test]
    fn last_arg_super_always_ignore_skips() {
        let opts = HashAlignmentOptions {
            enforced_hash_rocket_style: vec![HashAlignmentStyle::Key],
            enforced_colon_style: vec![HashAlignmentStyle::Key],
            enforced_last_argument_hash_style: LastArgumentHashStyle::AlwaysIgnore,
        };
        test::<HashAlignment>().with_options(&opts).expect_no_offenses(indoc! {r#"
            super(foo: 1,
              bar: 2)
        "#});
    }

    #[test]
    fn ignores_pair_not_beginning_its_line() {
        test::<HashAlignment>().expect_no_offenses(indoc! {r#"
            h = {
              foo: 1, bar: 2,
              baz: 3,
            }
        "#});
    }
}

