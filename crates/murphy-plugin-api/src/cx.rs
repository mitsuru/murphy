//! `Cx<'a>` — the single surface through which a cop reads the AST.

use std::marker::PhantomData;

use murphy_ast::{
    AstNode, CallClosingLoc, CallOperatorLoc, Comment, CommentKind, MagicComment, MagicCommentKind,
    NodeId, NodeKind, OptNodeId, Range, SourceToken, SourceTokenKind, collect_children,
    slot_layout,
};

use crate::abi::CxRaw;
use crate::{ConfigError, CopOptions};

/// Borrowed, direct-read view of the arena for one dispatch call.
///
/// Traversal and `NodeKind` matching are pure memory reads — zero FFI
/// (ADR 0038). The lifetime `'a` forbids retaining any part past the
/// call; the arena is immutable and host-owned for the call's duration.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    raw: &'a CxRaw,
    _marker: PhantomData<&'a murphy_ast::Ast>,
}

/// Reconstruct a slice from a `#[repr(C)]` pointer+length pair.
///
/// # Safety
/// `len == 0` → empty; otherwise `ptr..ptr+len` must be valid for `'a`.
unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

pub unsafe extern "C" fn unavailable_alloc_node_slice(
    _: *mut std::ffi::c_void,
    _: *const NodeId,
    _: usize,
) -> *const NodeId {
    panic!("CxRaw::alloc_node_slice is required for `$...` captured rest inside `<...>`")
}

/// Lazy source-location view for a node — Murphy's analog of rubocop-ast's
/// `node.loc`. `expression` and `name` are plain fields (zero-cost); all
/// other sub-ranges come from parser-provided side tables or are computed on
/// demand from the arena's sorted token list and source bytes.
pub struct LocRef<'a> {
    pub expression: Range,
    pub name: Range,
    // Private: precomputed for dot() and keyword()
    call_operator: Range,
    keyword_bearing: bool,
    sorted_tokens: &'a [SourceToken],
    source: &'a [u8],
}

/// A parsed `murphy:`/`rubocop:` directive comment.
///
/// `cop == None` means the directive applies to all cops. Ranges are byte
/// ranges into [`Cx::source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentDirective<'a> {
    pub kind: CommentDirectiveKind,
    pub scope: CommentDirectiveScope,
    pub comment_range: Range,
    pub line_range: Range,
    pub affected_range: Range,
    pub cop: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentDirectiveKind {
    Disable,
    Enable,
    Todo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentDirectiveScope {
    /// End-of-line directive; affects only the line containing the comment.
    SameLine,
    /// Own-line directive; affects following source until a matching enable.
    Block,
    /// File-top disable-all directive; affects the entire file.
    File,
}

/// Which side of a range a RuboCop-style range helper should expand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RangeSide {
    Left,
    Right,
    Both,
}

/// Options for [`Cx::range_with_surrounding_space`].
///
/// Defaults mirror RuboCop's `RangeHelp#range_with_surrounding_space`:
/// expand both sides through spaces/tabs and newlines, but not backslash
/// continuations or arbitrary Unicode whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceRangeOptions {
    pub side: RangeSide,
    pub newlines: bool,
    pub whitespace: bool,
    pub continuations: bool,
}

impl Default for SpaceRangeOptions {
    fn default() -> Self {
        Self {
            side: RangeSide::Both,
            newlines: true,
            whitespace: false,
            continuations: false,
        }
    }
}

fn line_range(source: &str, offset: usize) -> Range {
    let bytes = source.as_bytes();
    let start = bytes[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let end = bytes[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| offset + pos + 1);
    Range {
        start: start as u32,
        end: end as u32,
    }
}

fn clamp_range(range: Range, len: usize) -> Range {
    let start = (range.start as usize).min(len);
    let end = (range.end as usize).min(len).max(start);
    Range {
        start: start as u32,
        end: end as u32,
    }
}

fn range_directions(side: RangeSide) -> (bool, bool) {
    match side {
        RangeSide::Left => (true, false),
        RangeSide::Right => (false, true),
        RangeSide::Both => (true, true),
    }
}

fn move_pos(
    src: &[u8],
    mut pos: usize,
    step: isize,
    condition: bool,
    matches: impl Fn(u8) -> bool,
) -> usize {
    if !condition {
        return pos;
    }
    if step < 0 {
        while pos > 0 && matches(src[pos - 1]) {
            pos -= 1;
        }
    } else {
        while pos < src.len() && matches(src[pos]) {
            pos += 1;
        }
    }
    pos
}

fn move_pos_str(src: &[u8], mut pos: usize, step: isize, condition: bool, needle: &[u8]) -> usize {
    if !condition {
        return pos;
    }
    if step < 0 {
        while pos >= needle.len() && &src[pos - needle.len()..pos] == needle {
            pos -= needle.len();
        }
    } else {
        while pos + needle.len() <= src.len() && &src[pos..pos + needle.len()] == needle {
            pos += needle.len();
        }
    }
    pos
}

fn final_space_pos(src: &[u8], pos: usize, step: isize, options: SpaceRangeOptions) -> usize {
    let pos = move_pos(src, pos, step, true, |b| matches!(b, b' ' | b'\t'));
    let pos = move_pos_str(src, pos, step, options.continuations, b"\\\n");
    let pos = move_pos(src, pos, step, options.newlines, |b| b == b'\n');
    move_pos(src, pos, step, options.whitespace, |b| {
        b.is_ascii_whitespace()
    })
}

fn own_line_comment(source: &str, comment: Comment) -> bool {
    if comment.kind != murphy_ast::CommentKind::Inline {
        return false;
    }
    let line = line_range(source, comment.range.start as usize);
    source[line.start as usize..comment.range.start as usize]
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t'))
}

fn prefix_has_code(source: &str, end: usize) -> bool {
    source[..end].lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    })
}

fn parse_comment_directive(text: &str) -> Option<(CommentDirectiveKind, &str)> {
    let comment = text.strip_prefix('#')?.trim_start();
    let rest = comment
        .strip_prefix("murphy:")
        .or_else(|| comment.strip_prefix("rubocop:"))?;
    let rest = rest.trim_start();
    let (keyword, tail) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(keyword, tail)| (keyword, tail));
    let kind = match keyword {
        "disable" => CommentDirectiveKind::Disable,
        "enable" => CommentDirectiveKind::Enable,
        "todo" => CommentDirectiveKind::Todo,
        _ => return None,
    };
    let cops_text = tail.split_once("--").map_or(tail, |(cops, _)| cops).trim();
    Some((kind, cops_text))
}

fn first_fully_enabled_line(
    disabled_cop: Option<&str>,
    later: &[CommentDirective<'_>],
) -> Option<u32> {
    for candidate in later {
        if candidate.kind != CommentDirectiveKind::Enable {
            continue;
        }
        if candidate.cop.is_none() {
            return Some(candidate.line_range.start);
        }
        if candidate.cop == disabled_cop {
            return Some(candidate.line_range.start);
        }
    }
    None
}

/// Parse all directive comments from an already-translated comment table.
pub fn comment_directives_from_comments<'a>(
    source: &'a str,
    comments: &'a [Comment],
) -> Vec<CommentDirective<'a>> {
    let mut directives = Vec::new();
    for comment in comments {
        if comment.kind != murphy_ast::CommentKind::Inline {
            continue;
        }
        let comment_text = &source[comment.range.start as usize..comment.range.end as usize];
        let Some((kind, cops_text)) = parse_comment_directive(comment_text) else {
            continue;
        };
        let applies_to_all = cops_text.eq_ignore_ascii_case("all") || cops_text.is_empty();
        let line_range = line_range(source, comment.range.start as usize);
        let before_comment = &source[line_range.start as usize..comment.range.start as usize];
        let same_line = !before_comment.trim().is_empty();
        let scope = if !same_line
            && kind == CommentDirectiveKind::Disable
            && applies_to_all
            && !prefix_has_code(source, line_range.start as usize)
        {
            CommentDirectiveScope::File
        } else if same_line {
            CommentDirectiveScope::SameLine
        } else {
            CommentDirectiveScope::Block
        };
        let affected_range = match scope {
            CommentDirectiveScope::File => Range {
                start: 0,
                end: source.len() as u32,
            },
            CommentDirectiveScope::SameLine => line_range,
            CommentDirectiveScope::Block if kind == CommentDirectiveKind::Disable => Range {
                start: line_range.end,
                end: source.len() as u32,
            },
            CommentDirectiveScope::Block => line_range,
        };

        if applies_to_all {
            directives.push(CommentDirective {
                kind,
                scope,
                comment_range: comment.range,
                line_range,
                affected_range,
                cop: None,
            });
        } else {
            for cop in cops_text
                .split(',')
                .map(str::trim)
                .filter(|cop| !cop.is_empty())
            {
                directives.push(CommentDirective {
                    kind,
                    scope,
                    comment_range: comment.range,
                    line_range,
                    affected_range,
                    cop: Some(cop),
                });
            }
        }
    }

    for i in 0..directives.len() {
        if directives[i].kind != CommentDirectiveKind::Disable
            || directives[i].scope == CommentDirectiveScope::SameLine
        {
            continue;
        }
        let Some(enable_line_start) =
            first_fully_enabled_line(directives[i].cop, &directives[i + 1..])
        else {
            continue;
        };
        directives[i].affected_range.end = enable_line_start;
    }

    directives
}

impl<'a> LocRef<'a> {
    /// Parser-provided call-operator range: `.` for `Send`, `&.` for `Csend`.
    /// Returns `Range::ZERO` when Prism provided no call operator for the node.
    pub fn dot(&self) -> Range {
        self.call_operator
    }

    /// The leading keyword token range (`def`, `class`, `if`, `while`, …).
    /// Computed by binary-searching sorted_tokens for the first token at
    /// `expression.start`. Returns `Range::ZERO` if no token starts exactly
    /// at that position.
    ///
    /// **Limitation:** modifier-form control flow (`x if cond`) places the
    /// keyword *after* the expression start — returns `Range::ZERO` for those.
    pub fn keyword(&self) -> Range {
        if !self.keyword_bearing {
            return Range::ZERO;
        }
        let target = self.expression.start;
        let idx = self
            .sorted_tokens
            .partition_point(|t| t.range.start < target);
        if let Some(tok) = self.sorted_tokens.get(idx)
            && tok.range.start == target
        {
            return tok.range;
        }
        Range::ZERO
    }

    /// The opening-paren `(` range for this node's own argument list, or
    /// `Range::ZERO` if none. Covers only `(` — not `[`, `{`, or `do`.
    ///
    /// Searches from `name.end` (not `expression.start`) so that parens
    /// inside child nodes (e.g. `foo bar(baz)`) are not mistakenly returned
    /// for the outer call.
    pub fn begin(&self) -> Range {
        // Search from name.end to skip over child node parens.
        let search_from = if self.name != Range::ZERO {
            self.name.end
        } else {
            self.expression.start
        };
        let idx = self
            .sorted_tokens
            .partition_point(|t| t.range.start < search_from);
        if let Some(tok) = self.sorted_tokens.get(idx)
            && tok.range.start < self.expression.end
            && tok.kind == SourceTokenKind::LeftParen
        {
            return tok.range;
        }
        Range::ZERO
    }

    /// The closing-paren `)` matching this node's `begin()` paren, or
    /// `Range::ZERO` if `begin()` is `Range::ZERO`. Uses a nesting counter
    /// so `foo(bar(x))` correctly returns the outer `)`.
    pub fn end(&self) -> Range {
        let begin = self.begin();
        if begin == Range::ZERO {
            return Range::ZERO;
        }
        let search_from = begin.end;
        let expr_end = self.expression.end;
        let idx = self
            .sorted_tokens
            .partition_point(|t| t.range.start < search_from);
        let mut depth: i32 = 1;
        for tok in &self.sorted_tokens[idx..] {
            if tok.range.start >= expr_end {
                break;
            }
            match tok.kind {
                SourceTokenKind::LeftParen => depth += 1,
                SourceTokenKind::RightParen => {
                    depth -= 1;
                    if depth == 0 {
                        return tok.range;
                    }
                }
                _ => {}
            }
        }
        Range::ZERO
    }

    /// The exact source bytes of `tok`.
    fn token_text(&self, tok: SourceToken) -> &'a [u8] {
        &self.source[tok.range.start as usize..tok.range.end as usize]
    }

    /// The closing `end` keyword of a block-form `if`/`unless`/`while`/
    /// `until`/`def`/`class`/… , or `Range::ZERO` for modifier-form
    /// (`body if cond`) and ternaries, which have none. The `end` keyword
    /// terminates the node, so it is the token ending exactly at
    /// `expression.end`; presence is the `loc.end`-keyword signal that
    /// `modifier_form?` (RuboCop's `loc.end.nil?`) inverts.
    pub fn end_keyword(&self) -> Range {
        let expr_end = self.expression.end;
        let idx = self
            .sorted_tokens
            .partition_point(|t| t.range.end < expr_end);
        if let Some(tok) = self.sorted_tokens.get(idx)
            && tok.range.end == expr_end
            && self.token_text(*tok) == b"end"
        {
            return tok.range;
        }
        Range::ZERO
    }
}

impl<'a> Cx<'a> {
    /// Wrap a raw context.
    ///
    /// # Safety
    /// Every pointer/length pair in `raw` must describe live, immutable
    /// data valid for `'a`, and `raw.fns` must be non-null. The host
    /// upholds this for one dispatch call (ADR 0038 safety contract).
    pub unsafe fn from_raw(raw: &'a CxRaw) -> Cx<'a> {
        Cx {
            raw,
            _marker: PhantomData,
        }
    }

    /// Access the file-level variable scope analysis model.
    ///
    /// Returns `None` only when the host passes a null pointer — in
    /// practice `None` is unreachable during normal cop dispatch, because
    /// `murphy-core`'s dispatcher always sets this to a freshly-built
    /// model. Test harnesses that construct `CxRaw` by hand may leave it
    /// null; cops should treat `None` as "model unavailable" and skip
    /// any var-semantic checks.
    pub fn var_model(&self) -> Option<&'a crate::var_semantic_model::VarSemanticModel> {
        unsafe { self.raw.var_model.as_ref() }
    }

    /// Allocate a dispatch-lifetime copy of `elements` in the host arena.
    pub fn alloc_node_slice(&self, elements: &[NodeId]) -> &'a [NodeId] {
        if elements.is_empty() {
            return &[];
        }
        let ptr = unsafe {
            (self.raw.alloc_node_slice)(
                self.raw.node_slice_arena,
                elements.as_ptr(),
                elements.len(),
            )
        };
        assert!(
            !ptr.is_null(),
            "CxRaw::alloc_node_slice returned null for non-empty input"
        );
        unsafe { std::slice::from_raw_parts(ptr, elements.len()) }
    }

    fn nodes(&self) -> &'a [AstNode] {
        unsafe { slice(self.raw.nodes, self.raw.nodes_len) }
    }

    fn lists(&self) -> &'a [NodeId] {
        unsafe { slice(self.raw.lists, self.raw.lists_len) }
    }

    fn call_closing_locs(&self) -> &'a [CallClosingLoc] {
        unsafe { slice(self.raw.call_closing_locs, self.raw.call_closing_locs_len) }
    }

    fn call_operator_locs(&self) -> &'a [CallOperatorLoc] {
        unsafe { slice(self.raw.call_operator_locs, self.raw.call_operator_locs_len) }
    }

    fn find_magic_comment(&self, kind: MagicCommentKind) -> Option<MagicComment> {
        self.magic_comments()
            .into_iter()
            .find(|comment| comment.kind == kind)
    }

    fn source_line_range_without_newline(&self, offset: usize) -> Range {
        let source = self.source().as_bytes();
        let start = source[..offset]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let mut end = source[offset..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(source.len(), |pos| offset + pos);
        if end > start && source[end - 1] == b'\r' {
            end -= 1;
        }
        Range {
            start: start as u32,
            end: end as u32,
        }
    }

    fn magic_comment_kind(key: &str) -> Option<MagicCommentKind> {
        fn eq_normalized(actual: &str, expected: &str) -> bool {
            actual.len() == expected.len()
                && actual
                    .bytes()
                    .zip(expected.bytes())
                    .all(|(actual, expected)| {
                        let actual = if actual == b'-' {
                            b'_'
                        } else {
                            actual.to_ascii_lowercase()
                        };
                        actual == expected
                    })
        }

        if eq_normalized(key, "frozen_string_literal") {
            Some(MagicCommentKind::FrozenStringLiteral)
        } else if eq_normalized(key, "encoding") || eq_normalized(key, "coding") {
            Some(MagicCommentKind::Encoding)
        } else {
            None
        }
    }

    fn leading_comment_region_end(&self) -> usize {
        let source = self.source().as_bytes();
        let mut line_start = 0;
        while line_start < source.len() {
            let line_end = source[line_start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(source.len(), |pos| line_start + pos);
            let mut content_end = line_end;
            if content_end > line_start && source[content_end - 1] == b'\r' {
                content_end -= 1;
            }

            if line_start == 0 && source.starts_with(b"#!") {
                line_start = line_end.saturating_add(1);
                continue;
            }

            let mut first = line_start;
            while first < content_end && source[first].is_ascii_whitespace() {
                first += 1;
            }
            if first < content_end && source[first] == b'#' {
                line_start = line_end.saturating_add(1);
                continue;
            }
            return line_start;
        }
        source.len()
    }

    fn is_own_line_comment(&self, comment: Comment) -> bool {
        let source = self.source().as_bytes();
        let start = comment.range.start as usize;
        let line_start = source[..start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        source[line_start..start]
            .iter()
            .all(|byte| byte.is_ascii_whitespace())
    }

    fn parse_magic_comment(&self, comment: Comment) -> Option<MagicComment> {
        if comment.kind != CommentKind::Inline {
            return None;
        }
        let text = self.raw_source(comment.range);
        let bytes = text.as_bytes();
        if bytes.first() != Some(&b'#') {
            return None;
        }
        let mut key_start = 1;
        while key_start < bytes.len() && bytes[key_start].is_ascii_whitespace() {
            key_start += 1;
        }
        let mut parse_end = bytes.len();
        if bytes[key_start..].starts_with(b"-*-") {
            key_start += 3;
            while key_start < bytes.len() && bytes[key_start].is_ascii_whitespace() {
                key_start += 1;
            }
            if let Some(suffix_start) = bytes[key_start..]
                .windows(3)
                .rposition(|window| window == b"-*-")
                .map(|pos| key_start + pos)
            {
                parse_end = suffix_start;
            }
        }
        let mut key_end = key_start;
        while key_end < parse_end
            && (bytes[key_end].is_ascii_alphanumeric()
                || bytes[key_end] == b'_'
                || bytes[key_end] == b'-')
        {
            key_end += 1;
        }
        if key_start == key_end {
            return None;
        }
        let mut sep = key_end;
        while sep < parse_end && bytes[sep].is_ascii_whitespace() {
            sep += 1;
        }
        if !matches!(bytes.get(sep), Some(b':' | b'=')) {
            return None;
        }
        let mut value_start = sep + 1;
        while value_start < parse_end && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        let mut value_end = parse_end;
        while value_end > value_start && bytes[value_end - 1].is_ascii_whitespace() {
            value_end -= 1;
        }

        let kind = Self::magic_comment_kind(&text[key_start..key_end])?;
        let base = comment.range.start;
        let value = &text[value_start..value_end];
        Some(MagicComment {
            range: comment.range,
            key_range: Range {
                start: base + key_start as u32,
                end: base + key_end as u32,
            },
            value_range: Range {
                start: base + value_start as u32,
                end: base + value_end as u32,
            },
            kind,
            value_bool: u8::from(
                kind == MagicCommentKind::FrozenStringLiteral && value.eq_ignore_ascii_case("true"),
            ),
        })
    }

    fn call_operator_range(&self, id: NodeId) -> Range {
        let call_operator_locs = self.call_operator_locs();
        call_operator_locs
            .binary_search_by_key(&id.0, |entry| entry.node.0)
            .map(|idx| call_operator_locs[idx].operator)
            .unwrap_or(Range::ZERO)
    }

    /// The arena root node.
    pub fn root(&self) -> NodeId {
        self.raw.root
    }

    /// The node at `id`.
    pub fn node(&self, id: NodeId) -> &'a AstNode {
        &self.nodes()[id.0 as usize]
    }

    /// The kind of the node at `id`.
    pub fn kind(&self, id: NodeId) -> &'a NodeKind {
        &self.nodes()[id.0 as usize].kind
    }

    /// The source range of the node at `id` — shorthand for
    /// `self.loc(id).expression` / `self.node(id).loc.expression`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes()[id.0 as usize].loc.expression
    }

    /// The source-location view for `id`. `expression` and `name` are plain
    /// fields. Call `.dot()`, `.keyword()`, `.begin()`, `.end()` for sub-ranges
    /// — most compute only when used; `.dot()` returns the parser-provided
    /// call operator side-table entry captured here.
    pub fn loc(&self, id: NodeId) -> LocRef<'a> {
        let node = &self.nodes()[id.0 as usize];
        let call_operator = self.call_operator_range(id);
        let src: &'a [u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        // For conditionals/loops that have both block-form (`if cond; end`) and
        // modifier-form (`body if cond`), detect block-form by checking whether
        // the source at expression.start begins with the keyword itself.
        let starts_with_ctrl_kw = |start: u32| -> bool {
            let s = start as usize;
            let rest = &src[s..];
            let word_len = rest
                .iter()
                .position(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                .unwrap_or(rest.len());
            matches!(&rest[..word_len], b"if" | b"unless" | b"while" | b"until")
        };
        let keyword_bearing = matches!(
            node.kind,
            NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::Class { .. }
                | NodeKind::Module { .. }
                | NodeKind::Case { .. }
                | NodeKind::When { .. }
                | NodeKind::Begin(_)
                | NodeKind::Kwbegin(_)
                | NodeKind::Return(_)
                | NodeKind::Break(_)
                | NodeKind::Next(_)
                | NodeKind::Yield(_)
                | NodeKind::Super(_)
                | NodeKind::Zsuper
                | NodeKind::Defined(_)
                | NodeKind::For { .. }
                | NodeKind::Rescue { .. }
        ) || matches!(
            node.kind,
            NodeKind::If { .. } | NodeKind::While { .. } | NodeKind::Until { .. }
        ) && starts_with_ctrl_kw(node.loc.expression.start);
        LocRef {
            expression: node.loc.expression,
            name: node.loc.name,
            call_operator,
            keyword_bearing,
            sorted_tokens: self.sorted_tokens(),
            source: src,
        }
    }

    /// The parent of `id`; `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes()[id.0 as usize].parent
    }

    /// Resolve a [`NodeList`] to its backing slice of child ids.
    ///
    /// Zero-copy: returns a borrow directly into the arena's `node_lists`
    /// side table. This is the allocation-free counterpart to
    /// [`Self::children`] for the variable-length child field of a single
    /// `NodeKind` variant (e.g. `Send.args`, `Array`'s elements). The
    /// generated code of `def_node_matcher!` (murphy-9cr.18) uses it to bind
    /// `$...` seq captures and to match fixed-length argument lists.
    pub fn list(&self, l: murphy_ast::NodeList) -> &'a [NodeId] {
        let start = l.start as usize;
        &self.lists()[start..start + l.len as usize]
    }

    /// Direct children of `id`, in source order. Allocates one `Vec` per
    /// call because `collect_children` writes into a `Vec`; an
    /// allocation-free iterator variant could be added later if profiling
    /// shows it matters.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        collect_children(self.kind(id), self.lists(), &mut out);
        out
    }

    /// Ancestors of `id`, nearest first, up to and including the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + 'a {
        let nodes = self.nodes();
        let mut current = nodes[id.0 as usize].parent;
        std::iter::from_fn(move || {
            let next = current.get()?;
            current = nodes[next.0 as usize].parent;
            Some(next)
        })
    }

    /// Ancestors of `id` whose kind matches a pattern node-type name or alias
    /// group such as `call`, `any_block`, `numeric`, or `any_str`.
    pub fn ancestors_of_type(
        &self,
        id: NodeId,
        type_name: &str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        let tags = murphy_ast::tags_for_type_name(type_name);
        let mut ancestors = (!tags.is_empty()).then(|| self.ancestors(id));
        let cx = *self;
        std::iter::from_fn(move || {
            ancestors
                .as_mut()?
                .find(|&ancestor| tags.contains(&cx.kind(ancestor).tag()))
        })
    }

    /// All descendants of `id` in DFS pre-order, excluding `id`. Allocates
    /// one `Vec` per call (plus per-node `Vec`s via [`Self::children`]); an
    /// allocation-free iterator variant could be added later if profiling
    /// shows it matters.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = self.children(id);
        stack.reverse();
        while let Some(n) = stack.pop() {
            out.push(n);
            let mut kids = self.children(n);
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// Resolve an interner index (`Symbol` / `StringId`) to its string.
    fn resolve(&self, index: u32) -> &'a str {
        let offsets: &[Range] =
            unsafe { slice(self.raw.interner_offsets, self.raw.interner_offsets_len) };
        let blob: &[u8] = unsafe { slice(self.raw.interner_blob, self.raw.interner_blob_len) };
        let r = offsets[index as usize];
        std::str::from_utf8(&blob[r.start as usize..r.end as usize])
            .expect("interner blob holds valid UTF-8")
    }

    /// The string behind an interned `Symbol`.
    pub fn symbol_str(&self, sym: murphy_ast::Symbol) -> &'a str {
        self.resolve(sym.0)
    }

    /// The contents behind an interned string-literal `StringId`.
    pub fn string_str(&self, id: murphy_ast::StringId) -> &'a str {
        self.resolve(id.0)
    }

    /// The method-name selector of a method-bearing node — the call
    /// selector for `Send`/`Csend`, or the defined name for `Def`/`Defs`.
    /// A block node (`Block` / `Numblock` / `Itblock`) delegates to its
    /// wrapped call, so `foo.each { }` reports `"each"` — matching
    /// RuboCop, where `BlockNode`/`NumblockNode` are method-dispatch
    /// nodes whose `method_name` is the underlying `send_node`'s. `None`
    /// for any other node kind.
    pub fn method_name(&self, id: NodeId) -> Option<&'a str> {
        let sym = match *self.kind(id) {
            NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => method,
            NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => name,
            NodeKind::Block { call, .. } => return self.method_name(call),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => {
                return self.method_name(send);
            }
            _ => return None,
        };
        Some(self.symbol_str(sym))
    }

    /// The selector source range for method-like nodes. For calls and defs
    /// this is `loc.name`; for keyword-call nodes (`yield`, `super`,
    /// `defined?`) this is the leading keyword token.
    pub fn selector(&self, id: NodeId) -> Range {
        match *self.kind(id) {
            NodeKind::Send { .. }
            | NodeKind::Csend { .. }
            | NodeKind::Def { .. }
            | NodeKind::Defs { .. } => self.loc(id).name,
            NodeKind::Yield(_) | NodeKind::Super(_) | NodeKind::Zsuper | NodeKind::Defined(_) => {
                self.loc(id).keyword()
            }
            NodeKind::Block { call, .. } => self.selector(call),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => self.selector(send),
            _ => Range::ZERO,
        }
    }

    /// The block node wrapping this send-like node, if the block is the
    /// node's immediate parent. Mirrors RuboCop's `send_node.block_node`.
    pub fn block_node(&self, id: NodeId) -> OptNodeId {
        let Some(parent) = self.parent(id).get() else {
            return OptNodeId::NONE;
        };
        match *self.kind(parent) {
            NodeKind::Block { call, .. } if call == id => OptNodeId::some(parent),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } if send == id => {
                OptNodeId::some(parent)
            }
            _ => OptNodeId::NONE,
        }
    }

    /// `comparison_method?` for the node's selector — see
    /// [`crate::method_predicates::is_comparison_method`]. `false` for
    /// nodes without a selector.
    pub fn is_comparison_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_comparison_method)
    }

    /// `operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_operator_method`].
    pub fn is_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_operator_method)
    }

    /// `assignment_method?` for the node's selector — see
    /// [`crate::method_predicates::is_assignment_method`].
    pub fn is_assignment_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_assignment_method)
    }

    /// `predicate_method?` for the node's selector — see
    /// [`crate::method_predicates::is_predicate_method`].
    pub fn is_predicate_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_predicate_method)
    }

    /// `bang_method?` for the node's selector — see
    /// [`crate::method_predicates::is_bang_method`].
    pub fn is_bang_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_bang_method)
    }

    /// `camel_case_method?` for the node's selector — see
    /// [`crate::method_predicates::is_camel_case_method`].
    pub fn is_camel_case_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_camel_case_method)
    }

    /// `enumerable_method?` for the node's selector — see
    /// [`crate::method_predicates::is_enumerable_method`].
    pub fn is_enumerable_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_enumerable_method)
    }

    /// `enumerator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_enumerator_method`].
    pub fn is_enumerator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_enumerator_method)
    }

    /// `nonmutating_binary_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_binary_operator_method`].
    pub fn is_nonmutating_binary_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_binary_operator_method)
    }

    /// `nonmutating_unary_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_unary_operator_method`].
    pub fn is_nonmutating_unary_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_unary_operator_method)
    }

    /// `nonmutating_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_operator_method`].
    pub fn is_nonmutating_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_operator_method)
    }

    /// `nonmutating_array_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_array_method`].
    pub fn is_nonmutating_array_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_array_method)
    }

    /// `nonmutating_hash_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_hash_method`].
    pub fn is_nonmutating_hash_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_hash_method)
    }

    /// `nonmutating_string_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_string_method`].
    pub fn is_nonmutating_string_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_string_method)
    }

    /// The receiver of a call node (`Send`/`Csend`), or `OptNodeId::NONE`
    /// for a receiverless `Send` or any non-call node. Mirrors RuboCop's
    /// `node.receiver`.
    pub fn call_receiver(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Send { receiver, .. } => receiver,
            NodeKind::Csend { receiver, .. } => OptNodeId::some(receiver),
            _ => OptNodeId::NONE,
        }
    }

    /// The argument list of a call node (`Send`/`Csend`); an empty slice
    /// for a non-call node. Mirrors RuboCop's `node.arguments`.
    pub fn call_arguments(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => self.list(args),
            _ => &[],
        }
    }

    /// The first argument of a call node, or `OptNodeId::NONE`. Mirrors
    /// RuboCop's `node.first_argument`.
    pub fn first_argument(&self, id: NodeId) -> OptNodeId {
        self.call_arguments(id)
            .first()
            .copied()
            .map_or(OptNodeId::NONE, OptNodeId::some)
    }

    /// The last argument of a call node, or `OptNodeId::NONE`. Mirrors
    /// RuboCop's `node.last_argument`.
    pub fn last_argument(&self, id: NodeId) -> OptNodeId {
        self.call_arguments(id)
            .last()
            .copied()
            .map_or(OptNodeId::NONE, OptNodeId::some)
    }

    /// Whether a call node has any arguments. Mirrors RuboCop's
    /// `node.arguments?`.
    pub fn has_call_arguments(&self, id: NodeId) -> bool {
        !self.call_arguments(id).is_empty()
    }

    /// `self_receiver?` — the call's receiver is `self`. Mirrors RuboCop's
    /// `node.self_receiver?` (`receiver&.self_type?`).
    pub fn is_self_receiver(&self, id: NodeId) -> bool {
        self.call_receiver(id)
            .get()
            .is_some_and(|r| matches!(self.kind(r), NodeKind::SelfExpr))
    }

    /// `const_receiver?` — the call's receiver is a constant. Mirrors
    /// RuboCop's `node.const_receiver?` (`receiver&.const_type?`).
    pub fn is_const_receiver(&self, id: NodeId) -> bool {
        self.call_receiver(id)
            .get()
            .is_some_and(|r| matches!(self.kind(r), NodeKind::Const { .. }))
    }

    /// `command?(name)` — a receiverless `Send` whose selector is `name`.
    /// Mirrors RuboCop's `node.command?(name)` (`!receiver && method?(name)`).
    /// A `Csend` always has a receiver, so it is never a command.
    pub fn is_command(&self, id: NodeId, name: &str) -> bool {
        matches!(*self.kind(id), NodeKind::Send { receiver, .. } if receiver.get().is_none())
            && self.method_name(id) == Some(name)
    }

    /// `negation_method?` — a call to `!` with a receiver (`!x`, parsed as
    /// `x.!`). Mirrors RuboCop's `node.negation_method?`
    /// (`receiver && method_name == :!`).
    pub fn is_negation_method(&self, id: NodeId) -> bool {
        self.call_receiver(id).get().is_some() && self.method_name(id) == Some("!")
    }

    /// `global_const?(name)` — a constant with no namespace or an explicit
    /// top-level `::` namespace. Mirrors RuboCop's
    /// `(const {nil? cbase} %1)` node pattern.
    pub fn is_global_const(&self, id: NodeId, name: &str) -> bool {
        match *self.kind(id) {
            NodeKind::Const { scope, name: sym } if self.symbol_str(sym) == name => scope
                .get()
                .is_none_or(|s| matches!(self.kind(s), NodeKind::Cbase)),
            _ => false,
        }
    }

    fn is_global_const_any(&self, id: NodeId, names: &[&str]) -> bool {
        names.iter().any(|name| self.is_global_const(id, name))
    }

    /// `class_constructor?` — `Class.new` / `Module.new` / `Struct.new`,
    /// `Data.define`, or a block wrapped around those calls. Mirrors
    /// RuboCop's hand-written use of `#global_const?` inside the
    /// `class_constructor?` node pattern.
    pub fn is_class_constructor(&self, id: NodeId) -> bool {
        match *self.kind(id) {
            NodeKind::Block { call, .. }
            | NodeKind::Numblock { send: call, .. }
            | NodeKind::Itblock { send: call, .. } => self.is_class_constructor(call),
            NodeKind::Send { receiver, .. } => {
                let Some(receiver) = receiver.get() else {
                    return false;
                };
                match self.method_name(id) {
                    Some("new") => {
                        self.is_global_const_any(receiver, &["Class", "Module", "Struct"])
                    }
                    Some("define") => self.is_global_const(receiver, "Data"),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// `in_macro_scope?` — true for root nodes, direct children of
    /// class-like scopes, and children inside RuboCop's wrapper nodes
    /// (`begin`/`kwbegin`/any block/if branches) when the wrapper is itself
    /// in macro scope. The condition child of an `if` is deliberately
    /// excluded, matching RuboCop's `(if _condition <%0 _>)` pattern.
    pub fn is_in_macro_scope(&self, id: NodeId) -> bool {
        let Some(parent) = self.parent(id).get() else {
            return true;
        };
        match self.kind(parent) {
            NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. } => true,
            _ if self.is_class_constructor(parent) => self.is_in_macro_scope(parent),
            NodeKind::Begin(..) | NodeKind::Kwbegin(..) | NodeKind::Block { .. } => {
                self.is_in_macro_scope(parent)
            }
            NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => self.is_in_macro_scope(parent),
            NodeKind::If { .. } if self.sibling_index(id) != Some(0) => {
                self.is_in_macro_scope(parent)
            }
            _ => false,
        }
    }

    /// `macro?` — a receiverless method dispatch in macro scope. Mirrors
    /// RuboCop's `!receiver && in_macro_scope?`.
    pub fn is_macro(&self, id: NodeId) -> bool {
        matches!(*self.kind(id), NodeKind::Send { receiver, .. } if receiver.get().is_none())
            && self.is_in_macro_scope(id)
    }

    fn is_access_modifier_name(name: &str) -> bool {
        matches!(name, "public" | "protected" | "private" | "module_function")
    }

    /// `bare_access_modifier?` — a macro call to an access modifier with no
    /// arguments, affecting following method definitions.
    pub fn is_bare_access_modifier(&self, id: NodeId) -> bool {
        self.is_macro(id)
            && self
                .method_name(id)
                .is_some_and(Self::is_access_modifier_name)
            && self.call_arguments(id).is_empty()
    }

    /// `non_bare_access_modifier?` — a macro call to an access modifier with
    /// at least one argument, affecting only the named methods.
    pub fn is_non_bare_access_modifier(&self, id: NodeId) -> bool {
        self.is_macro(id)
            && self
                .method_name(id)
                .is_some_and(Self::is_access_modifier_name)
            && !self.call_arguments(id).is_empty()
    }

    /// `access_modifier?` — bare or non-bare `public`/`protected`/`private`/
    /// `module_function` in macro scope.
    pub fn is_access_modifier(&self, id: NodeId) -> bool {
        self.is_bare_access_modifier(id) || self.is_non_bare_access_modifier(id)
    }

    /// `special_modifier?` — a bare `private` or `protected` modifier.
    pub fn is_special_modifier(&self, id: NodeId) -> bool {
        self.is_bare_access_modifier(id)
            && matches!(self.method_name(id), Some("private" | "protected"))
    }

    fn is_any_def(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Def { .. } | NodeKind::Defs { .. })
    }

    /// `def_modifier` — for `private def foo; end` and nested modifier
    /// chains, returns the `def`/`defs` node being modified. Mirrors
    /// RuboCop's recursive `def_modifier(node = self)` helper.
    pub fn def_modifier(&self, id: NodeId) -> OptNodeId {
        if !matches!(*self.kind(id), NodeKind::Send { receiver, .. } if receiver.get().is_none()) {
            return OptNodeId::NONE;
        }
        let Some(&arg) = self.call_arguments(id).first() else {
            return OptNodeId::NONE;
        };
        if self.is_any_def(arg) {
            OptNodeId::some(arg)
        } else {
            self.def_modifier(arg)
        }
    }

    /// `def_modifier?` — whether this send participates in a modifier chain
    /// for a `def`/`defs` node.
    pub fn is_def_modifier(&self, id: NodeId) -> bool {
        self.def_modifier(id).get().is_some()
    }

    /// `dot?` — the call uses the `.` operator. Mirrors RuboCop's
    /// `node.dot?` (`loc_is?(:dot, '.')`). [`LocRef::dot`] returns Prism's
    /// parser-provided call operator (`.` or `&.`), so this checks the range's
    /// source text and returns `false` for safe navigation and operator sends.
    pub fn is_dot(&self, id: NodeId) -> bool {
        let dot = self.loc(id).dot();
        dot != Range::ZERO && self.raw_source(dot) == "."
    }

    /// `safe_navigation?` — the call uses `&.`. Mirrors RuboCop's
    /// `node.safe_navigation?` (`loc_is?(:dot, '&.')`); Murphy models
    /// safe-navigation as the distinct [`NodeKind::Csend`] variant, so
    /// this is a kind check (no loc scan needed).
    pub fn is_safe_navigation(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Csend { .. })
    }

    /// `parenthesized?` — the call's argument list is wrapped in parens.
    /// Mirrors RuboCop's `parenthesized?` (`loc_is?(:end, ')')`) using
    /// Prism's parser-provided `CallNode::closing_loc()` recorded by the
    /// translator. This intentionally differs from token scanning: command
    /// calls like `foo (1)` have a parenthesized argument, not a
    /// parenthesized call, so they have no call closing loc.
    pub fn is_parenthesized(&self, id: NodeId) -> bool {
        self.call_closing_locs()
            .binary_search_by_key(&id.0, |entry| entry.node.0)
            .is_ok()
    }

    /// `prefix_not?` — a negation written as the `not` keyword (`not x`).
    /// Mirrors RuboCop's `prefix_not?`
    /// (`negation_method? && loc.selector.is?('not')`); the selector
    /// range is Murphy's `loc.name`.
    pub fn is_prefix_not(&self, id: NodeId) -> bool {
        self.is_negation_method(id) && self.raw_source(self.loc(id).name) == "not"
    }

    /// `prefix_bang?` — a negation written as `!` (`!x`). Mirrors
    /// RuboCop's `prefix_bang?` (`negation_method? && loc.selector.is?('!')`).
    pub fn is_prefix_bang(&self, id: NodeId) -> bool {
        self.is_negation_method(id) && self.raw_source(self.loc(id).name) == "!"
    }

    /// `literal?` — the node is one of RuboCop's `LITERALS`
    /// (`TRUTHY_LITERALS + FALSEY_LITERALS`): string/xstring/dstring,
    /// symbol/dsymbol, integer/float/rational/complex, array, hash,
    /// regexp (+ its `regopt`), range, and `true`/`false`/`nil`.
    ///
    /// RuboCop distinguishes `irange`/`erange`; Murphy folds both into
    /// [`NodeKind::RangeExpr`], which is sound here because both are
    /// literals.
    pub fn is_literal(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Str(..)
                | NodeKind::Dstr(..)
                | NodeKind::Xstr(..)
                | NodeKind::Int(..)
                | NodeKind::Float(..)
                | NodeKind::Sym(..)
                | NodeKind::Dsym(..)
                | NodeKind::Array(..)
                | NodeKind::Hash(..)
                | NodeKind::Regexp { .. }
                | NodeKind::Regopt(..)
                | NodeKind::True_
                | NodeKind::False_
                | NodeKind::Nil
                | NodeKind::RangeExpr { .. }
                | NodeKind::Rational(..)
                | NodeKind::Complex(..)
        )
    }

    /// `basic_literal?` — a **non-composite** literal: RuboCop's
    /// `BASIC_LITERALS` (`LITERALS - COMPOSITE_LITERALS`) =
    /// string/integer/float/symbol/`true`/`false`/`nil`/complex/rational
    /// and the regexp-options `regopt`. Composite literals (array, hash,
    /// dstr, range, …) are **not** basic literals.
    pub fn is_basic_literal(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Str(..)
                | NodeKind::Int(..)
                | NodeKind::Float(..)
                | NodeKind::Sym(..)
                | NodeKind::True_
                | NodeKind::False_
                | NodeKind::Nil
                | NodeKind::Complex(..)
                | NodeKind::Rational(..)
                | NodeKind::Regopt(..)
        )
    }

    /// `numeric_type?` — an integer, float, rational, or complex literal.
    pub fn is_numeric(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Int(..)
                | NodeKind::Float(..)
                | NodeKind::Rational(..)
                | NodeKind::Complex(..)
        )
    }

    /// `reference?` — a regexp reference: a numbered (`$1`) or back
    /// (`$&`/`$~`) reference read.
    pub fn is_reference(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::NthRef(..) | NodeKind::BackRef(..))
    }

    /// `DefNode#argument_forwarding?` — the method is defined with a
    /// forward-all `...` parameter (`def f(...)`). `false` for a non-def
    /// node. Mirrors RuboCop's `DefNode#argument_forwarding?`.
    pub fn is_argument_forwarding(&self, id: NodeId) -> bool {
        let args = match *self.kind(id) {
            NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => args,
            _ => return false,
        };
        self.children(args)
            .iter()
            .any(|&c| matches!(self.kind(c), NodeKind::ForwardArgs))
    }

    /// `recursive_basic_literal?` — the node is a basic literal, a
    /// recursively-composed literal (`[1, 2]`, `{a: 1}`, `1..2`, `"a" "b"`,
    /// `a && b`, …) whose every child is itself a recursive basic literal,
    /// or a literal-operator send (`1 == 2`, `1 * 2`, `!true`) whose
    /// receiver and arguments are all recursive basic literals.
    ///
    /// Faithful to RuboCop's `def_recursive_literal_predicate :basic_literal`:
    /// - `send` arm — selector ∈ `LITERAL_RECURSIVE_METHODS`
    ///   (`COMPARISON_OPERATORS + [*, !, <=>]`) and receiver + args recurse;
    /// - `LITERAL_RECURSIVE_TYPES` arm — `and`/`or`/`dstr`/`xstr`/`dsym`/
    ///   `array`/`hash`/`irange`/`erange`/`regexp`/`begin`/`pair` recurse over
    ///   children (Murphy folds `irange`/`erange` into [`NodeKind::RangeExpr`];
    ///   `begin` is [`NodeKind::Begin`], **not** `kwbegin`);
    /// - otherwise — [`Self::is_basic_literal`].
    pub fn is_recursive_basic_literal(&self, id: NodeId) -> bool {
        const LITERAL_RECURSIVE_METHODS: &[&str] =
            &["==", "===", "!=", "<=", ">=", ">", "<", "*", "!", "<=>"];
        match self.kind(id) {
            NodeKind::Send { .. } | NodeKind::Csend { .. } => {
                let Some(name) = self.method_name(id) else {
                    return false;
                };
                LITERAL_RECURSIVE_METHODS.contains(&name)
                    && self
                        .call_receiver(id)
                        .get()
                        .is_some_and(|r| self.is_recursive_basic_literal(r))
                    && self
                        .call_arguments(id)
                        .iter()
                        .all(|&a| self.is_recursive_basic_literal(a))
            }
            NodeKind::And { .. }
            | NodeKind::Or { .. }
            | NodeKind::Dstr(..)
            | NodeKind::Xstr(..)
            | NodeKind::Dsym(..)
            | NodeKind::Array(..)
            | NodeKind::Hash(..)
            | NodeKind::RangeExpr { .. }
            | NodeKind::Regexp { .. }
            | NodeKind::Begin(..)
            | NodeKind::Pair { .. } => self
                .children(id)
                .iter()
                .all(|&c| self.is_recursive_basic_literal(c)),
            _ => self.is_basic_literal(id),
        }
    }

    /// `mutable_literal?` — RuboCop's `MUTABLE_LITERALS`
    /// (`str dstr xstr array hash regexp irange erange`): a literal whose
    /// evaluation yields a fresh mutable object. Murphy folds
    /// `irange`/`erange` into [`NodeKind::RangeExpr`] (both mutable, so the
    /// fold is sound).
    pub fn is_mutable_literal(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Str(..)
                | NodeKind::Dstr(..)
                | NodeKind::Xstr(..)
                | NodeKind::Array(..)
                | NodeKind::Hash(..)
                | NodeKind::Regexp { .. }
                | NodeKind::RangeExpr { .. }
        )
    }

    /// `immutable_literal?` — RuboCop's `IMMUTABLE_LITERALS`
    /// (`LITERALS - MUTABLE_LITERALS`): a literal that is not mutable
    /// (numerics, `sym`/`dsym`, `true`/`false`/`nil`, `regopt`). Defined as
    /// the set complement so the partition stays exact by construction.
    pub fn is_immutable_literal(&self, id: NodeId) -> bool {
        self.is_literal(id) && !self.is_mutable_literal(id)
    }

    /// `falsey_literal?` — RuboCop's `FALSEY_LITERALS` (`false`, `nil`).
    pub fn is_falsey_literal(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::False_ | NodeKind::Nil)
    }

    /// `truthy_literal?` — RuboCop's `TRUTHY_LITERALS`: every literal except
    /// `false`/`nil`. Defined as `literal? && !falsey_literal?`, pinning
    /// `LITERALS = TRUTHY ⊔ FALSEY`.
    pub fn is_truthy_literal(&self, id: NodeId) -> bool {
        self.is_literal(id) && !self.is_falsey_literal(id)
    }

    /// `recursive_literal?` — RuboCop's
    /// `def_recursive_literal_predicate :literal`: the `literal?` twin of
    /// [`Self::is_recursive_basic_literal`]. Same `send`/composite recursion,
    /// but the leaf arm is [`Self::is_literal`] instead of `basic_literal?`,
    /// so composite literals (`array`/`hash`/range/…) qualify at the leaf.
    pub fn is_recursive_literal(&self, id: NodeId) -> bool {
        const LITERAL_RECURSIVE_METHODS: &[&str] =
            &["==", "===", "!=", "<=", ">=", ">", "<", "*", "!", "<=>"];
        match self.kind(id) {
            NodeKind::Send { .. } | NodeKind::Csend { .. } => {
                let Some(name) = self.method_name(id) else {
                    return false;
                };
                LITERAL_RECURSIVE_METHODS.contains(&name)
                    && self
                        .call_receiver(id)
                        .get()
                        .is_some_and(|r| self.is_recursive_literal(r))
                    && self
                        .call_arguments(id)
                        .iter()
                        .all(|&a| self.is_recursive_literal(a))
            }
            NodeKind::And { .. }
            | NodeKind::Or { .. }
            | NodeKind::Dstr(..)
            | NodeKind::Xstr(..)
            | NodeKind::Dsym(..)
            | NodeKind::Array(..)
            | NodeKind::Hash(..)
            | NodeKind::RangeExpr { .. }
            | NodeKind::Regexp { .. }
            | NodeKind::Begin(..)
            | NodeKind::Pair { .. } => self
                .children(id)
                .iter()
                .all(|&c| self.is_recursive_literal(c)),
            _ => self.is_literal(id),
        }
    }

    /// `operator_keyword?` — RuboCop's `OPERATOR_KEYWORDS` (`and`, `or`).
    pub fn is_operator_keyword(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::And { .. } | NodeKind::Or { .. })
    }

    /// `post_condition_loop?` — RuboCop's `POST_CONDITION_LOOP_TYPES`
    /// (`while_post`, `until_post`): a do-while / do-until. Murphy folds the
    /// post forms into [`NodeKind::While`]/[`NodeKind::Until`] with
    /// `post: true`.
    pub fn is_post_condition_loop(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::While { post: true, .. } | NodeKind::Until { post: true, .. }
        )
    }

    /// `loop_keyword?` — RuboCop's `LOOP_TYPES` (`while until for` + the post
    /// forms): any `while`/`until`/`for`, regardless of pre/post form.
    pub fn is_loop_keyword(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::While { .. } | NodeKind::Until { .. } | NodeKind::For { .. }
        )
    }

    /// `WhileNode#inverse_keyword` / `UntilNode#inverse_keyword`: the opposite
    /// loop keyword — `until` for a `while`, `while` for an `until`, and empty
    /// for anything else. Unlike [`Self::if_inverse_keyword`] this is pure
    /// `NodeKind` dispatch, not a token-text lookup: the modifier form
    /// (`x while y`) has no `loc.keyword()`, so dispatch on the folded
    /// `While`/`Until` variant is the only faithful path. `for` has no inverse
    /// in RuboCop, so it returns empty.
    pub fn loop_inverse_keyword(&self, id: NodeId) -> &'static str {
        match self.kind(id) {
            NodeKind::While { .. } => "until",
            NodeKind::Until { .. } => "while",
            _ => "",
        }
    }

    /// `void_context?` — RuboCop defines this on four typed nodes; every other
    /// node is `false`. Verbatim (rubocop-ast):
    /// - `DefNode`: `(def_type? && method?(:initialize)) || assignment_method?`
    /// - `ForNode`: `true`
    /// - `BlockNode`: `VOID_CONTEXT_METHODS.include?(method_name)` where
    ///   `VOID_CONTEXT_METHODS = %i[each tap]` (the BlockNode class also backs
    ///   numblocks and itblocks, so Murphy's `Numblock`/`Itblock` share it)
    /// - `EnsureNode`: `true`
    ///
    /// `def_type?` distinguishes an instance `def` from a singleton `defs`.
    /// Murphy folds `def self.foo` into [`NodeKind::Def`] with a `receiver`, so
    /// `def_type?` is "receiver is absent"; a present receiver (or the
    /// [`NodeKind::Defs`] variant) is `defs` and fails the `initialize` clause
    /// but still honours the `assignment_method?` clause (`def self.foo=`).
    pub fn is_void_context(&self, id: NodeId) -> bool {
        match *self.kind(id) {
            NodeKind::Def { receiver, name, .. } => {
                let def_type = receiver.is_none();
                let name = self.symbol_str(name);
                (def_type && name == "initialize")
                    || crate::method_predicates::is_assignment_method(name)
            }
            NodeKind::Defs { name, .. } => {
                crate::method_predicates::is_assignment_method(self.symbol_str(name))
            }
            NodeKind::For { .. } | NodeKind::Ensure { .. } => true,
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => self
                .method_name(id)
                .is_some_and(|name| name == "each" || name == "tap"),
            _ => false,
        }
    }

    /// `basic_conditional?` — RuboCop's `BASIC_CONDITIONALS`
    /// (`if while until`). RuboCop's set excludes `while_post`/`until_post`,
    /// so Murphy's folded `While`/`Until` qualify only with `post: false`.
    pub fn is_basic_conditional(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::If { .. }
                | NodeKind::While { post: false, .. }
                | NodeKind::Until { post: false, .. }
        )
    }

    /// `conditional?` — RuboCop's `CONDITIONALS`
    /// (`BASIC_CONDITIONALS + case case_match`). Loop forms follow the same
    /// `post: false` restriction as [`Self::is_basic_conditional`].
    pub fn is_conditional(&self, id: NodeId) -> bool {
        self.is_basic_conditional(id)
            || matches!(
                self.kind(id),
                NodeKind::Case { .. } | NodeKind::CaseMatch { .. }
            )
    }

    /// `boolean_type?` — RuboCop's `:boolean` group (`true`, `false`).
    pub fn is_boolean_type(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::True_ | NodeKind::False_)
    }

    /// `range_type?` — RuboCop's `:range` group (`irange`, `erange`), folded
    /// into [`NodeKind::RangeExpr`] in Murphy.
    pub fn is_range_type(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::RangeExpr { .. })
    }

    /// `any_block_type?` — RuboCop's `:any_block` group
    /// (`block`, `numblock`, `itblock`).
    pub fn is_any_block_type(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
        )
    }

    /// `any_def_type?` — RuboCop's `:any_def` group (`def`, `defs`).
    pub fn is_any_def_type(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Def { .. } | NodeKind::Defs { .. })
    }

    /// `variable?` — RuboCop's `VARIABLES` (`ivar gvar cvar lvar`): a
    /// variable *read* (not a write/assignment).
    pub fn is_variable(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Lvar(..) | NodeKind::Ivar(..) | NodeKind::Cvar(..) | NodeKind::Gvar(..)
        )
    }

    /// `equals_asgn?` — RuboCop's `EQUALS_ASSIGNMENTS`
    /// (`lvasgn ivasgn cvasgn gvasgn casgn masgn`): a plain `=` write.
    pub fn is_equals_asgn(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Lvasgn { .. }
                | NodeKind::Ivasgn { .. }
                | NodeKind::Cvasgn { .. }
                | NodeKind::Gvasgn { .. }
                | NodeKind::Casgn { .. }
                | NodeKind::Masgn { .. }
        )
    }

    /// `shorthand_asgn?` — RuboCop's `SHORTHAND_ASSIGNMENTS`
    /// (`op_asgn or_asgn and_asgn`): `+=`, `||=`, `&&=`, etc.
    pub fn is_shorthand_asgn(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
        )
    }

    /// `assignment?` — RuboCop's `ASSIGNMENTS`
    /// (`EQUALS_ASSIGNMENTS ⊔ SHORTHAND_ASSIGNMENTS`). Does **not** include
    /// `index_asgn` — RuboCop's set is exactly these nine kinds.
    pub fn is_assignment(&self, id: NodeId) -> bool {
        self.is_equals_asgn(id) || self.is_shorthand_asgn(id)
    }

    /// `chained?` — RuboCop's `chained?`
    /// (`parent&.call_type? && eql?(parent.receiver)`): the node is the
    /// receiver of its parent call (`Send` *or* `Csend`). Identity is by
    /// `NodeId`.
    pub fn is_chained(&self, id: NodeId) -> bool {
        let Some(parent) = self.parent(id).get() else {
            return false;
        };
        matches!(
            self.kind(parent),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) && self.call_receiver(parent).get() == Some(id)
    }

    /// `argument?` — RuboCop's `argument?`
    /// (`parent&.send_type? && parent.arguments.include?(self)`): the node is
    /// an argument of its parent `Send`. Note the asymmetry with
    /// [`Self::is_chained`] — RuboCop restricts this to `send` (not `csend`).
    pub fn is_argument(&self, id: NodeId) -> bool {
        let Some(parent) = self.parent(id).get() else {
            return false;
        };
        if !matches!(self.kind(parent), NodeKind::Send { .. }) {
            return false;
        }
        self.call_arguments(parent).contains(&id)
    }

    /// `source_length` — RuboCop's `Node#source_length`
    /// (`source_range ? source_range.size : 0`): the node's expression length
    /// in **characters** (not bytes). A zero range is `0`.
    pub fn source_length(&self, id: NodeId) -> usize {
        let r = self.range(id);
        if r == Range::ZERO {
            return 0;
        }
        self.raw_source(r).chars().count()
    }

    /// `const_name` — RuboCop's `Node#const_name` for `const`/`casgn`: the
    /// fully-qualified constant name joined by `::`, recursing through the
    /// scope. `None` for any other node kind.
    ///
    /// **Murphy divergence:** the translator folds a top-level `::Foo` path
    /// to `Const { scope: None }` (no `cbase` scope node), so `::Foo` reports
    /// `"Foo"` — identical to RuboCop's output, which also drops the leading
    /// `::`. A `cbase` scope (should one appear) is likewise dropped.
    pub fn const_name(&self, id: NodeId) -> Option<String> {
        let (scope, name) = match *self.kind(id) {
            NodeKind::Const { scope, name } => (scope, name),
            NodeKind::Casgn { scope, name, .. } => (scope, name),
            _ => return None,
        };
        let short = self.symbol_str(name);
        // RuboCop: `if namespace && !namespace.cbase_type?` → join, else short.
        match scope.get() {
            Some(s) if !matches!(self.kind(s), NodeKind::Cbase) => {
                // RuboCop interpolates `namespace.const_name` (nil → "") for a
                // non-const namespace, yielding a leading `::`.
                let prefix = self.const_name(s).unwrap_or_default();
                Some(format!("{prefix}::{short}"))
            }
            _ => Some(short.to_string()),
        }
    }

    /// `double_colon?` — **Murphy-specific** (no rubocop-ast equivalent): a
    /// call (`Send`/`Csend`) whose call operator is `::` rather than `.`/`&.`
    /// (e.g. `Foo::bar`). Decided by the delimited operator gap between the
    /// receiver's expression end and the selector's `loc.name` start — its
    /// trimmed text equals `"::"`. `false` for receiverless calls,
    /// operator/index methods, and non-call kinds.
    pub fn is_double_colon(&self, id: NodeId) -> bool {
        if !matches!(
            self.kind(id),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            return false;
        }
        let Some(recv) = self.call_receiver(id).get() else {
            return false;
        };
        let name = self.loc(id).name;
        let recv_end = self.range(recv).end;
        if name.start <= recv_end {
            return false;
        }
        let gap = Range {
            start: recv_end,
            end: name.start,
        };
        self.raw_source(gap).trim() == "::"
    }

    /// `arithmetic_operation?` — RuboCop's
    /// `MethodDispatchNode#arithmetic_operation?`
    /// (`ARITHMETIC_OPERATORS.include?(method_name)`, where
    /// `ARITHMETIC_OPERATORS = %i[+ - * / % **]`): a `Send`/`Csend` (or block
    /// delegating to one) whose selector is a binary arithmetic operator.
    pub fn is_arithmetic_operation(&self, id: NodeId) -> bool {
        const ARITHMETIC_OPERATORS: &[&str] = &["+", "-", "*", "/", "%", "**"];
        self.method_name(id)
            .is_some_and(|name| ARITHMETIC_OPERATORS.contains(&name))
    }

    /// `pure?` — the node is free of side effects: a pure value leaf
    /// (literals, variable reads, `const`, `defined?`) or a composite
    /// (`and`/`or`/`if`/`case`/`begin`/`array`/`hash`/range/`while`/… ) all
    /// of whose child nodes are themselves pure. Mirrors RuboCop's
    /// `Node#pure?`.
    ///
    /// **Divergences (Murphy translator gaps, documented):** `__FILE__` /
    /// `__LINE__` and flip-flops parse to [`NodeKind::Unknown`] in Murphy,
    /// so they fall through to `false` where RuboCop would treat the former
    /// as pure and recurse into the latter. `until_post`/`while_post` are
    /// folded into [`NodeKind::Until`]/[`NodeKind::While`] (a `post` flag),
    /// so both forms are covered.
    pub fn is_pure(&self, id: NodeId) -> bool {
        match self.kind(id) {
            // Pure value leaves — always pure.
            NodeKind::Const { .. }
            | NodeKind::Cvar(..)
            | NodeKind::Defined(..)
            | NodeKind::False_
            | NodeKind::Float(..)
            | NodeKind::Gvar(..)
            | NodeKind::Int(..)
            | NodeKind::Ivar(..)
            | NodeKind::Lvar(..)
            | NodeKind::Nil
            | NodeKind::Str(..)
            | NodeKind::Sym(..)
            | NodeKind::True_
            | NodeKind::Regopt(..) => true,
            // Composites — pure iff every child node is pure.
            NodeKind::And { .. }
            | NodeKind::Or { .. }
            | NodeKind::Array(..)
            | NodeKind::Begin(..)
            | NodeKind::Kwbegin(..)
            | NodeKind::Case { .. }
            | NodeKind::Dstr(..)
            | NodeKind::Dsym(..)
            | NodeKind::Ensure { .. }
            | NodeKind::RangeExpr { .. }
            | NodeKind::For { .. }
            | NodeKind::Hash(..)
            | NodeKind::If { .. }
            | NodeKind::Not(..)
            | NodeKind::Pair { .. }
            | NodeKind::Regexp { .. }
            | NodeKind::Until { .. }
            | NodeKind::When { .. }
            | NodeKind::While { .. } => self.children(id).iter().all(|&c| self.is_pure(c)),
            _ => false,
        }
    }

    /// The parser-gem child **slots** of `id` (see
    /// [`murphy_ast::slot_layout`]): one entry per slot, `Some(child)` for a
    /// present node child and `None` for a phantom (selector / name / operator
    /// symbol, numblock count …) or absent optional slot. Positions match
    /// RuboCop's `node.children`.
    pub fn slot_layout(&self, id: NodeId) -> Vec<Option<NodeId>> {
        let mut out = Vec::new();
        slot_layout(self.kind(id), self.lists(), &mut out);
        out
    }

    /// `Node#sibling_index` — the zero-based position of `id` within its
    /// parent's parser-gem child-slot array, or `None` if `id` is the root.
    ///
    /// Faithful to RuboCop: the index counts **phantom** slots (the `:selector`
    /// of a `send`, the def/casgn name symbol, the `op_asgn` operator, the
    /// `numblock` count) and **absent** optional slots, so e.g. the sole
    /// argument of `foo(1)` reports index 2 (receiver slot 0 + selector slot 1),
    /// exactly as parser-gem's `node.children.index` would.
    pub fn sibling_index(&self, id: NodeId) -> Option<usize> {
        let parent = self.parent(id).get()?;
        self.slot_layout(parent)
            .iter()
            .position(|slot| *slot == Some(id))
    }

    /// `Node#left_sibling` — the node immediately preceding `id` among its
    /// parent's child slots, or `None` if `id` is the root, is the first slot,
    /// or the preceding slot is a phantom/absent (non-node) slot.
    pub fn left_sibling(&self, id: NodeId) -> OptNodeId {
        let Some(parent) = self.parent(id).get() else {
            return OptNodeId::NONE;
        };
        let slots = self.slot_layout(parent);
        let Some(index) = slots.iter().position(|slot| *slot == Some(id)) else {
            return OptNodeId::NONE;
        };
        match index.checked_sub(1).and_then(|i| slots[i]) {
            Some(node) => OptNodeId::some(node),
            None => OptNodeId::NONE,
        }
    }

    /// `Node#right_sibling` — the node immediately following `id` among its
    /// parent's child slots, or `None` if `id` is the root, is the last slot,
    /// or the following slot is a phantom/absent (non-node) slot.
    pub fn right_sibling(&self, id: NodeId) -> OptNodeId {
        let Some(parent) = self.parent(id).get() else {
            return OptNodeId::NONE;
        };
        let slots = self.slot_layout(parent);
        let Some(index) = slots.iter().position(|slot| *slot == Some(id)) else {
            return OptNodeId::NONE;
        };
        match slots.get(index + 1).copied().flatten() {
            Some(node) => OptNodeId::some(node),
            None => OptNodeId::NONE,
        }
    }

    /// `value_used?` — whether the result of evaluating `id` is consumed
    /// by its surrounding context (vs discarded as a void statement).
    /// Mirrors RuboCop's `Node#value_used?`: a parent-context walk —
    /// pass-through containers (array/hash/pair/range/dstr/… and `defined?`)
    /// delegate to the parent; `begin`/`kwbegin` use only their last child;
    /// `if`/`case` use the condition (index 0) or whatever uses the parent;
    /// `while`/`until` use only the condition; `for` uses the body
    /// (index 2); everything else uses the value. A root node's value is
    /// unused.
    ///
    /// `while_post`/`until_post` fold into [`NodeKind::While`]/
    /// [`NodeKind::Until`]; flip-flops parse to [`NodeKind::Unknown`]
    /// (handled by the `_ => true` arm, as RuboCop's pass-through would).
    pub fn is_value_used(&self, id: NodeId) -> bool {
        let Some(parent) = self.parent(id).get() else {
            return false;
        };
        match self.kind(parent) {
            // Pass-through containers: used iff the container's value is used.
            NodeKind::Array(..)
            | NodeKind::Defined(..)
            | NodeKind::Dstr(..)
            | NodeKind::Dsym(..)
            | NodeKind::RangeExpr { .. }
            | NodeKind::Float(..)
            | NodeKind::Hash(..)
            | NodeKind::Not(..)
            | NodeKind::Pair { .. }
            | NodeKind::Regexp { .. }
            | NodeKind::Str(..)
            | NodeKind::Sym(..)
            | NodeKind::When { .. }
            | NodeKind::Xstr(..) => self.is_value_used(parent),
            // begin/kwbegin: only the last child's value is the block's value.
            NodeKind::Begin(..) | NodeKind::Kwbegin(..) => {
                self.children(parent).last() == Some(&id) && self.is_value_used(parent)
            }
            // for var in enum; body; end → the body (index 2) flows to parent;
            // the var/enum (index 0/1) are used by the loop construct.
            NodeKind::For { .. } if self.sibling_index(id) == Some(2) => self.is_value_used(parent),
            NodeKind::For { .. } => true,
            // if/case: the condition (index 0) is used; branches flow to parent.
            NodeKind::If { .. } | NodeKind::Case { .. } => {
                self.sibling_index(id) == Some(0) || self.is_value_used(parent)
            }
            // while/until evaluate to nil: only the condition (index 0) is used.
            NodeKind::While { .. } | NodeKind::Until { .. } => self.sibling_index(id) == Some(0),
            _ => true,
        }
    }

    /// The number of source lines the node's expression range spans —
    /// Murphy's analog of RuboCop's `node.line_count`
    /// (`last_line - first_line + 1`), computed from the expression
    /// range's source text.
    fn line_count(&self, id: NodeId) -> usize {
        self.raw_source(self.range(id)).matches('\n').count() + 1
    }

    /// `single_line?` — the node's expression spans exactly one line.
    pub fn is_single_line(&self, id: NodeId) -> bool {
        self.line_count(id) == 1
    }

    /// `multiline?` — the node's expression spans more than one line.
    pub fn is_multiline(&self, id: NodeId) -> bool {
        self.line_count(id) > 1
    }

    // --- typed-node accessors (pure field projections) ---
    //
    // Each returns the relevant child of a specific node kind, or the
    // empty value (`OptNodeId::NONE` / `&[]`) when `id` is a different
    // kind, so a cop can call them without a prior kind check. Mirrors
    // the accessor methods on RuboCop's typed `IfNode` / `HashNode` /
    // `PairNode` / `BlockNode`.

    /// `IfNode#condition` — the `if`/`unless`/ternary condition.
    pub fn if_condition(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { cond, .. } => OptNodeId::some(cond),
            _ => OptNodeId::NONE,
        }
    }

    /// `IfNode#if_branch` — the `then` branch (the body run when the
    /// condition holds). `OptNodeId::NONE` if absent or not an `If`.
    pub fn if_then_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { then_, .. } => then_,
            _ => OptNodeId::NONE,
        }
    }

    /// `IfNode#else_branch` — the `else` branch. `OptNodeId::NONE` if
    /// absent or not an `If`.
    pub fn if_else_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { else_, .. } => else_,
            _ => OptNodeId::NONE,
        }
    }

    fn find_if_token_text(&self, id: NodeId, texts: &[&str]) -> Range {
        if !matches!(self.kind(id), NodeKind::If { .. }) {
            return Range::ZERO;
        }
        let children = self.children(id);
        for tok in self.tokens_in(self.range(id)) {
            let outside_children = children.iter().all(|&child| {
                let r = self.range(child);
                tok.range.start < r.start || tok.range.end > r.end
            });
            if outside_children && texts.contains(&self.token_text(*tok)) {
                return tok.range;
            }
        }
        Range::ZERO
    }

    /// `IfNode#keyword` token range (`if`, `unless`, or `elsif`). The scan is
    /// bounded to this `If` node and ignores direct child ranges, so nested
    /// conditionals in the predicate/branches cannot contaminate the result.
    pub fn if_keyword_loc(&self, id: NodeId) -> Range {
        self.find_if_token_text(id, &["if", "unless", "elsif"])
    }

    /// `IfNode#keyword` (`"if"`, `"unless"`, or `"elsif"`), or `""` for
    /// non-`If` nodes / malformed ranges.
    pub fn if_keyword(&self, id: NodeId) -> &'a str {
        let loc = self.if_keyword_loc(id);
        if loc == Range::ZERO {
            ""
        } else {
            self.raw_source(loc)
        }
    }

    /// `IfNode#inverse_keyword`: `unless` for `if`, `if` for `unless`, and
    /// empty for `elsif` / ternary-like malformed ranges.
    pub fn if_inverse_keyword(&self, id: NodeId) -> &'static str {
        match self.if_keyword(id) {
            "if" => "unless",
            "unless" => "if",
            _ => "",
        }
    }

    /// `if?` — source keyword is `if`, not `elsif`/`unless`.
    pub fn is_if(&self, id: NodeId) -> bool {
        self.if_keyword(id) == "if"
    }

    /// `unless?` — source keyword is `unless`.
    pub fn is_unless(&self, id: NodeId) -> bool {
        self.if_keyword(id) == "unless"
    }

    /// `elsif?` — source keyword is `elsif`.
    pub fn is_elsif(&self, id: NodeId) -> bool {
        self.if_keyword(id) == "elsif"
    }

    /// `then?` — this `If` node has a source-level `then` separator.
    pub fn is_then(&self, id: NodeId) -> bool {
        self.find_if_token_text(id, &["then"]) != Range::ZERO
    }

    /// `else?` — this `If` node has a source-level `else` keyword.
    pub fn is_else(&self, id: NodeId) -> bool {
        self.find_if_token_text(id, &["else"]) != Range::ZERO
            || self
                .if_else_branch(id)
                .get()
                .is_some_and(|else_| self.is_elsif(else_))
    }

    /// `nested_conditional?` — shallowly scans this node's branches for nested
    /// non-`elsif` `If` nodes.
    pub fn is_nested_conditional(&self, id: NodeId) -> bool {
        if !matches!(self.kind(id), NodeKind::If { .. }) {
            return false;
        }
        [self.if_branch(id), self.else_branch(id)]
            .into_iter()
            .filter_map(OptNodeId::get)
            .any(|branch| {
                (matches!(self.kind(branch), NodeKind::If { .. }) && !self.is_elsif(branch))
                    || self.descendants(branch).into_iter().any(|nested| {
                        matches!(self.kind(nested), NodeKind::If { .. }) && !self.is_elsif(nested)
                    })
            })
    }

    /// `IfNode#if_branch` — the condition-true branch. For `unless`, the
    /// translator has already applied parser-gem's then/else swap.
    pub fn if_branch(&self, id: NodeId) -> OptNodeId {
        self.if_then_branch(id)
    }

    /// `IfNode#else_branch` — the condition-false branch. For `unless`, the
    /// translator has already applied parser-gem's then/else swap.
    pub fn else_branch(&self, id: NodeId) -> OptNodeId {
        self.if_else_branch(id)
    }

    /// First token in the **gap** `[from, to)` whose source text is
    /// exactly `text`. Prism delimits tokens, so this is an exact-token
    /// match (`b"="` never matches the `=` in `==` or inside a string).
    /// Callers pass a gap *between two sibling child ranges*, which is
    /// what keeps punctuation lookups faithful: a `?`/`:`/`=` nested
    /// deeper in a child subtree falls outside the gap and is never
    /// matched. `Range::ZERO` if the gap is empty or holds no such token.
    fn find_token_text_in(&self, from: u32, to: u32, text: &[u8]) -> Range {
        if from >= to {
            return Range::ZERO;
        }
        let toks = self.sorted_tokens();
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        let idx = toks.partition_point(|t| t.range.start < from);
        for tok in &toks[idx..] {
            if tok.range.start >= to {
                break;
            }
            if &src[tok.range.start as usize..tok.range.end as usize] == text {
                return tok.range;
            }
        }
        Range::ZERO
    }

    /// The assignment-operator `=` of an attribute-write send
    /// (`obj.foo = x`, including the `obj.foo=(x)` spelling which prism
    /// parses as the same attribute write) — the parser-gem
    /// `loc.operator` analog — or `Range::ZERO`. Searched only in the gap
    /// between the selector and the first argument (the assigned value),
    /// so a `=` deeper inside an argument (`foo(a = 1)`) is never
    /// mistaken for it. This is the faithful signal behind
    /// `setter_method?` (`loc?(:operator)`), beyond the name-based
    /// [`Self::is_assignment_method`].
    pub fn assignment_operator_loc(&self, id: NodeId) -> Range {
        if !matches!(
            self.kind(id),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            return Range::ZERO;
        }
        // Read `loc.name` straight off the node — building a full `LocRef`
        // via `self.loc(id)` would run the `keyword()`/`dot()` setup we do
        // not need here (gemini review).
        let name = self.node(id).loc.name;
        let from = if name != Range::ZERO {
            name.end
        } else {
            self.range(id).start
        };
        let Some(first_arg) = self.call_arguments(id).first().copied() else {
            return Range::ZERO;
        };
        self.find_token_text_in(from, self.range(first_arg).start, b"=")
    }

    /// The ternary `?` of `a ? b : c` — the `loc.question` analog — or
    /// `Range::ZERO` for a non-ternary `if` (block-form `if`/`unless`
    /// have `then`/newline, modifier-form has the branch before the
    /// condition). Searched only in the gap between the condition and the
    /// then-branch, so a `?` inside a predicate selector or sub-expression
    /// is never matched. Presence is the faithful `IfNode#ternary?` signal.
    pub fn ternary_question_loc(&self, id: NodeId) -> Range {
        let (Some(cond), Some(then_)) =
            (self.if_condition(id).get(), self.if_then_branch(id).get())
        else {
            return Range::ZERO;
        };
        self.find_token_text_in(self.range(cond).end, self.range(then_).start, b"?")
    }

    /// The ternary `:` of `a ? b : c` — the `loc.colon` analog — or
    /// `Range::ZERO`. Searched only in the gap between the then- and
    /// else-branches, so a `:` inside the then-branch (a hash key
    /// `h(x: 1)`, a symbol, `::`) is never matched.
    pub fn ternary_colon_loc(&self, id: NodeId) -> Range {
        let (Some(then_), Some(else_)) =
            (self.if_then_branch(id).get(), self.if_else_branch(id).get())
        else {
            return Range::ZERO;
        };
        self.find_token_text_in(self.range(then_).end, self.range(else_).start, b":")
    }

    /// `setter_method?` — an attribute-write send (`obj.foo = x`). Mirrors
    /// RuboCop's `setter_method?` (`loc?(:operator)`): true iff the call
    /// carries a standalone assignment `=` operator. More precise than the
    /// name-based [`Self::is_assignment_method`] — it keys on the operator
    /// location, not just a trailing `=` in the selector.
    pub fn is_setter_method(&self, id: NodeId) -> bool {
        self.assignment_operator_loc(id) != Range::ZERO
    }

    /// `ternary?` — a ternary conditional (`a ? b : c`). Mirrors RuboCop's
    /// `IfNode#ternary?` (`loc?(:question)`).
    pub fn is_ternary(&self, id: NodeId) -> bool {
        self.ternary_question_loc(id) != Range::ZERO
    }

    /// `modifier_form?` — a modifier-form `if`/`unless`/`while`/`until`
    /// (`body if cond`), which has no closing `end` keyword. Mirrors
    /// RuboCop's `(if? || unless?) && loc.end.nil?`: a ternary (which also
    /// lacks `end`) is **excluded** via the `ternary?` guard. `unless` is
    /// an `If` node in Murphy; modifier `while`/`until` are `While`/`Until`.
    pub fn is_modifier_form(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::If { .. } | NodeKind::While { .. } | NodeKind::Until { .. }
        ) && self.ternary_question_loc(id) == Range::ZERO
            && self.loc(id).end_keyword() == Range::ZERO
    }

    /// `HashNode#pairs` — the hash's **`Pair`-type** children only.
    /// Faithful to RuboCop's `pairs` (`each_child_node(:pair)`): a
    /// `kwsplat` such as the `**h` in `{ **h, a: 1 }` is **excluded**
    /// (use [`Self::children`] for every child — verified via
    /// `murphy ast`: `{**h}` parses to `(hash (kwsplat …))`). Empty
    /// `Vec` for a non-`Hash` node. Allocates, like [`Self::children`].
    pub fn hash_pairs(&self, id: NodeId) -> Vec<NodeId> {
        match *self.kind(id) {
            NodeKind::Hash(list) => self
                .list(list)
                .iter()
                .copied()
                .filter(|&c| matches!(self.kind(c), NodeKind::Pair { .. }))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `PairNode#key`. `OptNodeId::NONE` if not a `Pair`.
    pub fn pair_key(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Pair { key, .. } => OptNodeId::some(key),
            _ => OptNodeId::NONE,
        }
    }

    /// `PairNode#value`. `OptNodeId::NONE` if not a `Pair`.
    pub fn pair_value(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Pair { value, .. } => OptNodeId::some(value),
            _ => OptNodeId::NONE,
        }
    }

    /// `PairNode#operator` — the hash pair delimiter (`=>` or `:`), or
    /// `Range::ZERO` for a non-`Pair`. Searched only in the gap between
    /// key and value, so nested delimiters inside either side cannot leak in.
    pub fn pair_operator_loc(&self, id: NodeId) -> Range {
        let (Some(key), Some(value)) = (self.pair_key(id).get(), self.pair_value(id).get()) else {
            return Range::ZERO;
        };
        let key_end = self.range(key).end;
        let value_start = self.range(value).start;
        let rocket = self.find_token_text_in(key_end, value_start, b"=>");
        if rocket != Range::ZERO {
            return rocket;
        }
        let key_range = self.range(key);
        if key_range.start < key_range.end {
            let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
            let colon_start = key_range.end - 1;
            if src[colon_start as usize] == b':' {
                return Range {
                    start: colon_start,
                    end: key_range.end,
                };
            }
        }
        self.find_token_text_in(key_end, value_start, b":")
    }

    /// `hash_rocket?` — this pair uses the `=>` delimiter.
    pub fn is_hash_rocket(&self, id: NodeId) -> bool {
        let op = self.pair_operator_loc(id);
        op != Range::ZERO && self.raw_source(op) == "=>"
    }

    /// `colon?` — this pair uses the `:` delimiter.
    pub fn is_colon(&self, id: NodeId) -> bool {
        let op = self.pair_operator_loc(id);
        op != Range::ZERO && self.raw_source(op) == ":"
    }

    /// `mixed_delimiters?` — this hash contains both `:` and `=>` pairs.
    pub fn is_mixed_delimiters(&self, id: NodeId) -> bool {
        let NodeKind::Hash(list) = *self.kind(id) else {
            return false;
        };
        let mut has_colon = false;
        let mut has_rocket = false;
        for &pair in self.list(list) {
            if !matches!(self.kind(pair), NodeKind::Pair { .. }) {
                continue;
            }
            has_colon |= self.is_colon(pair);
            has_rocket |= self.is_hash_rocket(pair);
            if has_colon && has_rocket {
                return true;
            }
        }
        false
    }

    /// `BlockNode#send_node` — the call the block is attached to.
    /// `OptNodeId::NONE` if not a `Block`.
    pub fn block_call(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { call, .. } => OptNodeId::some(call),
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#arguments` — the block's `Args` node (always present
    /// for a block, possibly empty). `OptNodeId::NONE` if not a `Block`.
    pub fn block_arguments(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { args, .. } => OptNodeId::some(args),
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#body` — the block body. `OptNodeId::NONE` for an empty
    /// body or a non-`Block` node.
    pub fn block_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { body, .. } => body,
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#lambda?` — the block is a lambda, in either spelling:
    /// the stabby `-> { … }` (a `Block` over the [`NodeKind::Lambda`]
    /// marker) or the `lambda { … }` method call (a block over a
    /// receiverless `lambda` send). `false` for an ordinary block or a
    /// non-block node.
    pub fn is_lambda(&self, id: NodeId) -> bool {
        let call = match *self.kind(id) {
            NodeKind::Block { call, .. } => call,
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
            _ => return false,
        };
        matches!(self.kind(call), NodeKind::Lambda)
            || (self.call_receiver(call).get().is_none()
                && self.method_name(call) == Some("lambda"))
    }

    /// `lambda_literal?` — the stabby `-> { … }` lambda specifically (not
    /// the `lambda { … }` method form). True only when the block's call is
    /// the [`NodeKind::Lambda`] marker.
    pub fn is_lambda_literal(&self, id: NodeId) -> bool {
        let call = match *self.kind(id) {
            NodeKind::Block { call, .. } => call,
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
            _ => return false,
        };
        matches!(self.kind(call), NodeKind::Lambda)
    }

    /// `numblock_type?` — a numbered-parameter block (`foo { _1 }`).
    pub fn is_numblock(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Numblock { .. })
    }

    /// `itblock_type?` — an `it`-parameter block (`foo { it }`, Ruby 3.4).
    pub fn is_itblock(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Itblock { .. })
    }

    /// `BlockNode#numbered_arguments?` — the highest numbered parameter
    /// (`_2` → 2) of a numbered block, or `None` for a non-numblock node.
    pub fn numblock_max(&self, id: NodeId) -> Option<u8> {
        match *self.kind(id) {
            NodeKind::Numblock { max_n, .. } => Some(max_n),
            _ => None,
        }
    }

    /// `ArrayNode#values` — the array's element nodes. Empty slice for a
    /// non-`Array` node.
    pub fn array_elements(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::Array(list) => self.list(list),
            _ => &[],
        }
    }

    /// `percent_literal?` — the array was written with a percent-literal
    /// opener such as `%w[`/`%i(`.
    pub fn is_percent_literal(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Array(_))
            && self.raw_source(self.range(id)).starts_with('%')
    }

    /// `square_brackets?` — the array was written with `[`/`]` delimiters.
    pub fn is_square_brackets(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Array(_))
            && self.raw_source(self.range(id)).starts_with('[')
    }

    /// `bracketed?` — the array has an explicit array delimiter.
    pub fn is_bracketed(&self, id: NodeId) -> bool {
        self.is_square_brackets(id) || self.is_percent_literal(id)
    }

    /// `CaseNode#condition` — the subject of a `case subj; when …`, or
    /// `OptNodeId::NONE` (a subject-less `case` or a non-`Case` node).
    pub fn case_subject(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Case { subject, .. } => subject,
            _ => OptNodeId::NONE,
        }
    }

    /// `CaseNode#when_branches` — the `When` child nodes. Empty slice for
    /// a non-`Case` node.
    pub fn case_when_branches(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::Case { whens, .. } => self.list(whens),
            _ => &[],
        }
    }

    /// `CaseNode#else_branch` — the `else` body, or `OptNodeId::NONE`.
    pub fn case_else_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Case { else_, .. } => else_,
            _ => OptNodeId::NONE,
        }
    }

    /// `WhenNode#conditions` — the match values of a `when a, b then …`.
    /// Empty slice for a non-`When` node.
    pub fn when_conditions(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::When { conds, .. } => self.list(conds),
            _ => &[],
        }
    }

    /// `WhenNode#body` — the branch body, or `OptNodeId::NONE`.
    pub fn when_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::When { body, .. } => body,
            _ => OptNodeId::NONE,
        }
    }

    /// `DefNode#receiver` — the singleton receiver of `def self.foo` /
    /// `def obj.foo` (a `Defs`), or `OptNodeId::NONE` for a plain `def`
    /// or non-def node.
    pub fn def_receiver(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Def { receiver, .. } => receiver,
            NodeKind::Defs { receiver, .. } => OptNodeId::some(receiver),
            _ => OptNodeId::NONE,
        }
    }

    /// `DefNode#arguments` — the method's `Args` node (always present for
    /// a def, possibly empty). `OptNodeId::NONE` for a non-def node.
    pub fn def_arguments(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => OptNodeId::some(args),
            _ => OptNodeId::NONE,
        }
    }

    /// `DefNode#body` — the method body, or `OptNodeId::NONE` for an
    /// empty body or a non-def node.
    pub fn def_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Def { body, .. } | NodeKind::Defs { body, .. } => body,
            _ => OptNodeId::NONE,
        }
    }

    /// `ForNode#variable` — the loop variable target of `for v in …`
    /// (an `Lvasgn`/`Mlhs` write target), or `OptNodeId::NONE` for a
    /// non-`For` node. Mirrors RuboCop's `ForNode#variable`.
    pub fn for_variable(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::For { var, .. } => OptNodeId::some(var),
            _ => OptNodeId::NONE,
        }
    }

    /// `ForNode#collection` — the enumerable iterated over (`for … in coll`).
    pub fn for_collection(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::For { iter, .. } => OptNodeId::some(iter),
            _ => OptNodeId::NONE,
        }
    }

    /// `ForNode#body` — the loop body, or `OptNodeId::NONE` for an empty
    /// body or non-`For` node.
    pub fn for_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::For { body, .. } => body,
            _ => OptNodeId::NONE,
        }
    }

    /// `CaseMatchNode#subject` — the matched value of `case subj; in …; end`.
    /// `OptNodeId::NONE` for a non-`CaseMatch` node. Unlike `case/when`, a
    /// `case/in` always has a subject, so this is `Some` for any real
    /// `CaseMatch` (the `Option` only encodes the non-`CaseMatch` miss).
    pub fn case_match_subject(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::CaseMatch { subject, .. } => OptNodeId::some(subject),
            _ => OptNodeId::NONE,
        }
    }

    /// `CaseMatchNode#in_pattern_branches` — the `InPattern` child nodes.
    /// Empty slice for a non-`CaseMatch` node.
    pub fn in_pattern_branches(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::CaseMatch { in_patterns, .. } => self.list(in_patterns),
            _ => &[],
        }
    }

    /// `CaseMatchNode#else_branch` — the `else` body, or `OptNodeId::NONE`
    /// (no `else`, or a non-`CaseMatch` node).
    pub fn case_match_else_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::CaseMatch { else_body, .. } => else_body,
            _ => OptNodeId::NONE,
        }
    }

    /// `InPatternNode#pattern` — the pattern matched by an `in` clause.
    /// `OptNodeId::NONE` for a non-`InPattern` node.
    pub fn in_pattern_pattern(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::InPattern { pattern, .. } => OptNodeId::some(pattern),
            _ => OptNodeId::NONE,
        }
    }

    /// `InPatternNode#guard` — the `if`/`unless` guard expression of an `in`
    /// clause, or `OptNodeId::NONE` (no guard, or a non-`InPattern` node).
    /// The if/unless distinction is not preserved (see `NodeKind::InPattern`).
    pub fn in_pattern_guard(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::InPattern { guard, .. } => guard,
            _ => OptNodeId::NONE,
        }
    }

    /// `InPatternNode#body` — the branch body, or `OptNodeId::NONE` for an
    /// empty body or a non-`InPattern` node.
    pub fn in_pattern_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::InPattern { body, .. } => body,
            _ => OptNodeId::NONE,
        }
    }

    /// The file's comments, in source order.
    pub fn comments(&self) -> &'a [Comment] {
        unsafe { slice(self.raw.comments, self.raw.comments_len) }
    }

    /// Comments fully contained in `range`, in source order.
    pub fn comments_in_range(&self, range: Range) -> Vec<Comment> {
        self.comments()
            .iter()
            .copied()
            .filter(|comment| comment.range.start >= range.start && comment.range.end <= range.end)
            .collect()
    }

    /// Comments associated with `id`, using Murphy's current CommentsHelp
    /// association: immediately preceding own-line comments plus comments
    /// inside the node's source range.
    pub fn comments_for_node(&self, id: NodeId) -> Vec<Comment> {
        self.comments_in_range(self.range_with_comments(id))
    }

    /// The file's structured magic comments, in source order.
    pub fn magic_comments(&self) -> Vec<MagicComment> {
        let mut comments = Vec::new();
        let leading_comment_region_end = self.leading_comment_region_end();
        if self.source().as_bytes().starts_with(b"#!") {
            comments.push(MagicComment {
                range: self.source_line_range_without_newline(0),
                key_range: Range::ZERO,
                value_range: Range::ZERO,
                kind: MagicCommentKind::Shebang,
                value_bool: 0,
            });
        }
        comments.extend(
            self.comments()
                .iter()
                .copied()
                .filter(|comment| {
                    comment.range.start as usize <= leading_comment_region_end
                        && self.is_own_line_comment(*comment)
                })
                .filter_map(|comment| self.parse_magic_comment(comment)),
        );
        comments.sort_by_key(|comment| comment.range.start);
        comments
    }

    /// The file's shebang line, if present.
    pub fn shebang(&self) -> Option<MagicComment> {
        self.find_magic_comment(MagicCommentKind::Shebang)
    }

    /// The file's `frozen_string_literal` magic comment, if present.
    pub fn frozen_string_literal_comment(&self) -> Option<MagicComment> {
        self.find_magic_comment(MagicCommentKind::FrozenStringLiteral)
    }

    /// The file's `encoding` magic comment, if present.
    pub fn encoding_comment(&self) -> Option<MagicComment> {
        self.find_magic_comment(MagicCommentKind::Encoding)
    }

    /// Parsed `murphy:`/`rubocop:` disable, enable, and todo directives from
    /// the file's comment table.
    pub fn comment_directives(&self) -> Vec<CommentDirective<'a>> {
        comment_directives_from_comments(self.source(), self.comments())
    }

    /// The file's source tokens, in source order.
    pub fn sorted_tokens(&self) -> &'a [SourceToken] {
        unsafe { slice(self.raw.sorted_tokens, self.raw.sorted_tokens_len) }
    }

    /// The exact source bytes of `tok`. Use this to match a token by text
    /// when no dedicated [`SourceTokenKind`] exists for it — e.g. `=`,
    /// `end`, or `::` (which all land in [`SourceTokenKind::Other`]).
    pub fn token_text(&self, tok: SourceToken) -> &'a str {
        self.raw_source(tok.range)
    }

    /// The source tokens fully contained within `range`, in source order.
    ///
    /// "Fully contained" means `tok.range.start >= range.start` **and**
    /// `tok.range.end <= range.end`; a token straddling a boundary is
    /// excluded. The returned slice is a contiguous sub-slice of
    /// [`sorted_tokens`](Self::sorted_tokens) (tokens are sorted by start
    /// offset; ends are monotonic, with at most equal-end overlaps), located
    /// by two binary searches plus a short trailing-straddler trim.
    pub fn tokens_in(&self, range: Range) -> &'a [SourceToken] {
        let toks = self.sorted_tokens();
        // First token whose start is at or after range.start.
        let lo = toks.partition_point(|t| t.range.start < range.start);
        // First token (from lo) whose start is at or after range.end —
        // everything from lo up to (but not including) hi starts inside
        // [range.start, range.end). Search only the [lo..] suffix.
        let hi = lo + toks[lo..].partition_point(|t| t.range.start < range.end);
        // Trim *every* trailing token whose end spills past range.end: ends
        // are monotonic, so straddlers cluster at the tail, but equal-end
        // overlaps (e.g. a heredoc-end token sharing its end with the
        // standalone newline) mean there can be more than one — a single
        // `if` would leave the rest in, breaking the "fully contained"
        // contract.
        let mut end = hi;
        while end > lo && toks[end - 1].range.end > range.end {
            end -= 1;
        }
        &toks[lo..end]
    }

    /// The last source token ending at or before `offset`, or `None` if no
    /// token lies entirely before it. `tok.range.end <= offset`.
    pub fn token_before(&self, offset: u32) -> Option<SourceToken> {
        let toks = self.sorted_tokens();
        // Tokens are sorted by start and non-overlapping, so `end` is also
        // monotonic; the last token with `end <= offset` is the one just
        // before the partition point on `end`.
        let idx = toks.partition_point(|t| t.range.end <= offset);
        if idx == 0 { None } else { Some(toks[idx - 1]) }
    }

    /// The first source token starting at or after `offset`, or `None` if no
    /// token lies at or after it. `tok.range.start >= offset`.
    pub fn token_after(&self, offset: u32) -> Option<SourceToken> {
        let toks = self.sorted_tokens();
        let idx = toks.partition_point(|t| t.range.start < offset);
        toks.get(idx).copied()
    }

    /// Expand `range` to cover the whole source lines it touches.
    ///
    /// Mirrors RuboCop's `RangeHelp#range_by_whole_lines`: the start moves
    /// to the first line's column 0, the end moves to the last line's end,
    /// and `include_final_newline` includes that line's terminating `\n` if
    /// it exists. Results are clamped to the file source range.
    pub fn range_by_whole_lines(&self, range: Range, include_final_newline: bool) -> Range {
        let source = self.source();
        let bytes = source.as_bytes();
        let range = clamp_range(range, bytes.len());
        let start = bytes[..range.start as usize]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let end_anchor = if range.end > range.start {
            range.end as usize - 1
        } else {
            range.end as usize
        };
        let mut end = bytes[end_anchor..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |pos| end_anchor + pos);
        if end < bytes.len() && bytes[end] == b'\n' && include_final_newline {
            end += 1;
        }
        Range {
            start: start as u32,
            end: end as u32,
        }
    }

    /// Expand `range` through surrounding whitespace in source bytes.
    ///
    /// The scan order intentionally follows RuboCop: spaces/tabs, optional
    /// backslash-newline continuations, optional newlines, then optional
    /// general ASCII whitespace.
    pub fn range_with_surrounding_space(&self, range: Range, options: SpaceRangeOptions) -> Range {
        let bytes = self.source().as_bytes();
        let range = clamp_range(range, bytes.len());
        let (go_left, go_right) = range_directions(options.side);
        let mut start = range.start as usize;
        let mut end = range.end as usize;
        if go_left {
            start = final_space_pos(bytes, start, -1, options);
        }
        if go_right {
            end = final_space_pos(bytes, end, 1, options);
        }
        Range {
            start: start as u32,
            end: end as u32,
        }
    }

    /// Union a node's source range with immediately preceding own-line
    /// comments.
    ///
    /// RuboCop uses `processed_source.ast_with_comments[node]`. Murphy does
    /// not yet have parser-compatible comment association, so this helper
    /// takes the conservative subset layout cops commonly need: contiguous
    /// `#` comment lines directly above the node.
    pub fn range_with_comments(&self, id: NodeId) -> Range {
        let source = self.source();
        let mut result = self.range(id);
        for &comment in self.comments().iter().rev() {
            if comment.range.end > result.start {
                continue;
            }
            if !own_line_comment(source, comment) {
                continue;
            }
            let comment_line = line_range(source, comment.range.start as usize);
            let result_line_start = line_range(source, result.start as usize).start;
            let comment_line_end = comment_line.end;
            if comment_line_end != result_line_start {
                break;
            }
            result.start = comment_line.start;
        }
        result
    }

    /// Compose [`Self::range_with_comments`] and [`Self::range_by_whole_lines`]
    /// with RuboCop's `include_final_newline: true` convention.
    pub fn range_with_comments_and_lines(&self, id: NodeId) -> Range {
        self.range_by_whole_lines(self.range_with_comments(id), true)
    }

    /// Decode the current cop's runtime options.
    pub fn options<T: CopOptions>(&self) -> Result<T, ConfigError> {
        let bytes = unsafe { self.raw.options_json.as_bytes() };
        T::from_config_json(bytes)
    }

    /// Decode the current cop's runtime options, falling back to defaults.
    pub fn options_or_default<T: CopOptions>(&self) -> T {
        self.options::<T>().unwrap_or_default()
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(&src[range.start as usize..range.end as usize])
            .expect("source is valid UTF-8")
    }

    /// Source range of the `.` or `&.` operator for an explicit-dot call
    /// — the parser-gem `node.loc.dot` analog from Prism's parser-provided
    /// call operator side table.
    ///
    /// Returns `None` for:
    /// - non-call kinds (anything but `Send` / `Csend`),
    /// - implicit `Send` (no receiver, e.g. a bare `foo` resolved as
    ///   `Kernel#foo`),
    /// - operator and bracket methods (`a + b`, `a[b]`),
    /// - implicit-call `foo.()` where Prism provides no call operator.
    pub fn call_operator_loc(&self, id: NodeId) -> Option<Range> {
        let r = self.call_operator_range(id);
        if r == Range::ZERO { None } else { Some(r) }
    }

    /// The whole file's source text. A `NodeCop` with `KINDS = &[]`
    /// (file-visit, see `NodeCop` doc) uses this to scan the entire
    /// file — `cx.range(cx.root())` only spans the AST root node,
    /// which can be narrower than the file (leading comments, trailing
    /// whitespace, etc. live outside the root's byte range).
    pub fn source(&self) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(src).expect("source is valid UTF-8")
    }

    /// Record an offense. `cop_name` is stamped from the `CxRaw` the host
    /// built for the running cop.
    pub fn emit_offense(&self, range: Range, message: &str, severity: Option<crate::Severity>) {
        let offense = crate::RawOffense {
            cop_name: self.raw.cop_name,
            message: crate::RawSlice {
                ptr: message.as_ptr(),
                len: message.len(),
            },
            range,
            severity: crate::Severity::to_wire(severity),
        };
        // Safety: `fns` is non-null per `from_raw`'s contract; `sink` is
        // an opaque host handle interpreted only by the callback. The
        // message slice outlives this synchronous call.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_offense)(self.raw.sink, &offense) };
    }

    /// Record an autocorrect edit. Offense↔edit correlation is the host's
    /// (murphy-9cr.22) concern.
    pub fn emit_edit(&self, range: Range, replacement: &str) {
        let edit = crate::RawEdit {
            range,
            replacement: crate::RawSlice {
                ptr: replacement.as_ptr(),
                len: replacement.len(),
            },
        };
        // Safety: see `emit_offense`.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_edit)(self.raw.sink, &edit) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CopOptions;
    use crate::abi::{CxRaw, FnTable, RawEdit, RawOffense, RawSlice};
    use murphy_ast::{Ast, AstBuilder, MagicCommentKind, NodeKind, OptNodeId, Range};

    /// Build `return nil` and return the owned `Ast` (kept alive by the
    /// caller) plus the root id.
    fn fixture() -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new("return nil", "t.rb".to_string());
        let nil = b.push(NodeKind::Nil, Range { start: 7, end: 10 });
        let root = b.push(
            NodeKind::Return(OptNodeId::some(nil)),
            Range { start: 0, end: 10 },
        );
        (b.finish(root), root)
    }

    // A FnTable is required to construct CxRaw; reads never call it.
    unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const RawOffense) {}
    unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

    /// Build a `CxRaw` pointing into `ast`'s backing storage. The returned
    /// `CxRaw` borrows both `ast` and `fns` for `'a` (raw-pointer fields,
    /// not lifetime-tracked — the caller keeps both alive).
    fn cx_raw_for<'a>(ast: &'a Ast, fns: &'a FnTable) -> CxRaw {
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
            cop_name: RawSlice::EMPTY,
            fns: fns as *const FnTable,
            sink: std::ptr::null_mut(),
            sorted_tokens: p.sorted_tokens.as_ptr(),
            sorted_tokens_len: p.sorted_tokens.len(),
            options_json: RawSlice::from_str("{}"),
            call_closing_locs: p.call_closing_locs.as_ptr(),
            call_closing_locs_len: p.call_closing_locs.len(),
            call_operator_locs: p.call_operator_locs.as_ptr(),
            call_operator_locs_len: p.call_operator_locs.len(),
            var_model: std::ptr::null(),
            node_slice_arena: std::ptr::null_mut(),
            alloc_node_slice: unavailable_alloc_node_slice,
        }
    }

    #[test]
    fn magic_comment_helpers_expose_structured_file_metadata() {
        let src = "#!/usr/bin/env ruby\n# frozen_string_literal: true\n# encoding: utf-8\nnil\n";
        let ast = murphy_translate::translate(src, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.magic_comments().len(), 3);
        assert_eq!(
            cx.shebang().map(|c| c.kind),
            Some(MagicCommentKind::Shebang)
        );
        let frozen = cx
            .frozen_string_literal_comment()
            .expect("frozen_string_literal comment");
        assert_eq!(cx.raw_source(frozen.value_range), "true");
        assert_eq!(frozen.value_bool, 1);
        let encoding = cx.encoding_comment().expect("encoding comment");
        assert_eq!(cx.raw_source(encoding.value_range), "utf-8");
    }

    #[test]
    fn magic_comment_helpers_parse_emacs_style_comments() {
        let src = "# -*- frozen_string_literal: true -*-\nnil\n";
        let ast = murphy_translate::translate(src, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let frozen = cx
            .frozen_string_literal_comment()
            .expect("frozen_string_literal comment");
        assert_eq!(cx.raw_source(frozen.key_range), "frozen_string_literal");
        assert_eq!(cx.raw_source(frozen.value_range), "true");
        assert_eq!(frozen.value_bool, 1);
    }

    #[test]
    fn magic_comment_helpers_treat_coding_as_encoding() {
        let src = "# coding: utf-8\nnil\n";
        let ast = murphy_translate::translate(src, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let encoding = cx.encoding_comment().expect("encoding comment");
        assert_eq!(cx.raw_source(encoding.key_range), "coding");
        assert_eq!(cx.raw_source(encoding.value_range), "utf-8");
    }

    #[test]
    fn magic_comment_helpers_ignore_comments_after_code() {
        let src = "puts 1 # frozen_string_literal: true\n# encoding: utf-8\n";
        let ast = murphy_translate::translate(src, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert!(cx.magic_comments().is_empty());
        assert!(cx.frozen_string_literal_comment().is_none());
        assert!(cx.encoding_comment().is_none());
    }

    #[derive(Default)]
    struct TestOptions {
        style: String,
    }

    impl CopOptions for TestOptions {
        fn from_config_json(bytes: &[u8]) -> Result<Self, crate::ConfigError> {
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(crate::ConfigError::parse)?;
            let style = value
                .get("style")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("default")
                .to_string();
            Ok(Self { style })
        }
    }

    #[test]
    fn accessors_match_the_underlying_ast() {
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.root(), root);
        assert_eq!(*cx.node(root), *ast.node(root));
        assert_eq!(*cx.kind(root), *ast.kind(root));
        assert_eq!(cx.range(root), ast.range(root));
        assert_eq!(cx.parent(root), ast.parent(root));
        let children = cx.children(root);
        assert_eq!(children, ast.children(root).collect::<Vec<_>>());
        // `root` has no ancestors; walk from the `nil` child so the
        // parent-walking loop is actually exercised.
        let nil = children[0];
        assert_eq!(
            cx.ancestors(nil).collect::<Vec<_>>(),
            ast.ancestors(nil).collect::<Vec<_>>()
        );
        assert_eq!(cx.ancestors(nil).collect::<Vec<_>>(), vec![root]);
        assert_eq!(
            cx.ancestors(root).collect::<Vec<_>>(),
            ast.ancestors(root).collect::<Vec<_>>()
        );
        let desc: Vec<_> = cx.descendants(root);
        assert_eq!(desc, ast.descendants(root).collect::<Vec<_>>());
        assert_eq!(cx.comments(), ast.comments());
        assert_eq!(
            cx.raw_source(cx.range(root)),
            ast.raw_source(ast.range(root))
        );
    }

    #[test]
    fn ancestors_of_type_filters_by_pattern_name_and_alias_group() {
        let mut b = AstBuilder::new("def m; foo { 1 }; end", "t.rb".to_string());
        let method = b.intern_symbol("m");
        let foo = b.intern_symbol("foo");
        let one = b.push(NodeKind::Int(1), Range { start: 14, end: 15 });
        let block_args = b.push(
            NodeKind::Args(murphy_ast::NodeList::EMPTY),
            Range { start: 10, end: 10 },
        );
        let def_args = b.push(
            NodeKind::Args(murphy_ast::NodeList::EMPTY),
            Range { start: 5, end: 5 },
        );
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: foo,
                args: murphy_ast::NodeList::EMPTY,
            },
            Range { start: 7, end: 10 },
        );
        let block = b.push(
            NodeKind::Block {
                call: send,
                args: block_args,
                body: OptNodeId::some(one),
            },
            Range { start: 7, end: 17 },
        );
        let root = b.push(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: method,
                args: def_args,
                body: OptNodeId::some(block),
            },
            Range { start: 0, end: 21 },
        );
        let ast = b.finish(root);

        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(
            cx.ancestors_of_type(one, "block").collect::<Vec<_>>(),
            vec![block]
        );
        assert_eq!(
            cx.ancestors_of_type(one, "any_block").collect::<Vec<_>>(),
            vec![block]
        );
        assert_eq!(
            cx.ancestors_of_type(one, "def").collect::<Vec<_>>(),
            vec![root]
        );
        assert_eq!(
            cx.ancestors_of_type(one, "call").collect::<Vec<_>>(),
            Vec::new()
        );
        assert_eq!(
            cx.ancestors_of_type(one, "sned").collect::<Vec<_>>(),
            Vec::new()
        );
    }

    #[test]
    fn comment_directives_expose_same_line_and_block_ranges() {
        let source = concat!(
            "puts 'x' # murphy:disable Murphy/NoReceiverPuts\n",
            "# murphy:disable Layout/LineLength, Style/StringLiterals\n",
            "puts \"y\"\n",
            "# murphy:enable Layout/LineLength\n",
            "puts \"z\"\n",
            "# murphy:enable Style/StringLiterals\n",
        );
        let ast = murphy_translate::translate(source, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let directives = cx.comment_directives();

        assert_eq!(directives.len(), 5);
        assert_eq!(directives[0].kind, CommentDirectiveKind::Disable);
        assert_eq!(directives[0].scope, CommentDirectiveScope::SameLine);
        assert_eq!(directives[0].cop, Some("Murphy/NoReceiverPuts"));
        assert_eq!(
            cx.raw_source(directives[0].affected_range),
            "puts 'x' # murphy:disable Murphy/NoReceiverPuts\n"
        );

        assert_eq!(directives[1].kind, CommentDirectiveKind::Disable);
        assert_eq!(directives[1].scope, CommentDirectiveScope::Block);
        assert_eq!(directives[1].cop, Some("Layout/LineLength"));
        assert_eq!(cx.raw_source(directives[1].affected_range), "puts \"y\"\n");

        assert_eq!(directives[2].kind, CommentDirectiveKind::Disable);
        assert_eq!(directives[2].scope, CommentDirectiveScope::Block);
        assert_eq!(directives[2].cop, Some("Style/StringLiterals"));
        assert_eq!(
            cx.raw_source(directives[2].affected_range),
            "puts \"y\"\n# murphy:enable Layout/LineLength\nputs \"z\"\n"
        );

        assert_eq!(directives[3].kind, CommentDirectiveKind::Enable);
        assert_eq!(directives[3].scope, CommentDirectiveScope::Block);
        assert_eq!(directives[3].cop, Some("Layout/LineLength"));

        assert_eq!(directives[4].kind, CommentDirectiveKind::Enable);
        assert_eq!(directives[4].scope, CommentDirectiveScope::Block);
        assert_eq!(directives[4].cop, Some("Style/StringLiterals"));
    }

    #[test]
    fn comment_directives_classify_file_top_disable_all() {
        let source = "# murphy:disable\nputs 'x'\n";
        let ast = murphy_translate::translate(source, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let directives = cx.comment_directives();

        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].kind, CommentDirectiveKind::Disable);
        assert_eq!(directives[0].scope, CommentDirectiveScope::File);
        assert_eq!(directives[0].cop, None);
        assert_eq!(cx.raw_source(directives[0].affected_range), source);
    }

    fn cx_for_source(source: &str) -> (Ast, FnTable) {
        let ast = murphy_translate::translate(source, "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        (ast, fns)
    }

    #[test]
    fn range_help_range_by_whole_lines_expands_to_line_bounds() {
        let (ast, fns) = cx_for_source("alpha\n  beta\ngamma");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_by_whole_lines(Range { start: 8, end: 10 }, false);

        assert_eq!(range, Range { start: 6, end: 12 });
        assert_eq!(cx.raw_source(range), "  beta");
    }

    #[test]
    fn range_help_range_by_whole_lines_can_include_final_newline() {
        let (ast, fns) = cx_for_source("alpha\n  beta\ngamma");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_by_whole_lines(Range { start: 8, end: 10 }, true);

        assert_eq!(range, Range { start: 6, end: 13 });
        assert_eq!(cx.raw_source(range), "  beta\n");
    }

    #[test]
    fn range_help_range_by_whole_lines_respects_half_open_line_boundary_end() {
        let (ast, fns) = cx_for_source("alpha\nbeta\n");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_by_whole_lines(Range { start: 0, end: 6 }, false);

        assert_eq!(range, Range { start: 0, end: 5 });
        assert_eq!(cx.raw_source(range), "alpha");
    }

    #[test]
    fn range_help_range_with_surrounding_space_matches_rubocop_defaults() {
        let (ast, fns) = cx_for_source("foo  +\n  bar");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx
            .range_with_surrounding_space(Range { start: 5, end: 6 }, SpaceRangeOptions::default());

        assert_eq!(range, Range { start: 3, end: 7 });
        assert_eq!(cx.raw_source(range), "  +\n");
    }

    #[test]
    fn range_help_range_with_surrounding_space_honors_side_and_no_newlines() {
        let (ast, fns) = cx_for_source("foo  +\n  bar");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_with_surrounding_space(
            Range { start: 5, end: 6 },
            SpaceRangeOptions {
                side: RangeSide::Left,
                newlines: false,
                continuations: false,
                whitespace: false,
            },
        );

        assert_eq!(range, Range { start: 3, end: 6 });
        assert_eq!(cx.raw_source(range), "  +");
    }

    #[test]
    fn range_help_range_with_comments_includes_adjacent_own_line_comments() {
        let source = concat!(
            "# doc one\n",
            "# doc two\n",
            "def m\n",
            "  puts 1\n",
            "end\n"
        );
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_with_comments(cx.root());

        assert_eq!(
            cx.raw_source(range),
            "# doc one\n# doc two\ndef m\n  puts 1\nend"
        );
    }

    #[test]
    fn range_help_range_with_comments_includes_comments_for_non_root_node() {
        let source = concat!(
            "class C\n",
            "  # doc one\n",
            "  # doc two\n",
            "  def m\n",
            "    puts 1\n",
            "  end\n",
            "end\n"
        );
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let def = cx
            .descendants(cx.root())
            .into_iter()
            .find(|&id| matches!(cx.kind(id), NodeKind::Def { .. }))
            .expect("def node");

        let range = cx.range_with_comments(def);

        assert_eq!(
            cx.raw_source(range),
            "  # doc one\n  # doc two\n  def m\n    puts 1\n  end"
        );
    }

    #[test]
    fn range_help_range_with_comments_and_lines_includes_final_newline() {
        let source = concat!(
            "# doc one\n",
            "# doc two\n",
            "def m\n",
            "  puts 1\n",
            "end\n"
        );
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_with_comments_and_lines(cx.root());

        assert_eq!(cx.raw_source(range), source);
    }

    #[test]
    fn range_help_range_with_comments_ignores_inline_comments() {
        let source = "foo # inline\nbar\n";
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_with_comments(cx.root());

        assert_eq!(cx.raw_source(range), "foo # inline\nbar");
    }

    #[test]
    fn range_help_range_with_comments_stops_at_blank_line() {
        let source = concat!("# doc\n", "\n", "def m\n", "end\n");
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let range = cx.range_with_comments(cx.root());

        assert_eq!(cx.raw_source(range), "def m\nend");
    }

    #[test]
    fn comments_help_comments_in_range_returns_contained_comments() {
        let source = concat!(
            "# outside\n",
            "case value\n",
            "when 1\n",
            "  # body\n",
            "when 2\n",
            "  :ok\n",
            "end\n"
        );
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let when = cx
            .descendants(cx.root())
            .into_iter()
            .find(|&id| matches!(cx.kind(id), NodeKind::When { .. }))
            .expect("when node");
        let region = Range {
            start: cx.range(when).end,
            end: cx.range(cx.root()).end,
        };

        let comments = cx.comments_in_range(region);

        assert_eq!(comments.len(), 1);
        assert_eq!(cx.raw_source(comments[0].range), "# body");
    }

    #[test]
    fn comments_help_comments_for_node_includes_leading_and_inner_comments() {
        let source = concat!(
            "class C\n",
            "  # doc\n",
            "  def m\n",
            "    # body\n",
            "    puts 1\n",
            "  end\n",
            "end\n"
        );
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let def = cx
            .descendants(cx.root())
            .into_iter()
            .find(|&id| matches!(cx.kind(id), NodeKind::Def { .. }))
            .expect("def node");

        let comments = cx.comments_for_node(def);

        assert_eq!(comments.len(), 2);
        assert_eq!(cx.raw_source(comments[0].range), "# doc");
        assert_eq!(cx.raw_source(comments[1].range), "# body");
    }

    #[test]
    fn range_help_range_by_whole_lines_handles_heredoc_body_and_end_lines() {
        let source = "value = <<~TEXT\n  body\nTEXT\n";
        let (ast, fns) = cx_for_source(source);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let heredoc_end = cx
            .sorted_tokens()
            .iter()
            .find(|tok| tok.kind == murphy_ast::SourceTokenKind::HeredocEnd)
            .expect("heredoc end token");

        let range = cx.range_by_whole_lines(heredoc_end.range, true);

        assert_eq!(cx.raw_source(range), "TEXT\n");
    }

    #[test]
    fn list_resolves_node_list_to_a_borrowed_slice() {
        use murphy_ast::{AstBuilder, NodeKind, NodeList, OptNodeId, Range};

        // `foo(1, 2)` — a Send whose `args` NodeList holds two Int nodes.
        let mut b = AstBuilder::new("foo(1, 2)", "t.rb".to_string());
        let one = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        let two = b.push(NodeKind::Int(2), Range { start: 7, end: 8 });
        let args = b.push_list(&[one, two]);
        let method = b.intern_symbol("foo");
        let root = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method,
                args,
            },
            Range { start: 0, end: 9 },
        );
        let ast = b.finish(root);

        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        // Pull the `args` NodeList back out of the Send and resolve it.
        let NodeKind::Send { args, .. } = *cx.kind(root) else {
            panic!("expected Send");
        };
        assert_eq!(cx.list(args), &[one, two]);
        // An empty NodeList resolves to an empty slice.
        assert_eq!(cx.list(NodeList::EMPTY), &[] as &[murphy_ast::NodeId]);
    }

    #[test]
    fn sorted_tokens_match_the_underlying_ast() {
        let mut b = AstBuilder::new("foo(1)", "t.rb".to_string());
        let root = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        b.add_source_token(murphy_ast::SourceToken {
            kind: murphy_ast::SourceTokenKind::LeftParen,
            range: Range { start: 3, end: 4 },
        });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.sorted_tokens(), ast.sorted_tokens());
    }

    /// Build a synthetic Send/Csend with the receiver/name ranges a real
    /// parser would emit for `source[recv]` (the receiver text) chained
    /// onto `source[name]` (the selector text). Returns the call's
    /// `NodeId` plus the owned `Ast`.
    fn build_call(
        source: &str,
        recv: Option<Range>,
        name: Range,
        call_operator: Option<Range>,
        is_csend: bool,
    ) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new(source.to_string(), "t.rb".to_string());
        let recv_id = recv.map(|r| {
            let recv_method = b.intern_symbol("recv");
            b.push_named(
                NodeKind::Send {
                    receiver: OptNodeId::NONE,
                    method: recv_method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                r,
                r,
            )
        });
        let method = b.intern_symbol(&source[name.start as usize..name.end as usize]);
        let expression = Range {
            start: recv.map(|r| r.start).unwrap_or(name.start),
            end: name.end,
        };
        let root = if is_csend {
            let recv_id = recv_id.expect("Csend requires a receiver");
            b.push_named(
                NodeKind::Csend {
                    receiver: recv_id,
                    method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                expression,
                name,
            )
        } else {
            b.push_named(
                NodeKind::Send {
                    receiver: recv_id.map(OptNodeId::some).unwrap_or(OptNodeId::NONE),
                    method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                expression,
                name,
            )
        };
        if let Some(operator) = call_operator {
            b.add_call_operator_loc(root, operator);
        }
        (b.finish(root), root)
    }

    #[test]
    fn loc_ref_fields_match_nodeloc() {
        let (ast, root) = build_call(
            "foo.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 7 },
            Some(Range { start: 3, end: 4 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let loc = cx.loc(root);
        assert_eq!(loc.expression, cx.range(root));
        assert_eq!(loc.name, cx.node(root).loc.name);
    }

    #[test]
    fn loc_dot_finds_explicit_dot() {
        let (ast, root) = build_call(
            "foo.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 7 },
            Some(Range { start: 3, end: 4 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.loc(root).dot(), Range { start: 3, end: 4 });
    }

    #[test]
    fn loc_dot_finds_safe_navigation() {
        let (ast, root) = build_call(
            "foo&.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 5, end: 8 },
            Some(Range { start: 3, end: 5 }),
            true,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.loc(root).dot(), Range { start: 3, end: 5 });
    }

    #[test]
    fn loc_dot_zero_for_no_receiver() {
        // bare `puts 'x'` — Send with no receiver
        let (ast, root) = build_call("puts", None, Range { start: 0, end: 4 }, None, false);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.loc(root).dot(), Range::ZERO);
    }

    #[test]
    fn call_operator_loc_finds_explicit_dot() {
        // `foo.bar`
        let (ast, root) = build_call(
            "foo.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 7 },
            Some(Range { start: 3, end: 4 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 3, end: 4 }));
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), ".");
    }

    #[test]
    fn call_operator_loc_finds_safe_navigation() {
        // `foo&.bar`
        let (ast, root) = build_call(
            "foo&.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 5, end: 8 },
            Some(Range { start: 3, end: 5 }),
            true,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 3, end: 5 }));
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), "&.");
    }

    #[test]
    fn call_operator_loc_handles_multiline_chain() {
        // `foo\n  .bar` — receiver ends at offset 3, name starts at 7.
        let (ast, root) = build_call(
            "foo\n  .bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 7, end: 10 },
            Some(Range { start: 6, end: 7 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 6, end: 7 }));
    }

    #[test]
    fn call_operator_loc_uses_parser_provided_operator_range() {
        // Synthetic malformed spacing: byte scanning would find the first dot,
        // but the plugin API must expose Prism's parser-provided operator loc.
        let src = "foo..bar";
        let (ast, root) = build_call(
            src,
            Some(Range { start: 0, end: 3 }),
            Range { start: 5, end: 8 },
            Some(Range { start: 4, end: 5 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 4, end: 5 }));
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), ".");
    }

    #[test]
    fn call_operator_loc_returns_none_for_implicit_send() {
        // bare `foo` — Send with receiver = None
        let (ast, root) = build_call("foo", None, Range { start: 0, end: 3 }, None, false);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_operator_method() {
        // `foo + bar` — Send with method `:+`; Prism provides no call operator.
        let (ast, root) = build_call(
            "foo + bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 5 },
            None,
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_bracket_method() {
        // `a[b]` — Send with method `:[]`; Prism provides no call operator.
        let (ast, root) = build_call(
            "a[b]",
            Some(Range { start: 0, end: 1 }),
            Range { start: 1, end: 3 },
            None,
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_non_call_kinds() {
        // A bare `nil` literal — not a call kind.
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        // `root` is Return, child is Nil — both non-call.
        assert_eq!(cx.call_operator_loc(root), None);
        let nil = cx.children(root)[0];
        assert_eq!(cx.call_operator_loc(nil), None);
    }

    #[test]
    fn options_or_default_decodes_current_cop_options() {
        let (ast, _) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let mut raw = cx_raw_for(&ast, &fns);
        raw.options_json = RawSlice::from_str(r#"{"style":"configured"}"#);
        let cx = unsafe { Cx::from_raw(&raw) };

        let options = cx.options_or_default::<TestOptions>();
        assert_eq!(options.style, "configured");
    }

    #[test]
    fn options_or_default_falls_back_on_decode_error() {
        let (ast, _) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let mut raw = cx_raw_for(&ast, &fns);
        raw.options_json = RawSlice::from_str("not json");
        let cx = unsafe { Cx::from_raw(&raw) };

        let options = cx.options_or_default::<TestOptions>();
        assert_eq!(options.style, "");
    }

    use std::cell::RefCell;

    struct Sink {
        offenses: Vec<(String, String, Range, u8)>,
        edits: Vec<(Range, String)>,
    }

    unsafe extern "C" fn record_offense(sink: *mut std::ffi::c_void, o: *const RawOffense) {
        let sink = unsafe { &*(sink as *const RefCell<Sink>) };
        let o = unsafe { &*o };
        sink.borrow_mut().offenses.push((
            String::from_utf8(unsafe { o.cop_name.as_bytes() }.to_vec()).unwrap(),
            String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).unwrap(),
            o.range,
            o.severity,
        ));
    }

    unsafe extern "C" fn record_edit(sink: *mut std::ffi::c_void, e: *const RawEdit) {
        let sink = unsafe { &*(sink as *const RefCell<Sink>) };
        let e = unsafe { &*e };
        sink.borrow_mut().edits.push((
            e.range,
            String::from_utf8(unsafe { e.replacement.as_bytes() }.to_vec()).unwrap(),
        ));
    }

    #[test]
    fn emit_forwards_offense_and_edit_to_the_fn_table() {
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: record_offense,
            emit_edit: record_edit,
        };
        let sink = RefCell::new(Sink {
            offenses: Vec::new(),
            edits: Vec::new(),
        });

        let mut raw = cx_raw_for(&ast, &fns);
        raw.cop_name = RawSlice::from_str("Plugin/Demo");
        raw.sink = &sink as *const _ as *mut std::ffi::c_void;
        let cx = unsafe { Cx::from_raw(&raw) };

        cx.emit_offense(cx.range(root), "bad return", Some(crate::Severity::Error));
        cx.emit_edit(Range { start: 7, end: 10 }, "false");

        let s = sink.borrow();
        assert_eq!(s.offenses.len(), 1);
        assert_eq!(s.offenses[0].0, "Plugin/Demo");
        assert_eq!(s.offenses[0].1, "bad return");
        assert_eq!(
            s.offenses[0].3,
            crate::Severity::to_wire(Some(crate::Severity::Error))
        );
        assert_eq!(s.offenses[0].2, cx.range(root));
        assert_eq!(
            s.edits,
            vec![(Range { start: 7, end: 10 }, "false".to_string())]
        );
    }

    #[test]
    fn symbol_and_string_resolve_through_the_interner() {
        let mut b = AstBuilder::new("x = \"hi\"", "t.rb".to_string());
        let sym = b.intern_symbol("x");
        let str_id = b.intern_string("hi");
        let root = b.push(NodeKind::Nil, Range { start: 0, end: 0 });
        let ast = b.finish(root);

        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.symbol_str(sym), "x");
        assert_eq!(cx.string_str(str_id), "hi");
    }

    /// Build a bare `def <name>; end` and return its `Def` node id + Ast.
    fn build_def(source: &str, name: &str, name_range: Range) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new(source.to_string(), "t.rb".to_string());
        let args = b.push(NodeKind::Args(murphy_ast::NodeList::EMPTY), name_range);
        let sym = b.intern_symbol(name);
        let root = b.push_named(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: sym,
                args,
                body: OptNodeId::NONE,
            },
            Range {
                start: 0,
                end: source.len() as u32,
            },
            name_range,
        );
        (b.finish(root), root)
    }

    #[test]
    fn method_name_resolves_send_csend_and_def_selectors() {
        // Send: `a == b` — selector `==` at [2, 4).
        let (ast, send) = build_call(
            "a == b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 4 },
            None,
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(send), Some("=="));

        // Csend: `a&.foo` — selector `foo` at [3, 6).
        let (ast, csend) = build_call(
            "a&.foo",
            Some(Range { start: 0, end: 1 }),
            Range { start: 3, end: 6 },
            Some(Range { start: 1, end: 3 }),
            true,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(csend), Some("foo"));

        // Def: `def foo=(v); end` — selector `foo=`.
        let (ast, def) = build_def("def foo=(v); end", "foo=", Range { start: 4, end: 8 });
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(def), Some("foo="));
    }

    #[test]
    fn method_name_is_none_for_non_method_nodes() {
        // An Int literal has no selector.
        let mut b = AstBuilder::new("42", "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(root), None);
    }

    #[test]
    fn cx_predicate_wrappers_classify_the_node_selector() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `a == b` → comparison + operator, not assignment/predicate/bang/camel.
        let (ast, cmp) = build_call(
            "a == b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 4 },
            None,
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_comparison_method(cmp));
        assert!(cx.is_operator_method(cmp));
        assert!(!cx.is_assignment_method(cmp));
        assert!(!cx.is_predicate_method(cmp));

        // `def foo=(v); end` → assignment, not comparison.
        let (ast, setter) = build_def("def foo=(v); end", "foo=", Range { start: 4, end: 8 });
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_assignment_method(setter));
        assert!(!cx.is_comparison_method(setter));

        // `a.foo?` → predicate.
        let (ast, pred) = build_call(
            "a.foo?",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 6 },
            Some(Range { start: 1, end: 2 }),
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_predicate_method(pred));
        assert!(!cx.is_bang_method(pred));

        // `Foo()` → camel-case method.
        let (ast, camel) = build_call("Foo()", None, Range { start: 0, end: 3 }, None, false);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_camel_case_method(camel));
    }

    #[test]
    fn cx_predicate_wrappers_are_false_for_non_method_nodes() {
        let mut b = AstBuilder::new("42", "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_comparison_method(root));
        assert!(!cx.is_operator_method(root));
        assert!(!cx.is_assignment_method(root));
        assert!(!cx.is_predicate_method(root));
        assert!(!cx.is_bang_method(root));
        assert!(!cx.is_camel_case_method(root));
    }

    #[test]
    fn cx_collection_and_enumerable_wrappers_classify_the_node_selector() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `a.map` → enumerable + enumerator (in set), not a nonmutating
        // collection-specific table.
        let (ast, map) = build_call(
            "a.map",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 5 },
            Some(Range { start: 1, end: 2 }),
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_enumerable_method(map));
        assert!(cx.is_enumerator_method(map));

        // `a.each_slice` → enumerator via the `each_` prefix rule.
        let (ast, es) = build_call(
            "a.each_slice",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 12 },
            Some(Range { start: 1, end: 2 }),
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_enumerator_method(es));

        // `a.merge` → nonmutating hash method.
        let (ast, merge) = build_call(
            "a.merge",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 7 },
            Some(Range { start: 1, end: 2 }),
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_nonmutating_hash_method(merge));

        // `a + b` → nonmutating binary operator (so also nonmutating operator).
        let (ast, plus) = build_call(
            "a + b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 3 },
            None,
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_nonmutating_binary_operator_method(plus));
        assert!(cx.is_nonmutating_operator_method(plus));
        assert!(!cx.is_nonmutating_unary_operator_method(plus));
    }

    /// Build `<recv-kind>.<sel>(args…)` where the receiver is a chosen
    /// `NodeKind` (self / const / a sub-send), returning the call + Ast.
    fn build_call_with(
        recv_kind: Option<NodeKind>,
        selector: &str,
        arg_ints: &[i64],
    ) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let z = Range { start: 0, end: 1 };
        let receiver = match recv_kind {
            Some(k) => OptNodeId::some(b.push(k, z)),
            None => OptNodeId::NONE,
        };
        let arg_ids: Vec<_> = arg_ints
            .iter()
            .map(|&n| b.push(NodeKind::Int(n), z))
            .collect();
        let args = b.push_list(&arg_ids);
        let method = b.intern_symbol(selector);
        let root = b.push(
            NodeKind::Send {
                receiver,
                method,
                args,
            },
            z,
        );
        (b.finish(root), root)
    }

    #[test]
    fn call_receiver_and_arguments_resolve_send_parts() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `foo(1, 2)` — receiverless, two args.
        let (ast, call) = build_call_with(None, "foo", &[1, 2]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.call_receiver(call).get().is_none());
        let args = cx.call_arguments(call);
        assert_eq!(args.len(), 2);
        assert!(cx.has_call_arguments(call));
        assert_eq!(cx.first_argument(call).get(), Some(args[0]));
        assert_eq!(cx.last_argument(call).get(), Some(args[1]));

        // `self.bar` — self receiver, no args.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "bar", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.call_receiver(call).get().is_some());
        assert!(!cx.has_call_arguments(call));
        assert!(cx.first_argument(call).get().is_none());
        assert!(cx.last_argument(call).get().is_none());
    }

    #[test]
    fn self_and_const_receiver_predicates() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `self.foo`
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_self_receiver(call));
        assert!(!cx.is_const_receiver(call));

        // `Foo.bar` — const receiver.
        let const_name = {
            let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
            b.intern_symbol("Foo")
        };
        let (ast, call) = build_call_with(
            Some(NodeKind::Const {
                scope: OptNodeId::NONE,
                name: const_name,
            }),
            "bar",
            &[],
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_const_receiver(call));
        assert!(!cx.is_self_receiver(call));

        // Receiverless send is neither.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_self_receiver(call));
        assert!(!cx.is_const_receiver(call));
    }

    #[test]
    fn command_and_negation_predicates() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `foo` — receiverless ⇒ command?("foo") true, command?("bar") false.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_command(call, "foo"));
        assert!(!cx.is_command(call, "bar"));
        assert!(!cx.is_negation_method(call));

        // `self.foo` — has a receiver ⇒ not a command.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_command(call, "foo"));

        // `x.!` — receiver + `!` selector ⇒ negation_method?.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "!", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_negation_method(call));
        // Bare `!` with no receiver is not a negation method.
        let (ast, call) = build_call_with(None, "!", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_negation_method(call));
    }

    #[test]
    fn literal_predicate_matches_literal_node_kinds() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // An Int literal.
        let mut b = AstBuilder::new("42".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_literal(root));

        // A `nil` literal.
        let mut b = AstBuilder::new("nil".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Nil, Range { start: 0, end: 3 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_literal(root));

        // A Send is not a literal.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_literal(call));
    }

    #[test]
    fn single_and_multiline_count_expression_lines() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `42` — one line.
        let mut b = AstBuilder::new("42".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_single_line(root));
        assert!(!cx.is_multiline(root));

        // `[\n1,\n2,\n]` — the Array expression spans four lines.
        let src = "[\n1,\n2,\n]";
        let mut b = AstBuilder::new(src.to_string(), "t.rb".to_string());
        let one = b.push(NodeKind::Int(1), Range { start: 2, end: 3 });
        let two = b.push(NodeKind::Int(2), Range { start: 5, end: 6 });
        let elems = b.push_list(&[one, two]);
        let root = b.push(
            NodeKind::Array(elems),
            Range {
                start: 0,
                end: src.len() as u32,
            },
        );
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_multiline(root));
        assert!(!cx.is_single_line(root));
    }

    #[test]
    fn if_node_accessors_project_branches() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let cond = b.push(NodeKind::True_, z);
        let then_ = b.push(NodeKind::Int(1), z);
        let else_ = b.push(NodeKind::Int(2), z);
        let iff = b.push(
            NodeKind::If {
                cond,
                then_: OptNodeId::some(then_),
                else_: OptNodeId::some(else_),
            },
            z,
        );
        let ast = b.finish(iff);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.if_condition(iff).get(), Some(cond));
        assert_eq!(cx.if_then_branch(iff).get(), Some(then_));
        assert_eq!(cx.if_else_branch(iff).get(), Some(else_));
        // Non-If node projects to NONE.
        assert!(cx.if_condition(then_).get().is_none());
    }

    #[test]
    fn hash_and_pair_accessors_project_children() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let key = b.intern_symbol("k");
        let key_node = b.push(NodeKind::Sym(key), z);
        let val_node = b.push(NodeKind::Int(7), z);
        let pair = b.push(
            NodeKind::Pair {
                key: key_node,
                value: val_node,
            },
            z,
        );
        // `{ **h, k => 7 }` — a kwsplat plus a pair. `pairs` must return
        // only the pair (faithful to RuboCop's `each_child_node(:pair)`),
        // excluding the kwsplat — the shape `{**h}` -> (hash (kwsplat …))
        // confirmed via `murphy ast`.
        let h_recv = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: key,
                args: murphy_ast::NodeList::EMPTY,
            },
            z,
        );
        let kwsplat = b.push(NodeKind::Kwsplat(OptNodeId::some(h_recv)), z);
        let pairs = b.push_list(&[kwsplat, pair]);
        let hash = b.push(NodeKind::Hash(pairs), z);
        let ast = b.finish(hash);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.hash_pairs(hash), vec![pair]);
        assert_eq!(cx.children(hash).len(), 2, "children includes the kwsplat");
        assert_eq!(cx.pair_key(pair).get(), Some(key_node));
        assert_eq!(cx.pair_value(pair).get(), Some(val_node));
        // Non-matching kinds project empty.
        assert!(cx.hash_pairs(pair).is_empty());
        assert!(cx.pair_key(hash).get().is_none());
    }

    #[test]
    fn pair_operator_predicates_distinguish_hash_rocket_and_colon() {
        with_parsed("{ a => b }", |cx, root| {
            let pair = cx.hash_pairs(root)[0];
            let op = cx.pair_operator_loc(pair);
            assert_eq!(cx.raw_source(op), "=>");
            assert!(cx.is_hash_rocket(pair));
            assert!(!cx.is_colon(pair));
        });
        with_parsed("{ a: b }", |cx, root| {
            let pair = cx.hash_pairs(root)[0];
            let op = cx.pair_operator_loc(pair);
            assert_eq!(cx.raw_source(op), ":");
            assert!(cx.is_colon(pair));
            assert!(!cx.is_hash_rocket(pair));
        });
        with_parsed("{ a: { b => c } }", |cx, root| {
            let pair = cx.hash_pairs(root)[0];
            assert!(cx.is_colon(pair), "outer pair uses the colon delimiter");
        });
        with_parsed("foo", |cx, root| {
            assert_eq!(cx.pair_operator_loc(root), Range::ZERO);
            assert!(!cx.is_hash_rocket(root));
            assert!(!cx.is_colon(root));
        });
    }

    #[test]
    fn pair_operator_loc_does_not_slice_multibyte_key_as_utf8() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let src = "あい 1";
        let mut b = AstBuilder::new(src.to_string(), "t.rb".to_string());
        let key_text = b.intern_string("あい");
        let key = b.push(NodeKind::Str(key_text), Range { start: 0, end: 6 });
        let value = b.push(NodeKind::Int(1), Range { start: 7, end: 8 });
        let pair = b.push(NodeKind::Pair { key, value }, Range { start: 0, end: 8 });
        let ast = b.finish(pair);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.pair_operator_loc(pair), Range::ZERO);
    }

    #[test]
    fn hash_mixed_delimiters_detects_colon_and_hash_rocket_pairs() {
        with_parsed("{ a: 1, b => 2 }", |cx, root| {
            assert!(cx.is_mixed_delimiters(root));
        });
        with_parsed("{ a: 1, b: 2 }", |cx, root| {
            assert!(!cx.is_mixed_delimiters(root));
        });
        with_parsed("{ a => 1, b => 2 }", |cx, root| {
            assert!(!cx.is_mixed_delimiters(root));
        });
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_mixed_delimiters(root));
        });
    }

    #[test]
    fn array_delimiter_predicates_distinguish_percent_and_square_brackets() {
        with_parsed("[1, 2]", |cx, root| {
            assert!(cx.is_square_brackets(root));
            assert!(cx.is_bracketed(root));
            assert!(!cx.is_percent_literal(root));
        });
        with_parsed("%w[a b]", |cx, root| {
            assert!(cx.is_percent_literal(root));
            assert!(cx.is_bracketed(root));
            assert!(!cx.is_square_brackets(root));
        });
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_percent_literal(root));
            assert!(!cx.is_square_brackets(root));
            assert!(!cx.is_bracketed(root));
        });
    }

    #[test]
    fn block_accessors_project_call_args_body() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let method = b.intern_symbol("each");
        let call = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method,
                args: murphy_ast::NodeList::EMPTY,
            },
            z,
        );
        let args = b.push(NodeKind::Args(murphy_ast::NodeList::EMPTY), z);
        let body = b.push(NodeKind::Int(1), z);
        let block = b.push(
            NodeKind::Block {
                call,
                args,
                body: OptNodeId::some(body),
            },
            z,
        );
        let ast = b.finish(block);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.block_call(block).get(), Some(call));
        assert_eq!(cx.block_arguments(block).get(), Some(args));
        assert_eq!(cx.block_body(block).get(), Some(body));
        // Non-Block node projects to NONE.
        assert!(cx.block_call(body).get().is_none());
    }

    #[test]
    fn loc_keyword_def() {
        let ast = murphy_translate::translate("def foo; end", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        let kw = cx.loc(root).keyword();
        assert_eq!(kw, Range { start: 0, end: 3 });
        assert_eq!(cx.raw_source(kw), "def");
    }

    #[test]
    fn loc_keyword_zero_for_send() {
        // `foo.bar` — a Send node has no keyword token at expression.start.
        let (ast, root) = build_call(
            "foo.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 7 },
            Some(Range { start: 3, end: 4 }),
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.loc(root).keyword(), Range::ZERO);
    }

    #[test]
    fn loc_keyword_zero_for_send_real_parse() {
        // Real prism parse: `foo` identifier token is at expression.start
        // but keyword() must return ZERO because Send is not keyword_bearing.
        let ast = murphy_translate::translate("foo.bar", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        assert_eq!(cx.loc(root).keyword(), Range::ZERO);
    }

    #[test]
    fn loc_begin_finds_open_paren() {
        let ast = murphy_translate::translate("foo(a, b)", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        assert_eq!(cx.loc(root).begin(), Range { start: 3, end: 4 });
    }

    #[test]
    fn loc_end_finds_close_paren() {
        let ast = murphy_translate::translate("foo(a, b)", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        assert_eq!(cx.loc(root).end(), Range { start: 8, end: 9 });
    }

    #[test]
    fn loc_begin_zero_when_no_parens() {
        let ast = murphy_translate::translate("foo a, b", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        assert_eq!(cx.loc(root).begin(), Range::ZERO);
        assert_eq!(cx.loc(root).end(), Range::ZERO);
    }

    #[test]
    fn loc_keyword_block_if() {
        // Block-form `if` starts with keyword: keyword() returns `if` range.
        let ast = murphy_translate::translate("if true; end", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        let kw = cx.loc(root).keyword();
        assert_eq!(kw, Range { start: 0, end: 2 });
        assert_eq!(cx.raw_source(kw), "if");
    }

    #[test]
    fn loc_keyword_zero_modifier_if() {
        // Modifier-form `if` places keyword after body: keyword() returns ZERO.
        let ast = murphy_translate::translate("1 if true", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        assert_eq!(cx.loc(root).keyword(), Range::ZERO);
    }

    #[test]
    fn loc_begin_zero_for_command_with_arg_paren() {
        // `foo bar(baz)` — outer Send has no `(`, only inner `bar(baz)` does.
        let ast = murphy_translate::translate("foo bar(baz)", "t.rb");
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let root = ast.root();
        // Outer `foo` call: begin()/end() must be ZERO.
        assert_eq!(cx.loc(root).begin(), Range::ZERO);
        assert_eq!(cx.loc(root).end(), Range::ZERO);
    }

    /// Parse `src` for real (prism → arena) and hand the root node to `f`.
    /// Unlike the hand-built `AstBuilder` fixtures, this exercises the
    /// actual translator, so loc-dependent predicates are verified against
    /// the real token/selector ranges they assume — not ranges the test
    /// planted itself.
    fn with_parsed(src: &str, f: impl FnOnce(&Cx<'_>, murphy_ast::NodeId)) {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let ast = murphy_translate::translate(src, "t.rb");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        f(&cx, ast.root());
    }

    #[test]
    fn dot_and_safe_navigation_on_real_parses() {
        with_parsed("a.b", |cx, root| {
            assert!(cx.is_dot(root), "a.b uses the dot operator");
            assert!(!cx.is_safe_navigation(root));
        });
        with_parsed("a&.b", |cx, root| {
            assert!(cx.is_safe_navigation(root), "a&.b is a csend");
            assert!(!cx.is_dot(root), "&. is not .");
        });
        // Operator sends have a receiver but no dot — es99.6's dot()
        // returns ZERO, so dot? must be false (not "Send with receiver").
        with_parsed("a + b", |cx, root| {
            assert!(
                !cx.is_dot(root),
                "a + b is an operator send, not a dot call"
            );
            assert!(!cx.is_safe_navigation(root));
        });
    }

    #[test]
    fn parenthesized_on_real_parses() {
        with_parsed("foo(1)", |cx, root| assert!(cx.is_parenthesized(root)));
        with_parsed("foo", |cx, root| assert!(!cx.is_parenthesized(root)));
        with_parsed("foo()", |cx, root| assert!(cx.is_parenthesized(root)));
        // A command call with a parenthesized *argument* (`foo (1)`, note
        // the space) has no parser-provided call closing paren. RuboCop's
        // `loc.end` therefore makes `parenthesized?` false.
        with_parsed("foo (1)", |cx, root| {
            assert!(
                !cx.is_parenthesized(root),
                "command + parenthesized argument is not a parenthesized call",
            );
        });
    }

    #[test]
    fn prefix_not_and_bang_on_real_parses() {
        with_parsed("!x", |cx, root| {
            assert!(cx.is_negation_method(root));
            assert!(cx.is_prefix_bang(root), "!x is the bang form");
            assert!(!cx.is_prefix_not(root));
        });
        with_parsed("not x", |cx, root| {
            assert!(cx.is_negation_method(root));
            assert!(cx.is_prefix_not(root), "not x is the keyword form");
            assert!(!cx.is_prefix_bang(root));
        });
        // A non-negation send is neither.
        with_parsed("x.foo", |cx, root| {
            assert!(!cx.is_negation_method(root));
            assert!(!cx.is_prefix_not(root));
            assert!(!cx.is_prefix_bang(root));
        });
    }

    #[test]
    fn method_name_delegates_through_block_nodes() {
        // `foo.each { }` — the Block delegates to its `foo.each` call,
        // so method_name is "each" (RuboCop parity: BlockNode is a
        // method-dispatch node).
        with_parsed("foo.each { }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Block { .. }));
            assert_eq!(cx.method_name(root), Some("each"));
            // The predicate wrappers route through method_name, so they
            // also see the inner call's selector.
            assert!(cx.is_enumerable_method(root));
        });
        // Numbered-parameter block `foo.map { _1 }`.
        with_parsed("foo.map { _1 }", |cx, root| {
            assert_eq!(cx.method_name(root), Some("map"));
        });
    }

    #[test]
    fn selector_resolves_call_and_keyword_like_nodes_on_real_parses() {
        with_parsed("foo.bar", |cx, root| {
            assert_eq!(cx.raw_source(cx.selector(root)), "bar");
        });
        with_parsed("foo&.bar", |cx, root| {
            assert_eq!(cx.raw_source(cx.selector(root)), "bar");
        });
        with_parsed("def m; yield(1); end", |cx, root| {
            let yield_node = cx
                .descendants(root)
                .into_iter()
                .find(|&id| matches!(cx.kind(id), NodeKind::Yield(_)))
                .expect("expected yield node");
            assert_eq!(cx.raw_source(cx.selector(yield_node)), "yield");
        });
        with_parsed("def m; super(1); end", |cx, root| {
            let super_node = cx
                .descendants(root)
                .into_iter()
                .find(|&id| matches!(cx.kind(id), NodeKind::Super(_)))
                .expect("expected super node");
            assert_eq!(cx.raw_source(cx.selector(super_node)), "super");
        });
        with_parsed("def m; super; end", |cx, root| {
            let zsuper_node = cx
                .descendants(root)
                .into_iter()
                .find(|&id| matches!(cx.kind(id), NodeKind::Zsuper))
                .expect("expected zsuper node");
            assert_eq!(cx.raw_source(cx.selector(zsuper_node)), "super");
        });
        with_parsed("defined?(foo)", |cx, root| {
            assert_eq!(cx.raw_source(cx.selector(root)), "defined?");
        });
    }

    #[test]
    fn block_node_finds_the_block_wrapping_a_send() {
        with_parsed("foo.each { bar }", |cx, root| {
            let NodeKind::Block { call, .. } = *cx.kind(root) else {
                panic!("expected block root");
            };
            assert_eq!(cx.block_node(call), OptNodeId::some(root));
            assert_eq!(cx.block_node(root), OptNodeId::NONE);
        });
    }

    #[test]
    fn assignment_operator_loc_is_bounded_to_the_selector_value_gap() {
        // Attribute write: `=` sits between selector and value.
        with_parsed("self.foo = bar", |cx, root| {
            let op = cx.assignment_operator_loc(root);
            assert_eq!(cx.raw_source(op), "=");
        });
        // `obj.foo=(x)` is parsed as the same attribute write — operator present.
        with_parsed("obj.foo=(x)", |cx, root| {
            assert_eq!(cx.raw_source(cx.assignment_operator_loc(root)), "=");
        });
        // Contamination guard: a `=` *inside an argument* is NOT the
        // call's operator (this is the case the unbounded scan got wrong).
        with_parsed("foo(a = 1)", |cx, root| {
            assert_eq!(
                cx.assignment_operator_loc(root),
                Range::ZERO,
                "the `=` inside the argument is not foo's operator",
            );
        });
        // A comparison is not an attribute write.
        with_parsed("a == b", |cx, root| {
            assert_eq!(cx.assignment_operator_loc(root), Range::ZERO);
        });
    }

    #[test]
    fn ternary_question_and_colon_are_bounded_between_branches() {
        with_parsed("a ? b : c", |cx, root| {
            assert_eq!(cx.raw_source(cx.ternary_question_loc(root)), "?");
            assert_eq!(cx.raw_source(cx.ternary_colon_loc(root)), ":");
        });
        // Contamination guard: a hash colon inside the then-branch is NOT
        // the ternary colon (the case the unbounded scan got wrong).
        with_parsed("a ? foo(x: 1) : c", |cx, root| {
            let colon = cx.ternary_colon_loc(root);
            assert_ne!(colon, Range::ZERO);
            // The matched colon is the ternary one — it sits after the
            // then-branch `foo(x: 1)`, not the `x:` hash colon within it.
            let then_end = cx.range(cx.if_then_branch(root).get().unwrap()).end;
            assert!(
                colon.start >= then_end,
                "matched the ternary colon, not the hash key colon"
            );
        });
        // Block-form and modifier `if` are not ternaries.
        with_parsed("if a then b end", |cx, root| {
            assert_eq!(cx.ternary_question_loc(root), Range::ZERO);
            assert_eq!(cx.ternary_colon_loc(root), Range::ZERO);
        });
        with_parsed("b if a", |cx, root| {
            assert_eq!(cx.ternary_question_loc(root), Range::ZERO);
        });
    }

    #[test]
    fn loc_end_keyword_marks_block_form_not_modifier() {
        with_parsed("if a then b end", |cx, root| {
            assert_eq!(cx.raw_source(cx.loc(root).end_keyword()), "end");
        });
        with_parsed("while a do b end", |cx, root| {
            assert_eq!(cx.raw_source(cx.loc(root).end_keyword()), "end");
        });
        // Modifier form has no `end`.
        with_parsed("b if a", |cx, root| {
            assert_eq!(cx.loc(root).end_keyword(), Range::ZERO);
        });
        // Ternary has no `end`.
        with_parsed("a ? b : c", |cx, root| {
            assert_eq!(cx.loc(root).end_keyword(), Range::ZERO);
        });
    }

    #[test]
    fn setter_ternary_modifier_consumer_predicates() {
        // setter_method?
        with_parsed("self.foo = bar", |cx, root| {
            assert!(cx.is_setter_method(root))
        });
        with_parsed("foo(a = 1)", |cx, root| assert!(!cx.is_setter_method(root)));
        with_parsed("a == b", |cx, root| assert!(!cx.is_setter_method(root)));

        // ternary?
        with_parsed("a ? b : c", |cx, root| assert!(cx.is_ternary(root)));
        with_parsed("if a then b end", |cx, root| assert!(!cx.is_ternary(root)));
        with_parsed("b if a", |cx, root| assert!(!cx.is_ternary(root)));

        // modifier_form? — if / unless / while modifiers are true;
        // block-form and ternary are false.
        with_parsed("b if a", |cx, root| assert!(cx.is_modifier_form(root)));
        with_parsed("b unless a", |cx, root| assert!(cx.is_modifier_form(root)));
        with_parsed("b while a", |cx, root| assert!(cx.is_modifier_form(root)));
        with_parsed("b until a", |cx, root| assert!(cx.is_modifier_form(root)));
        with_parsed("if a then b end", |cx, root| {
            assert!(!cx.is_modifier_form(root))
        });
        with_parsed("while a do b end", |cx, root| {
            assert!(!cx.is_modifier_form(root))
        });
        with_parsed("until a do b end", |cx, root| {
            assert!(!cx.is_modifier_form(root))
        });
        // Ternary lacks `end` but is excluded by the ternary? guard.
        with_parsed("a ? b : c", |cx, root| assert!(!cx.is_modifier_form(root)));
    }

    #[test]
    fn if_keyword_predicates_distinguish_if_unless_elsif_and_modifiers() {
        with_parsed("if a then b end", |cx, root| {
            assert_eq!(cx.if_keyword(root), "if");
            assert_eq!(cx.raw_source(cx.if_keyword_loc(root)), "if");
            assert!(cx.is_if(root));
            assert!(!cx.is_unless(root));
            assert!(!cx.is_elsif(root));
            assert!(cx.is_then(root));
            assert!(!cx.is_else(root));
            assert_eq!(cx.if_inverse_keyword(root), "unless");
        });

        with_parsed("b unless a", |cx, root| {
            assert_eq!(cx.if_keyword(root), "unless");
            assert_eq!(cx.raw_source(cx.if_keyword_loc(root)), "unless");
            assert!(!cx.is_if(root));
            assert!(cx.is_unless(root));
            assert!(cx.is_modifier_form(root));
            assert_eq!(cx.if_inverse_keyword(root), "if");
        });

        with_parsed("if a then b elsif c then d else e end", |cx, root| {
            assert!(cx.is_else(root));
            let nested = cx.if_else_branch(root).get().expect("elsif is nested if");
            assert_eq!(cx.if_keyword(nested), "elsif");
            assert_eq!(cx.raw_source(cx.if_keyword_loc(nested)), "elsif");
            assert_eq!(cx.if_inverse_keyword(nested), "");
            assert!(cx.is_elsif(nested));
            assert!(!cx.is_nested_conditional(nested));
            assert!(cx.is_then(nested));
            assert!(cx.is_else(nested));
        });

        with_parsed("if a then b elsif c then d end", |cx, root| {
            assert!(cx.is_else(root), "else? is true for elsif clauses");
        });
    }

    #[test]
    fn nested_conditional_detects_non_elsif_if_inside_branches() {
        with_parsed("if a then foo(if b then c end) end", |cx, root| {
            assert!(cx.is_nested_conditional(root));
        });

        with_parsed("if a then b elsif c then d end", |cx, root| {
            let nested = cx.if_else_branch(root).get().expect("elsif is nested if");
            assert!(!cx.is_nested_conditional(root));
            assert!(!cx.is_nested_conditional(nested));
        });
    }

    #[test]
    fn if_keyword_scan_ignores_child_conditionals() {
        with_parsed("if foo(if a then b end) then c end", |cx, root| {
            assert_eq!(cx.if_keyword(root), "if");
            assert_eq!(cx.raw_source(cx.if_keyword_loc(root)), "if");

            let cond = cx.if_condition(root).get().expect("outer condition");
            let inner = cx
                .descendants(cond)
                .into_iter()
                .find(|&id| matches!(cx.kind(id), NodeKind::If { .. }))
                .expect("nested if in parenthesized condition");
            assert_eq!(cx.if_keyword(inner), "if");
            assert_ne!(cx.if_keyword_loc(root), cx.if_keyword_loc(inner));
        });
    }

    #[test]
    fn if_branch_accessors_preserve_translator_unless_swap() {
        with_parsed("unless a then b else c end", |cx, root| {
            assert!(cx.is_unless(root));
            assert_eq!(
                cx.raw_source(cx.range(cx.if_branch(root).get().unwrap())),
                "c"
            );
            assert_eq!(
                cx.raw_source(cx.range(cx.else_branch(root).get().unwrap())),
                "b"
            );
        });

        with_parsed("if a then b else c end", |cx, root| {
            assert!(cx.is_if(root));
            assert_eq!(
                cx.raw_source(cx.range(cx.if_branch(root).get().unwrap())),
                "b"
            );
            assert_eq!(
                cx.raw_source(cx.range(cx.else_branch(root).get().unwrap())),
                "c"
            );
        });
    }

    #[test]
    fn array_accessor_projects_elements() {
        with_parsed("[10, 20, 30]", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Array(_)));
            assert_eq!(cx.array_elements(root).len(), 3);
        });
        with_parsed("[]", |cx, root| assert!(cx.array_elements(root).is_empty()));
        // Non-array projects empty.
        with_parsed("foo", |cx, root| {
            assert!(cx.array_elements(root).is_empty())
        });
    }

    #[test]
    fn case_and_when_accessors_project_parts() {
        let src = "case x\nwhen 1, 2 then a\nelse b\nend";
        with_parsed(src, |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Case { .. }));
            assert!(cx.case_subject(root).get().is_some());
            let whens = cx.case_when_branches(root);
            assert_eq!(whens.len(), 1);
            assert!(cx.case_else_branch(root).get().is_some());
            // The When child: two conditions, a body.
            let when = whens[0];
            assert!(matches!(cx.kind(when), NodeKind::When { .. }));
            assert_eq!(cx.when_conditions(when).len(), 2);
            assert!(cx.when_body(when).get().is_some());
        });
        // Subject-less `case` has no subject.
        with_parsed("case\nwhen a then b\nend", |cx, root| {
            assert!(cx.case_subject(root).get().is_none());
            assert_eq!(cx.case_when_branches(root).len(), 1);
            assert!(cx.case_else_branch(root).get().is_none());
        });
    }

    #[test]
    fn def_accessors_project_receiver_args_body() {
        // Plain instance method: no receiver, args + body present.
        with_parsed("def foo(a, b)\n  c\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Def { .. }));
            assert!(cx.def_receiver(root).get().is_none());
            assert!(cx.def_arguments(root).get().is_some());
            assert!(cx.def_body(root).get().is_some());
        });
        // Singleton method `def self.foo`: receiver present.
        with_parsed("def self.foo\nend", |cx, root| {
            assert!(
                cx.def_receiver(root).get().is_some(),
                "def self.foo has a receiver"
            );
            assert!(cx.def_body(root).get().is_none(), "empty body");
        });
        // Non-def projects empty.
        with_parsed("foo", |cx, root| {
            assert!(cx.def_receiver(root).get().is_none());
            assert!(cx.def_arguments(root).get().is_none());
        });
    }

    #[test]
    fn basic_literal_excludes_composites() {
        // Note: `1r` / `1i` currently parse to `Unknown` in Murphy's
        // translator (it does not yet emit Rational/Complex), so they are
        // not exercised here; `is_basic_literal` still matches those
        // variants for when the translator produces them.
        for src in ["42", "\"s\"", ":sym", "nil", "true", "false", "1.5"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_basic_literal(root), "{src} is a basic literal");
            });
        }
        // Composites and non-literals are not basic literals.
        for src in ["[1]", "{a: 1}", "1..2", "foo"] {
            with_parsed(src, |cx, root| {
                assert!(!cx.is_basic_literal(root), "{src} is not a basic literal");
            });
        }
    }

    #[test]
    fn recursive_basic_literal_recurses_through_composites_and_literal_ops() {
        for src in [
            "42",
            "[1, 2]",
            "[1, [2, 3]]",
            "{a: 1}",
            "1..2",
            "\"a\" \"b\"", // adjacent-string concat -> dstr
            "1 == 2",
            "1 * 2", // `*` is a LITERAL_RECURSIVE_METHOD
            "!true",
            "1 <=> 2",
        ] {
            with_parsed(src, |cx, root| {
                assert!(
                    cx.is_recursive_basic_literal(root),
                    "{src} should be recursive_basic_literal"
                );
            });
        }
        for src in [
            "[1, foo]", // foo is a non-literal send
            "foo",
            "1 + 2", // `+` is NOT in LITERAL_RECURSIVE_METHODS (RuboCop quirk)
            "[1, foo.bar]",
            "{a: foo}",
        ] {
            with_parsed(src, |cx, root| {
                assert!(
                    !cx.is_recursive_basic_literal(root),
                    "{src} should NOT be recursive_basic_literal"
                );
            });
        }
    }

    #[test]
    fn pure_predicate_covers_leaves_and_pure_composites() {
        for src in [
            "42",
            ":s",
            "nil",
            "true",
            "1.5", // literal leaves
            "@x",
            "$x",
            "@@x",
            "FOO",         // ivar/gvar/cvar/const reads
            "defined?(x)", // defined? is a pure leaf
            "[1, 2]",
            "{a: 1}",
            "1..2", // pure composites
            "1 && 2",
            "1 || 2", // and/or of pure (note: `!x` parses to a
            // `send :!`, not a `not` node, so it is not pure — matching RuboCop)
            "1 if true", // if: pure cond + pure branch
        ] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_pure(root), "{src} should be pure")
            });
        }
        for src in [
            "foo", // a method send may have side effects
            "foo.bar", "puts 1", "[1, foo]", // composite with a non-pure child
            "x = 1",    // assignment
            "@x = 1",
        ] {
            with_parsed(src, |cx, root| {
                assert!(!cx.is_pure(root), "{src} should NOT be pure")
            });
        }
    }

    #[test]
    fn value_used_walks_parent_context() {
        with_parsed("foo(bar)", |cx, root| {
            let arg = cx.call_arguments(root)[0];
            assert!(cx.is_value_used(arg));
        });
        with_parsed("x = 1", |cx, root| {
            let value = cx.children(root)[0];
            assert!(cx.is_value_used(value));
        });
        with_parsed("if a then b end", |cx, root| {
            assert!(cx.is_value_used(cx.if_condition(root).get().unwrap()));
            assert!(!cx.is_value_used(cx.if_then_branch(root).get().unwrap()));
        });
        with_parsed("[1, 2]", |cx, root| {
            assert!(!cx.is_value_used(cx.array_elements(root)[0]));
        });
        with_parsed("x = [1, 2]", |cx, root| {
            let array = cx.children(root)[0];
            assert!(cx.is_value_used(cx.array_elements(array)[0]));
        });
        with_parsed("while a do b end", |cx, root| {
            let kids = cx.children(root);
            assert!(cx.is_value_used(kids[0]), "while condition is used");
            assert!(!cx.is_value_used(kids[1]), "while body value is discarded");
        });
    }

    fn first_send_named(cx: &Cx<'_>, root: murphy_ast::NodeId, name: &str) -> murphy_ast::NodeId {
        std::iter::once(root)
            .chain(cx.descendants(root))
            .find(|&id| {
                matches!(cx.kind(id), NodeKind::Send { .. }) && cx.method_name(id) == Some(name)
            })
            .unwrap_or_else(|| panic!("expected to find send `{name}`"))
    }

    #[test]
    fn global_const_matches_unscoped_and_cbase_constants() {
        with_parsed("Class.new", |cx, root| {
            let receiver = cx.call_receiver(root).get().unwrap();
            assert!(cx.is_global_const(receiver, "Class"));
            assert!(!cx.is_global_const(receiver, "Struct"));
        });
        with_parsed("::Class.new", |cx, root| {
            let receiver = cx.call_receiver(root).get().unwrap();
            assert!(cx.is_global_const(receiver, "Class"));
        });
        with_parsed("A::Class.new", |cx, root| {
            let receiver = cx.call_receiver(root).get().unwrap();
            assert!(!cx.is_global_const(receiver, "Class"));
        });
    }

    #[test]
    fn class_constructor_matches_new_and_define_forms() {
        for src in ["Class.new", "Module.new", "Struct.new", "Data.define(:id)"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_class_constructor(root), "{src}")
            });
        }
        with_parsed("Class.new do\n  foo\nend", |cx, root| {
            assert!(
                cx.is_class_constructor(root),
                "block form is a class constructor"
            );
            assert!(cx.is_class_constructor(cx.block_call(root).get().unwrap()));
        });
        with_parsed("Object.new", |cx, root| {
            assert!(!cx.is_class_constructor(root))
        });
        with_parsed("Class.define(:id)", |cx, root| {
            assert!(!cx.is_class_constructor(root))
        });
    }

    #[test]
    fn in_macro_scope_follows_rubocop_wrapper_rules() {
        with_parsed("top_level_macro", |cx, root| {
            assert!(cx.is_in_macro_scope(root))
        });
        with_parsed(
            "class C\n  macro_call\n  if condition\n    nested_macro\n  end\nend",
            |cx, root| {
                let macro_call = first_send_named(cx, root, "macro_call");
                let condition = first_send_named(cx, root, "condition");
                let nested_macro = first_send_named(cx, root, "nested_macro");

                assert!(cx.is_in_macro_scope(macro_call));
                assert!(!cx.is_in_macro_scope(condition));
                assert!(cx.is_in_macro_scope(nested_macro));
            },
        );
        with_parsed("Class.new do\n  macro_call\nend", |cx, root| {
            let macro_call = first_send_named(cx, root, "macro_call");
            assert!(cx.is_in_macro_scope(macro_call));
        });
        with_parsed(
            "def build\n  Class.new do\n    not_macro\n  end\nend",
            |cx, root| {
                let not_macro = first_send_named(cx, root, "not_macro");
                assert!(!cx.is_in_macro_scope(not_macro));
            },
        );
        with_parsed("foo(argument_call)", |cx, root| {
            let argument_call = first_send_named(cx, root, "argument_call");
            assert!(!cx.is_in_macro_scope(argument_call));
        });
    }

    #[test]
    fn macro_predicate_requires_implicit_receiver_in_macro_scope() {
        with_parsed(
            "class C\n  macro_call\n  self.not_macro\nend",
            |cx, root| {
                let macro_call = first_send_named(cx, root, "macro_call");
                let not_macro = first_send_named(cx, root, "not_macro");
                assert!(cx.is_macro(macro_call));
                assert!(!cx.is_macro(not_macro));
            },
        );
        with_parsed("foo(argument_call)", |cx, root| {
            let argument_call = first_send_named(cx, root, "argument_call");
            assert!(!cx.is_macro(argument_call));
        });
    }

    #[test]
    fn access_modifier_predicates_match_bare_and_non_bare_forms() {
        with_parsed(
            "class C\n  private\n  protected :foo\n  public(:bar)\n  module_function :baz\nend",
            |cx, root| {
                let private = first_send_named(cx, root, "private");
                let protected = first_send_named(cx, root, "protected");
                let public = first_send_named(cx, root, "public");
                let module_function = first_send_named(cx, root, "module_function");

                assert!(cx.is_bare_access_modifier(private));
                assert!(!cx.is_non_bare_access_modifier(private));
                assert!(cx.is_special_modifier(private));

                assert!(!cx.is_bare_access_modifier(protected));
                assert!(cx.is_non_bare_access_modifier(protected));
                assert!(!cx.is_special_modifier(protected));

                assert!(cx.is_access_modifier(public));
                assert!(cx.is_access_modifier(module_function));
            },
        );
        with_parsed("private", |cx, root| assert!(cx.is_access_modifier(root)));
        with_parsed("obj.private", |cx, root| {
            assert!(!cx.is_access_modifier(root))
        });
    }

    #[test]
    fn def_modifier_finds_def_argument_through_modifier_chain() {
        with_parsed("private def foo\nend", |cx, root| {
            let modified = cx.def_modifier(root).get().expect("def modifier target");
            assert!(matches!(cx.kind(modified), NodeKind::Def { .. }));
            assert!(cx.is_def_modifier(root));
        });
        with_parsed("private protected def foo\nend", |cx, root| {
            let modified = cx
                .def_modifier(root)
                .get()
                .expect("nested def modifier target");
            assert!(matches!(cx.kind(modified), NodeKind::Def { .. }));
            assert!(cx.is_def_modifier(root));
        });
        with_parsed("private :foo", |cx, root| {
            assert!(cx.def_modifier(root).get().is_none());
            assert!(!cx.is_def_modifier(root));
        });
    }

    // --- es99.8: token-search helpers ---

    #[test]
    fn token_before_returns_last_token_ending_at_or_before_offset() {
        // `a = b`: the `=` token spans [2, 3). Searching just before `b`
        // (offset 4) must return the `=`, not the `b` identifier.
        with_parsed("a = b", |cx, _root| {
            let eq = cx.token_before(4).expect("token before offset 4");
            assert_eq!(cx.token_text(eq), "=");
        });
    }

    #[test]
    fn token_after_returns_first_token_starting_at_or_after_offset() {
        // `a = b`: from offset 1 (just after `a`) the next token is `=`.
        with_parsed("a = b", |cx, _root| {
            let eq = cx.token_after(1).expect("token after offset 1");
            assert_eq!(cx.token_text(eq), "=");
        });
    }

    #[test]
    fn token_before_and_after_at_boundaries() {
        with_parsed("a = b", |cx, _root| {
            // Nothing ends at or before offset 0.
            assert!(cx.token_before(0).is_none());
            // Prism emits a zero-width EOF token at the end of source, so
            // `token_after(len)` finds it. Past the EOF token's start there
            // is nothing left.
            let past_end = cx.source().len() as u32 + 1;
            assert!(cx.token_after(past_end).is_none());
        });
    }

    #[test]
    fn tokens_in_returns_fully_contained_tokens() {
        // `[1, 2]`: the comma at [2, 3) is fully inside the array's range.
        with_parsed("[1, 2]", |cx, root| {
            let toks = cx.tokens_in(cx.range(root));
            assert!(
                toks.iter().any(|t| t.kind == SourceTokenKind::Comma),
                "array literal contains a comma token"
            );
        });
    }

    #[test]
    fn braces_via_tokens_in_finds_hash_and_block_braces() {
        // Demonstrate a `braces?`-style consumer: a hash/block node whose
        // range contains a LeftBrace + RightBrace token pair.
        let has_braces = |cx: &Cx<'_>, id: murphy_ast::NodeId| {
            let toks = cx.tokens_in(cx.range(id));
            toks.iter().any(|t| t.kind == SourceTokenKind::LeftBrace)
                && toks.iter().any(|t| t.kind == SourceTokenKind::RightBrace)
        };
        with_parsed("{a: 1}", |cx, root| {
            assert!(has_braces(cx, root), "hash literal is brace-delimited");
        });
        with_parsed("foo { }", |cx, root| {
            assert!(has_braces(cx, root), "brace block is brace-delimited");
        });
        // `do`/`end` block is NOT brace-delimited.
        with_parsed("foo do end", |cx, root| {
            assert!(!has_braces(cx, root), "do/end block has no brace tokens");
        });
    }

    #[test]
    fn sorted_tokens_are_monotonic_even_with_heredoc_and_interpolation() {
        // Heredocs/interpolation are where prism's lex order is least
        // source-like, so pin the invariants the partition-based helpers
        // stand on, on exactly that input:
        //   - `token_after`/`tokens_in` partition on `start` → start-monotonic
        //   - `token_before` partitions on `end`            → end-monotonic
        // (Strict non-overlap does NOT hold: a heredoc-end token folds the
        // trailing newline and shares its `end` with the standalone newline
        // token — an equal-end overlap that is benign for all three helpers.)
        let src = "foo(<<~H, \"a#{b}c\")\nbody\nH\n";
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let ast = murphy_translate::translate(src, "t.rb");
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let toks = cx.sorted_tokens();
        assert!(
            toks.windows(2)
                .all(|p| p[0].range.start <= p[1].range.start),
            "start-monotonic (token_after/tokens_in): {toks:?}",
        );
        assert!(
            toks.windows(2).all(|p| p[0].range.end <= p[1].range.end),
            "end-monotonic (token_before): {toks:?}",
        );
        // The helpers agree on this input: the token before the `,` offset
        // is the heredoc opener.
        let comma = toks
            .iter()
            .find(|t| t.kind == SourceTokenKind::Comma)
            .expect("comma present");
        let before = cx
            .token_before(comma.range.start)
            .expect("token before comma");
        assert_eq!(cx.token_text(before), "<<~H");
    }

    #[test]
    fn tokens_in_trims_multiple_trailing_straddlers() {
        // Regression (gemini, PR #129): two equal-end tokens both start
        // inside the query range but end past it. A single-`if` trim would
        // leave one straddler in; the `while` loop must drop both.
        use murphy_ast::{AstBuilder, NodeKind, Range};
        let mut b = AstBuilder::new("xxxxxx", "t.rb");
        let root = b.push(NodeKind::Int(1), Range { start: 0, end: 1 });
        // Contained.
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::Comma,
            range: Range { start: 0, end: 2 },
        });
        // Two straddlers: start < 4 (inside [0,4)) but end == 6 > 4.
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::LeftBrace,
            range: Range { start: 2, end: 6 },
        });
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::RightBrace,
            range: Range { start: 3, end: 6 },
        });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        let toks = cx.tokens_in(Range { start: 0, end: 4 });
        assert_eq!(toks.len(), 1, "both straddlers trimmed, only [0,2) remains");
        assert_eq!(toks[0].kind, SourceTokenKind::Comma);
    }

    #[test]
    fn colon_colon_found_via_text_search_without_a_dedicated_kind() {
        // `::` lands in `Other`; consumers retrieve it by token text, the
        // same path `=`/`end` use. No dedicated SourceTokenKind needed.
        with_parsed("Foo::Bar", |cx, root| {
            let has_colon_colon = cx
                .tokens_in(cx.range(root))
                .iter()
                .any(|t| cx.token_text(*t) == "::");
            assert!(has_colon_colon, "Foo::Bar contains a `::` token");
        });
    }

    #[test]
    fn for_node_accessors_on_real_parse() {
        // `for x in [1, 2]; x; end` — the translator now emits a `For`
        // (previously `Unknown`), so the accessors and the dependent
        // value/pure paths see a real node.
        with_parsed("for x in [1, 2]\n  x\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::For { .. }));
            // variable is an Lvasgn target, collection an Array, body present.
            let var = cx.for_variable(root).get().unwrap();
            assert!(matches!(cx.kind(var), NodeKind::Lvasgn { .. }));
            let coll = cx.for_collection(root).get().unwrap();
            assert!(matches!(cx.kind(coll), NodeKind::Array(..)));
            assert!(cx.for_body(root).get().is_some());
            // `for` is a loop construct, not a ternary/modifier.
            assert!(!cx.is_modifier_form(root));
            // Non-For projects to NONE.
            assert!(cx.for_variable(coll).get().is_none());
        });
    }

    #[test]
    fn numblock_itblock_on_real_parse() {
        // Numbered-parameter block → Numblock; method_name delegates to
        // the inner call; numblock_max gives the highest `_n`.
        with_parsed("foo.map { _1 + _2 }", |cx, root| {
            assert!(cx.is_numblock(root));
            assert_eq!(cx.numblock_max(root), Some(2));
            assert_eq!(cx.method_name(root), Some("map"));
            assert!(!cx.is_itblock(root));
        });
        // `it`-parameter block → Itblock.
        with_parsed("foo { it.bar }", |cx, root| {
            assert!(cx.is_itblock(root));
            assert_eq!(cx.method_name(root), Some("foo"));
            assert!(cx.numblock_max(root).is_none());
        });
        // An ordinary block is neither.
        with_parsed("foo { |x| x }", |cx, root| {
            assert!(!cx.is_numblock(root));
            assert!(!cx.is_itblock(root));
        });
    }

    #[test]
    fn lambda_predicates_on_real_parse() {
        // Stabby lambda — emitted as a Block over the Lambda marker.
        with_parsed("-> { 1 }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Block { .. }));
            assert!(cx.is_lambda(root));
            assert!(cx.is_lambda_literal(root));
        });
        // Params/body of a stabby lambda translate (previously Unknown).
        with_parsed("->(x) { x }", |cx, root| {
            assert!(cx.is_lambda_literal(root));
            assert!(cx.block_arguments(root).get().is_some());
            assert!(cx.block_body(root).get().is_some());
        });
        // `lambda { }` method form — a lambda, but not a lambda *literal*.
        with_parsed("lambda { 1 }", |cx, root| {
            assert!(cx.is_lambda(root));
            assert!(!cx.is_lambda_literal(root));
        });
        // An ordinary block is neither.
        with_parsed("foo { 1 }", |cx, root| {
            assert!(!cx.is_lambda(root));
            assert!(!cx.is_lambda_literal(root));
        });
    }

    #[test]
    fn numeric_reference_forwarding_on_real_parse() {
        // numeric_type? — int/float/rational/complex (1r/1i now emit real
        // Rational/Complex nodes instead of Unknown).
        for src in ["42", "1.5", "1r", "1i"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_numeric(root), "{src} is numeric")
            });
        }
        for src in ["\"s\"", ":x", "nil"] {
            with_parsed(src, |cx, root| {
                assert!(!cx.is_numeric(root), "{src} is not numeric")
            });
        }
        // reference? — numbered ($1) and back ($&) references.
        with_parsed("$1", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::NthRef(..)));
            assert!(cx.is_reference(root));
        });
        with_parsed("$&", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::BackRef(..)));
            assert!(cx.is_reference(root));
        });
        with_parsed("@x", |cx, root| assert!(!cx.is_reference(root)));
        // argument_forwarding? — `def f(...)`.
        with_parsed("def f(...)\nend", |cx, root| {
            assert!(cx.is_argument_forwarding(root))
        });
        with_parsed("def f(a)\nend", |cx, root| {
            assert!(!cx.is_argument_forwarding(root))
        });
    }

    // ── es99.25 tests ──────────────────────────────────────────────

    #[test]
    fn mutable_and_immutable_literals_partition_literals() {
        // MUTABLE_LITERALS = str dstr xstr array hash regexp irange erange
        for src in [
            "\"s\"",
            "\"a#{b}\"",
            "`c`",
            "[1]",
            "{a: 1}",
            "/re/",
            "1..2",
            "1...2",
        ] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_mutable_literal(root), "{src} is a mutable literal");
                assert!(cx.is_literal(root), "{src} is a literal");
                assert!(
                    !cx.is_immutable_literal(root),
                    "{src} must not also be immutable",
                );
            });
        }
        // IMMUTABLE = LITERALS - MUTABLE: numerics, sym/dsym, true/false/nil.
        for src in [
            "1",
            "1.5",
            "1r",
            "1i",
            ":s",
            ":\"a#{b}\"",
            "true",
            "false",
            "nil",
        ] {
            with_parsed(src, |cx, root| {
                assert!(
                    cx.is_immutable_literal(root),
                    "{src} is an immutable literal"
                );
                assert!(cx.is_literal(root), "{src} is a literal");
                assert!(!cx.is_mutable_literal(root), "{src} must not be mutable");
            });
        }
        // Non-literals are neither.
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_mutable_literal(root));
            assert!(!cx.is_immutable_literal(root));
        });
    }

    #[test]
    fn truthy_and_falsey_literals_partition_literals() {
        // FALSEY_LITERALS = false nil.
        for src in ["false", "nil"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_falsey_literal(root), "{src} is falsey");
                assert!(!cx.is_truthy_literal(root), "{src} is not truthy");
                assert!(cx.is_literal(root));
            });
        }
        // Everything else literal is truthy (TRUTHY ⊔ FALSEY = LITERALS).
        for src in ["1", "\"s\"", ":s", "[1]", "{a: 1}", "/re/", "1..2", "true"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_truthy_literal(root), "{src} is truthy");
                assert!(!cx.is_falsey_literal(root), "{src} is not falsey");
                assert!(cx.is_literal(root));
            });
        }
        // A non-literal is neither.
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_truthy_literal(root));
            assert!(!cx.is_falsey_literal(root));
        });
    }

    #[test]
    fn recursive_literal_includes_composite_leaves() {
        // Composite literal whose children recurse — qualifies under
        // recursive_literal? but not recursive_basic_literal?.
        with_parsed("[1, 2]", |cx, root| {
            assert!(
                cx.is_recursive_literal(root),
                "[1, 2] is a recursive literal"
            );
            assert!(
                cx.is_recursive_basic_literal(root),
                "[1, 2]'s leaves are basic too",
            );
        });
        // An array containing a composite leaf: literal-recursive yes,
        // basic-recursive yes (arrays recurse in both). Use a hash leaf to
        // separate them: a bare `{a: 1}` is composite — recursive_literal?
        // (its pair recurses to int) but its element is not "basic".
        with_parsed("[[1], {a: 2}]", |cx, root| {
            assert!(cx.is_recursive_literal(root));
        });
        // Literal-operator send on composite operands — recurse via send arm.
        // `[1]`/`[2]` are arrays of basic ints, so they qualify as recursive
        // *basic* literals too (the array arm recurses over its int children).
        with_parsed("[1] == [2]", |cx, root| {
            assert!(
                cx.is_recursive_literal(root),
                "[1] == [2] recurses on literals"
            );
            assert!(cx.is_recursive_basic_literal(root));
        });
        // The recursive_literal/basic split shows on a composite *leaf*: a
        // bare hash `{a: 1}` is a recursive literal but NOT a recursive basic
        // literal (hash ∈ COMPOSITE_LITERALS, excluded from BASIC_LITERALS),
        // yet both predicates recurse through hash, so the discriminator is
        // the leaf int — equal here. Use a string-receiver send to separate:
        // `[1].foo` — `foo` ∉ LITERAL_RECURSIVE_METHODS → both false.
        with_parsed("[1].foo", |cx, root| {
            assert!(
                !cx.is_recursive_literal(root),
                "foo is not a literal-recursive method"
            );
        });
        // A variable read is not a recursive literal.
        with_parsed("[x]", |cx, root| {
            assert!(!cx.is_recursive_literal(root), "[x] holds a non-literal");
        });
    }

    #[test]
    fn operator_keyword_is_and_or() {
        with_parsed("a and b", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::And { .. }));
            assert!(cx.is_operator_keyword(root));
        });
        with_parsed("a or b", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Or { .. }));
            assert!(cx.is_operator_keyword(root));
        });
        with_parsed("a && b", |cx, root| assert!(cx.is_operator_keyword(root)));
        with_parsed("a + b", |cx, root| assert!(!cx.is_operator_keyword(root)));
    }

    #[test]
    fn loop_and_post_condition_predicates() {
        // Post forms (post: true) — both loop_keyword? and post_condition_loop?.
        with_parsed("begin; x; end while y", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::While { post: true, .. }));
            assert!(cx.is_post_condition_loop(root));
            assert!(cx.is_loop_keyword(root));
            // RuboCop's BASIC_CONDITIONALS excludes while_post.
            assert!(
                !cx.is_basic_conditional(root),
                "while_post is not a basic conditional"
            );
            assert!(!cx.is_conditional(root));
        });
        with_parsed("begin; x; end until y", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Until { post: true, .. }));
            assert!(cx.is_post_condition_loop(root));
            assert!(cx.is_loop_keyword(root));
        });
        // Pre forms (post: false) — loop_keyword? + basic_conditional?, but
        // not post_condition_loop?.
        with_parsed("x while y", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::While { post: false, .. }));
            assert!(!cx.is_post_condition_loop(root));
            assert!(cx.is_loop_keyword(root));
            assert!(cx.is_basic_conditional(root));
            assert!(cx.is_conditional(root));
        });
        with_parsed("for i in a; end", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::For { .. }));
            assert!(cx.is_loop_keyword(root), "for is a loop keyword");
            assert!(!cx.is_post_condition_loop(root));
            assert!(!cx.is_basic_conditional(root), "for is not a conditional");
        });
    }

    #[test]
    fn conditional_predicates() {
        with_parsed("if a; end", |cx, root| {
            assert!(cx.is_basic_conditional(root));
            assert!(cx.is_conditional(root));
        });
        with_parsed("case a; when 1; end", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Case { .. }));
            assert!(
                !cx.is_basic_conditional(root),
                "case is conditional but not basic"
            );
            assert!(cx.is_conditional(root));
        });
        // case/in (CaseMatch) — the translator does not yet emit CaseMatch
        // from real source (`case a; in 1; end` parses to Unknown), so the
        // predicate is exercised against a hand-built CaseMatch fixture.
        {
            let mut b = AstBuilder::new("case a; in 1; end", "t.rb".to_string());
            let subj = b.push(NodeKind::Nil, Range { start: 5, end: 6 });
            let pat = b.push(NodeKind::Int(1), Range { start: 11, end: 12 });
            let inp = b.push(
                NodeKind::InPattern {
                    pattern: pat,
                    guard: OptNodeId::NONE,
                    body: OptNodeId::NONE,
                },
                Range { start: 8, end: 12 },
            );
            let ins = b.push_list(&[inp]);
            let root = b.push(
                NodeKind::CaseMatch {
                    subject: subj,
                    in_patterns: ins,
                    else_body: OptNodeId::NONE,
                },
                Range { start: 0, end: 17 },
            );
            let ast = b.finish(root);
            let fns = FnTable {
                emit_offense: noop_offense,
                emit_edit: noop_edit,
            };
            let raw = cx_raw_for(&ast, &fns);
            let cx = unsafe { Cx::from_raw(&raw) };
            assert!(matches!(cx.kind(root), NodeKind::CaseMatch { .. }));
            assert!(!cx.is_basic_conditional(root));
            assert!(cx.is_conditional(root), "case/in is a conditional");
        }
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_basic_conditional(root));
            assert!(!cx.is_conditional(root));
        });
    }

    #[test]
    fn type_group_predicates() {
        // boolean_type? = true/false (not nil).
        with_parsed("true", |cx, root| assert!(cx.is_boolean_type(root)));
        with_parsed("false", |cx, root| assert!(cx.is_boolean_type(root)));
        with_parsed("nil", |cx, root| assert!(!cx.is_boolean_type(root)));
        // range_type? = irange/erange (folded RangeExpr).
        with_parsed("1..2", |cx, root| assert!(cx.is_range_type(root)));
        with_parsed("1...2", |cx, root| assert!(cx.is_range_type(root)));
        with_parsed("1", |cx, root| assert!(!cx.is_range_type(root)));
        // any_block_type? = block/numblock/itblock. The translator emits a
        // plain Block for `foo { }`; Numblock/Itblock are not produced from
        // real source yet, so those two arms use hand-built fixtures.
        with_parsed("foo { }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Block { .. }));
            assert!(cx.is_any_block_type(root));
        });
        {
            // Numblock fixture: `foo { _1 }`.
            let mut b = AstBuilder::new("foo { _1 }", "t.rb".to_string());
            let m = b.intern_symbol("foo");
            let send = b.push(
                NodeKind::Send {
                    receiver: OptNodeId::NONE,
                    method: m,
                    args: murphy_ast::NodeList::EMPTY,
                },
                Range { start: 0, end: 3 },
            );
            let root = b.push(
                NodeKind::Numblock {
                    send,
                    max_n: 1,
                    body: OptNodeId::NONE,
                },
                Range { start: 0, end: 10 },
            );
            let ast = b.finish(root);
            let fns = FnTable {
                emit_offense: noop_offense,
                emit_edit: noop_edit,
            };
            let raw = cx_raw_for(&ast, &fns);
            let cx = unsafe { Cx::from_raw(&raw) };
            assert!(matches!(cx.kind(root), NodeKind::Numblock { .. }));
            assert!(cx.is_any_block_type(root));
        }
        {
            // Itblock fixture: `foo { it }`.
            let mut b = AstBuilder::new("foo { it }", "t.rb".to_string());
            let m = b.intern_symbol("foo");
            let send = b.push(
                NodeKind::Send {
                    receiver: OptNodeId::NONE,
                    method: m,
                    args: murphy_ast::NodeList::EMPTY,
                },
                Range { start: 0, end: 3 },
            );
            let root = b.push(
                NodeKind::Itblock {
                    send,
                    body: OptNodeId::NONE,
                },
                Range { start: 0, end: 10 },
            );
            let ast = b.finish(root);
            let fns = FnTable {
                emit_offense: noop_offense,
                emit_edit: noop_edit,
            };
            let raw = cx_raw_for(&ast, &fns);
            let cx = unsafe { Cx::from_raw(&raw) };
            assert!(matches!(cx.kind(root), NodeKind::Itblock { .. }));
            assert!(cx.is_any_block_type(root), "itblock is an any_block type");
        }
        with_parsed("foo", |cx, root| assert!(!cx.is_any_block_type(root)));
        // any_def_type? = def/defs.
        with_parsed("def f; end", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Def { .. }));
            assert!(cx.is_any_def_type(root));
        });
        // Defs (`def self.f`) — the translator currently lowers singleton
        // defs to a `Def` with a self receiver, so the Defs arm is pinned
        // against a hand-built fixture.
        {
            let mut b = AstBuilder::new("def self.f; end", "t.rb".to_string());
            let recv = b.push(NodeKind::SelfExpr, Range { start: 4, end: 8 });
            let name = b.intern_symbol("f");
            let args = b.push(
                NodeKind::Args(murphy_ast::NodeList::EMPTY),
                Range { start: 9, end: 10 },
            );
            let root = b.push(
                NodeKind::Defs {
                    receiver: recv,
                    name,
                    args,
                    body: OptNodeId::NONE,
                },
                Range { start: 0, end: 15 },
            );
            let ast = b.finish(root);
            let fns = FnTable {
                emit_offense: noop_offense,
                emit_edit: noop_edit,
            };
            let raw = cx_raw_for(&ast, &fns);
            let cx = unsafe { Cx::from_raw(&raw) };
            assert!(matches!(cx.kind(root), NodeKind::Defs { .. }));
            assert!(cx.is_any_def_type(root));
        }
        with_parsed("foo", |cx, root| assert!(!cx.is_any_def_type(root)));
    }

    #[test]
    fn variable_reads() {
        // VARIABLES = ivar gvar cvar lvar.
        with_parsed("@i", |cx, root| assert!(cx.is_variable(root)));
        with_parsed("$g", |cx, root| assert!(cx.is_variable(root)));
        with_parsed("@@c", |cx, root| assert!(cx.is_variable(root)));
        // A bare local read only parses as lvar after an assignment is seen.
        with_parsed("x = 1\nx", |cx, root| {
            let stmts = cx.children(root);
            let last = *stmts.last().unwrap();
            assert!(matches!(cx.kind(last), NodeKind::Lvar(..)));
            assert!(cx.is_variable(last));
        });
        // A constant read is not a "variable".
        with_parsed("Foo", |cx, root| assert!(!cx.is_variable(root)));
        // An assignment is a write, not a variable read.
        with_parsed("@i = 1", |cx, root| assert!(!cx.is_variable(root)));
    }

    #[test]
    fn assignment_predicates() {
        // EQUALS_ASSIGNMENTS.
        for src in [
            "a = 1",
            "@a = 1",
            "@@a = 1",
            "$a = 1",
            "A = 1",
            "a, b = 1, 2",
        ] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_equals_asgn(root), "{src} is an equals assignment");
                assert!(cx.is_assignment(root), "{src} is an assignment");
                assert!(!cx.is_shorthand_asgn(root), "{src} is not shorthand");
            });
        }
        // SHORTHAND_ASSIGNMENTS.
        for src in ["a += 1", "a ||= 1", "a &&= 1"] {
            with_parsed(src, |cx, root| {
                assert!(
                    cx.is_shorthand_asgn(root),
                    "{src} is a shorthand assignment"
                );
                assert!(cx.is_assignment(root), "{src} is an assignment");
                assert!(
                    !cx.is_equals_asgn(root),
                    "{src} is not an equals assignment"
                );
            });
        }
        // index_asgn is NOT in RuboCop's ASSIGNMENTS set. (The translator
        // currently lowers `a[0] = 1` to a `Send :[]=`, which is likewise
        // not an assignment — either way the predicate is false.)
        with_parsed("a[0] = 1", |cx, root| {
            assert!(
                !cx.is_assignment(root),
                "index assignment is not in ASSIGNMENTS"
            );
            assert!(!cx.is_equals_asgn(root));
        });
        // Hand-built IndexAsgn fixture pins the predicate against the actual
        // node kind too, independent of the translator's lowering choice.
        {
            let mut b = AstBuilder::new("a[0] = 1", "t.rb".to_string());
            let m = b.intern_symbol("a");
            let recv = b.push(
                NodeKind::Send {
                    receiver: OptNodeId::NONE,
                    method: m,
                    args: murphy_ast::NodeList::EMPTY,
                },
                Range { start: 0, end: 1 },
            );
            let idx = b.push(NodeKind::Int(0), Range { start: 2, end: 3 });
            let args = b.push_list(&[idx]);
            let val = b.push(NodeKind::Int(1), Range { start: 7, end: 8 });
            let root = b.push(
                NodeKind::IndexAsgn {
                    receiver: recv,
                    args,
                    value: val,
                },
                Range { start: 0, end: 8 },
            );
            let ast = b.finish(root);
            let fns = FnTable {
                emit_offense: noop_offense,
                emit_edit: noop_edit,
            };
            let raw = cx_raw_for(&ast, &fns);
            let cx = unsafe { Cx::from_raw(&raw) };
            assert!(matches!(cx.kind(root), NodeKind::IndexAsgn { .. }));
            assert!(!cx.is_assignment(root), "index_asgn is not in ASSIGNMENTS");
            assert!(!cx.is_equals_asgn(root));
        }
        with_parsed("foo", |cx, root| {
            assert!(!cx.is_assignment(root));
            assert!(!cx.is_equals_asgn(root));
            assert!(!cx.is_shorthand_asgn(root));
        });
    }

    #[test]
    fn chained_and_argument_predicates() {
        // chained? — node is the receiver of its parent call.
        with_parsed("a.b.c", |cx, root| {
            // root is `(a.b).c`; its receiver `a.b` is chained.
            let recv = cx.call_receiver(root).get().expect("a.b");
            assert!(cx.is_chained(recv), "a.b is the receiver of .c");
            assert!(!cx.is_chained(root), "the outermost call is not chained");
        });
        // chained? holds across csend (call_type? includes csend).
        with_parsed("a&.b.c", |cx, root| {
            let recv = cx.call_receiver(root).get().expect("a&.b");
            assert!(cx.is_chained(recv), "a&.b is the receiver of .c");
        });
        // argument? — node is an argument of its parent send.
        with_parsed("foo(bar, baz)", |cx, root| {
            let args = cx.call_arguments(root);
            assert_eq!(args.len(), 2);
            assert!(cx.is_argument(args[0]), "bar is an argument");
            assert!(cx.is_argument(args[1]), "baz is an argument");
            // The receiver position is not an argument; the call itself isn't.
            assert!(!cx.is_argument(root));
        });
        // argument? is send-only: a csend argument is still an argument
        // (its parent is the csend), but the receiver of a csend is NOT an
        // argument. Pin the asymmetry: chained holds, argument does not.
        with_parsed("a&.b", |cx, root| {
            let recv = cx.call_receiver(root).get().expect("a");
            assert!(cx.is_chained(recv), "a is the receiver of a&.b");
            assert!(!cx.is_argument(recv), "a csend receiver is not an argument");
        });
    }

    #[test]
    fn source_length_counts_characters() {
        with_parsed("foo", |cx, root| assert_eq!(cx.source_length(root), 3));
        // Multi-byte: a 3-char string literal "あい" spans 8 source bytes
        // ("あい" = 6 bytes + 2 quote bytes) but 4 characters.
        with_parsed("\"\u{3042}\u{3044}\"", |cx, root| {
            assert_eq!(cx.raw_source(cx.range(root)), "\"\u{3042}\u{3044}\"");
            assert_eq!(
                cx.source_length(root),
                4,
                "2 chars + 2 quotes = 4 characters"
            );
        });
        with_parsed("1 + 2", |cx, root| assert_eq!(cx.source_length(root), 5));
    }

    #[test]
    fn const_name_qualifies_through_scope() {
        with_parsed("Foo", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Const { .. }));
            assert_eq!(cx.const_name(root).as_deref(), Some("Foo"));
        });
        with_parsed("A::B", |cx, root| {
            assert_eq!(cx.const_name(root).as_deref(), Some("A::B"));
        });
        with_parsed("A::B::C", |cx, root| {
            assert_eq!(cx.const_name(root).as_deref(), Some("A::B::C"));
        });
        // Murphy divergence: ::Foo folds to scope=None, so const_name drops
        // the leading `::` — matching RuboCop's output for cbase paths.
        with_parsed("::Foo", |cx, root| {
            assert_eq!(cx.const_name(root).as_deref(), Some("Foo"));
        });
        // casgn also yields a const_name.
        with_parsed("A::B = 1", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Casgn { .. }));
            assert_eq!(cx.const_name(root).as_deref(), Some("A::B"));
        });
        // Non-const → None.
        with_parsed("foo", |cx, root| assert_eq!(cx.const_name(root), None));
    }

    #[test]
    fn double_colon_detects_path_call_operator() {
        with_parsed("Foo::bar", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Send { .. }));
            assert!(
                cx.is_double_colon(root),
                "Foo::bar uses :: as call operator"
            );
        });
        // Plain dot is not double-colon.
        with_parsed("Foo.bar", |cx, root| {
            assert!(!cx.is_double_colon(root));
        });
        // Safe-navigation is not double-colon.
        with_parsed("a&.b", |cx, root| assert!(!cx.is_double_colon(root)));
        // Operator send: the gap is whitespace, not `::`.
        with_parsed("a + b", |cx, root| assert!(!cx.is_double_colon(root)));
        // Receiverless call has no operator gap.
        with_parsed("foo", |cx, root| assert!(!cx.is_double_colon(root)));
        // A constant path read (not a call) is not a double-colon *call*.
        with_parsed("Foo::Bar", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Const { .. }));
            assert!(
                !cx.is_double_colon(root),
                "Foo::Bar is a const read, not a call"
            );
        });
    }

    #[test]
    fn arithmetic_operation_matches_binary_operators() {
        for src in ["a + b", "a - b", "a * b", "a / b", "a % b", "a ** b"] {
            with_parsed(src, |cx, root| {
                assert!(cx.is_arithmetic_operation(root), "{src} is arithmetic");
            });
        }
        // Non-arithmetic operators / methods are excluded.
        with_parsed("a == b", |cx, root| {
            assert!(!cx.is_arithmetic_operation(root))
        });
        with_parsed("a << b", |cx, root| {
            assert!(!cx.is_arithmetic_operation(root))
        });
        with_parsed("a.foo", |cx, root| {
            assert!(!cx.is_arithmetic_operation(root))
        });
        // Unary minus parses as `-@`, not the binary `-`.
        with_parsed("-a", |cx, root| assert!(!cx.is_arithmetic_operation(root)));
        // A non-call node is not an arithmetic operation.
        with_parsed("1", |cx, root| assert!(!cx.is_arithmetic_operation(root)));
    }

    /// Find the first descendant (or `root` itself) whose kind matches
    /// `pred`, in DFS pre-order.
    fn find_node(cx: &Cx<'_>, root: NodeId, pred: impl Fn(&NodeKind) -> bool) -> NodeId {
        if pred(cx.kind(root)) {
            return root;
        }
        cx.descendants(root)
            .into_iter()
            .find(|&n| pred(cx.kind(n)))
            .expect("no matching node")
    }

    #[test]
    fn sibling_index_send_arg_skips_receiver_and_selector_slots() {
        // `foo(1)` — receiver is nil (slot 0), selector `:foo` is the phantom
        // slot 1, so the lone `1` argument lands at parser-gem index 2.
        with_parsed("foo(1)", |cx, root| {
            let send = find_node(cx, root, |k| matches!(k, NodeKind::Send { .. }));
            let NodeKind::Send { args, .. } = *cx.kind(send) else {
                unreachable!()
            };
            let arg = cx.list(args)[0];
            assert!(matches!(cx.kind(arg), NodeKind::Int(_)));
            assert_eq!(cx.sibling_index(arg), Some(2));
            // slot 1 is the `:foo` selector phantom → no node left sibling.
            assert_eq!(cx.left_sibling(arg), OptNodeId::NONE);
            assert_eq!(cx.right_sibling(arg), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_two_args_are_adjacent_after_phantom() {
        // `foo(a, b)` — args at indices 2 and 3; `a`.left == None (selector),
        // `b`.left == `a`, `a`.right == `b`.
        with_parsed("foo(a, b)", |cx, root| {
            let send = find_node(cx, root, |k| matches!(k, NodeKind::Send { .. }));
            let NodeKind::Send { args, .. } = *cx.kind(send) else {
                unreachable!()
            };
            let list = cx.list(args).to_vec();
            let (a, b) = (list[0], list[1]);
            assert_eq!(cx.sibling_index(a), Some(2));
            assert_eq!(cx.sibling_index(b), Some(3));
            assert_eq!(cx.left_sibling(a), OptNodeId::NONE);
            assert_eq!(cx.left_sibling(b), OptNodeId::some(a));
            assert_eq!(cx.right_sibling(a), OptNodeId::some(b));
            assert_eq!(cx.right_sibling(b), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_method_receiver_at_slot_zero() {
        // `recv.foo(arg)` — the receiver is slot 0, the selector phantom is
        // slot 1, the argument slot 2.
        with_parsed("recv.foo(arg)", |cx, root| {
            let send = find_node(cx, root, |k| matches!(k, NodeKind::Send { .. }));
            let NodeKind::Send { receiver, args, .. } = *cx.kind(send) else {
                unreachable!()
            };
            let recv = receiver.get().expect("explicit receiver");
            let arg = cx.list(args)[0];
            assert_eq!(cx.sibling_index(recv), Some(0));
            assert_eq!(cx.sibling_index(arg), Some(2));
            // The receiver's right neighbour is the selector phantom → None.
            assert_eq!(cx.right_sibling(recv), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_casgn_value_at_slot_two() {
        // `X = 1` — scope is nil (slot 0), `:X` phantom (slot 1), value slot 2.
        with_parsed("X = 1", |cx, root| {
            let casgn = find_node(cx, root, |k| matches!(k, NodeKind::Casgn { .. }));
            let NodeKind::Casgn { value, .. } = *cx.kind(casgn) else {
                unreachable!()
            };
            let v = value.get().expect("rhs present");
            assert_eq!(cx.sibling_index(v), Some(2));
            assert_eq!(cx.left_sibling(v), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_op_asgn_value_after_operator_phantom() {
        // `a += b` — target slot 0, `:+` operator phantom slot 1, value slot 2.
        with_parsed("a += b", |cx, root| {
            let op = find_node(cx, root, |k| matches!(k, NodeKind::OpAsgn { .. }));
            let NodeKind::OpAsgn { target, value, .. } = *cx.kind(op) else {
                unreachable!()
            };
            assert_eq!(cx.sibling_index(target), Some(0));
            assert_eq!(cx.sibling_index(value), Some(2));
            assert_eq!(cx.left_sibling(value), OptNodeId::NONE);
            assert_eq!(cx.right_sibling(target), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_def_without_receiver_body_at_slot_one() {
        // `def m; x; end` — parser `def`: [:name phantom, args(0->1), body(2)].
        with_parsed("def m\n  x\nend", |cx, root| {
            let def = find_node(cx, root, |k| matches!(k, NodeKind::Def { .. }));
            let NodeKind::Def { args, body, .. } = *cx.kind(def) else {
                unreachable!()
            };
            assert_eq!(cx.sibling_index(args), Some(1));
            let b = body.get().expect("body present");
            assert_eq!(cx.sibling_index(b), Some(2));
            // args left neighbour is the `:m` name phantom → None.
            assert_eq!(cx.left_sibling(args), OptNodeId::NONE);
            assert_eq!(cx.right_sibling(args), OptNodeId::some(b));
        });
    }

    #[test]
    fn sibling_index_def_with_receiver_is_parser_defs_layout() {
        // `def self.foo(a); x; end` — parser `defs`:
        // [definee(0), :name phantom(1), args(2), body(3)].
        with_parsed("def self.foo(a)\n  x\nend", |cx, root| {
            let def = find_node(cx, root, |k| matches!(k, NodeKind::Def { .. }));
            let NodeKind::Def {
                receiver,
                args,
                body,
                ..
            } = *cx.kind(def)
            else {
                unreachable!()
            };
            let definee = receiver.get().expect("singleton receiver");
            assert_eq!(cx.sibling_index(definee), Some(0));
            assert_eq!(cx.sibling_index(args), Some(2));
            let b = body.get().expect("body present");
            assert_eq!(cx.sibling_index(b), Some(3));
            assert_eq!(cx.left_sibling(args), OptNodeId::NONE);
            assert_eq!(cx.right_sibling(definee), OptNodeId::NONE);
        });
    }

    #[test]
    fn sibling_index_if_condition_at_slot_zero() {
        // Guards the `is_value_used` invariant: the `if` condition is slot 0.
        with_parsed("if cond\n  body\nend", |cx, root| {
            let if_node = find_node(cx, root, |k| matches!(k, NodeKind::If { .. }));
            let NodeKind::If { cond, .. } = *cx.kind(if_node) else {
                unreachable!()
            };
            assert_eq!(cx.sibling_index(cond), Some(0));
        });
    }

    #[test]
    fn sibling_index_for_body_at_slot_two() {
        // Guards the `is_value_used` invariant: `for` body is slot 2 (no phantom).
        with_parsed("for v in items\n  body\nend", |cx, root| {
            let for_node = find_node(cx, root, |k| matches!(k, NodeKind::For { .. }));
            let NodeKind::For { var, body, .. } = *cx.kind(for_node) else {
                unreachable!()
            };
            assert_eq!(cx.sibling_index(var), Some(0));
            let b = body.get().expect("body present");
            assert_eq!(cx.sibling_index(b), Some(2));
        });
    }

    #[test]
    fn sibling_index_root_has_no_siblings() {
        with_parsed("x", |cx, root| {
            assert_eq!(cx.sibling_index(root), None);
            assert_eq!(cx.left_sibling(root), OptNodeId::NONE);
            assert_eq!(cx.right_sibling(root), OptNodeId::NONE);
        });
    }

    #[test]
    fn case_match_and_in_pattern_accessors_on_real_parse() {
        // `case x; in [a]; a; end` — the translator now emits CaseMatch /
        // InPattern (previously Unknown), so the accessors project real
        // children.
        with_parsed("case x\nin [a]\n  a\nend\n", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::CaseMatch { .. }));
            // subject is always present for a case/in.
            assert!(cx.case_match_subject(root).get().is_some());
            let branches = cx.in_pattern_branches(root);
            assert_eq!(branches.len(), 1, "one in-clause");
            let in_branch = branches[0];
            assert!(matches!(cx.kind(in_branch), NodeKind::InPattern { .. }));
            // pattern is an ArrayPattern; no guard; body present.
            let pat = cx.in_pattern_pattern(in_branch).get().unwrap();
            assert!(matches!(cx.kind(pat), NodeKind::ArrayPattern(..)));
            assert!(cx.in_pattern_guard(in_branch).get().is_none());
            assert!(cx.in_pattern_body(in_branch).get().is_some());
            // No `else`.
            assert!(cx.case_match_else_branch(root).get().is_none());
            // Wrong-node projections are NONE / empty.
            assert!(cx.case_match_subject(pat).get().is_none());
            assert!(cx.in_pattern_branches(pat).is_empty());
            assert!(cx.in_pattern_pattern(root).get().is_none());
        });
    }

    #[test]
    fn in_pattern_guard_and_else_accessors_on_real_parse() {
        // Guard is hoisted into the dedicated guard slot; `else` lands in the
        // case_match else branch.
        with_parsed("case x\nin [a] if a\n  a\nelse\n  0\nend\n", |cx, root| {
            assert!(cx.case_match_else_branch(root).get().is_some());
            let in_branch = cx.in_pattern_branches(root)[0];
            assert!(
                cx.in_pattern_guard(in_branch).get().is_some(),
                "guard expression must be present"
            );
        });
    }

    #[test]
    fn loop_inverse_keyword_dispatches_on_while_until() {
        // `while` ⇄ `until`, both in keyword and modifier (post) form. The
        // modifier form has no `loc.keyword()`, so this must be NodeKind
        // dispatch, not token text.
        with_parsed("while x\n  y\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::While { post: false, .. }));
            assert_eq!(cx.loop_inverse_keyword(root), "until");
        });
        with_parsed("until x\n  y\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Until { post: false, .. }));
            assert_eq!(cx.loop_inverse_keyword(root), "while");
        });
        with_parsed("begin; x; end while y", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::While { post: true, .. }));
            assert_eq!(cx.loop_inverse_keyword(root), "until");
        });
        with_parsed("begin; x; end until y", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Until { post: true, .. }));
            assert_eq!(cx.loop_inverse_keyword(root), "while");
        });
        // `for` is a loop but RuboCop has no inverse for it.
        with_parsed("for i in a\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::For { .. }));
            assert_eq!(cx.loop_inverse_keyword(root), "");
        });
        // Non-loop nodes return empty.
        with_parsed("if x\nend", |cx, root| {
            assert_eq!(cx.loop_inverse_keyword(root), "");
        });
        with_parsed("1", |cx, root| {
            assert_eq!(cx.loop_inverse_keyword(root), "")
        });
    }

    #[test]
    fn void_context_on_def_for_block_ensure() {
        // DefNode: (def_type? && method?(:initialize)) || assignment_method?.
        // Instance `def initialize` — def_type? true, name initialize → void.
        with_parsed("def initialize\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Def { .. }));
            assert!(cx.is_void_context(root));
        });
        // Singleton `def self.initialize` folds to Def with a receiver →
        // def_type? false → the initialize clause does not fire, and
        // `initialize` is not an assignment method → not void.
        with_parsed("def self.initialize\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Def { .. }));
            assert!(!cx.is_void_context(root));
        });
        // assignment_method? clause: a setter is void regardless of receiver.
        with_parsed("def foo=(v)\nend", |cx, root| {
            assert!(cx.is_void_context(root));
        });
        with_parsed("def self.foo=(v)\nend", |cx, root| {
            assert!(cx.is_void_context(root));
        });
        // Ordinary instance method — not void.
        with_parsed("def foo\nend", |cx, root| {
            assert!(!cx.is_void_context(root));
        });
        // A comparison operator def ends with `=` but is NOT an assignment
        // method (assignment_method? = !comparison_method? && ends_with `=`),
        // so it is not a void context. Pins the load-bearing exclusion.
        with_parsed("def ==(o)\nend", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Def { .. }));
            assert!(!cx.is_void_context(root), "`def ==` is not assignment/void");
        });
        // ForNode: always void.
        with_parsed("for i in a\nend", |cx, root| {
            assert!(cx.is_void_context(root));
        });
        // BlockNode: VOID_CONTEXT_METHODS = each/tap (also numblock/itblock).
        with_parsed("[1].each { |x| x }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Block { .. }));
            assert!(cx.is_void_context(root));
        });
        with_parsed("foo.tap { |x| x }", |cx, root| {
            assert!(cx.is_void_context(root));
        });
        with_parsed("foo.map { |x| x }", |cx, root| {
            assert!(
                !cx.is_void_context(root),
                "map is not a void-context method"
            );
        });
        with_parsed("[1].each { _1 }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Numblock { .. }));
            assert!(cx.is_void_context(root), "numblock each is void");
        });
        with_parsed("[1].each { it }", |cx, root| {
            assert!(matches!(cx.kind(root), NodeKind::Itblock { .. }));
            assert!(cx.is_void_context(root), "itblock each is void");
        });
        with_parsed("foo.map { _1 }", |cx, root| {
            assert!(!cx.is_void_context(root));
        });
        // EnsureNode: always void. The `ensure` node is nested inside `begin`.
        with_parsed("begin\n  a\nensure\n  b\nend", |cx, root| {
            let ens = find_node(cx, root, |k| matches!(k, NodeKind::Ensure { .. }));
            assert!(cx.is_void_context(ens));
        });
        // A plain expression is not a void context.
        with_parsed("1", |cx, root| assert!(!cx.is_void_context(root)));
    }
}
