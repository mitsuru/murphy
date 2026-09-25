//! `Layout/ElseAlignment` \u2014 align `else`/`elsif` keywords with their opening
//! keyword (`if`/`unless`, `begin`/`def`, `when`, `in`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ElseAlignment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_if`, `on_rescue`, `on_case` and `on_case_match` plus the
//!   `check_assignment` variable-alignment base.
//!   `on_if`: for an `if`/`unless` chain the base is the opening keyword, except
//!   under `EnforcedStyleAlignWith: variable` (or `start_of_line`, which RuboCop
//!   treats the same for this cop) when the `if` is the RHS of an assignment /
//!   command call on the same line \u2014 then the base is the assignment / command
//!   (`x = if`, `foo bar, if`). A line break before the keyword falls back to the
//!   keyword. Every `else`/`elsif` in the chain that begins its own line must share
//!   the base column; mismatch emits "Align `<else>` with `<base>`." with a
//!   leading-whitespace rewrite.
//!   The elsif chain is walked downward from the head; nested `elsif` `If` nodes are
//!   skipped when visited directly (their start lands on `elsif`), avoiding
//!   double-processing without per-cop state. `unless` branch-swap and ternaries
//!   are handled positionally as before.
//!   `on_rescue`: base is `def`/`defs` (or its access-modifier selector, e.g.
//!   `private`), the explicit `begin` keyword, or for blocks the assignment when the
//!   block and assignment share a line else the block call (`foo`). The `else`
//!   keyword is located between the last resbody (or body) and the else body.
//!   `Ensure` wrappers and implicit single-child `Begin` body wrappers (Murphy lowers
//!   parser-gem `def`-rescue directly to `Def -> Begin -> Rescue`) are skipped when
//!   finding the effective parent. Unknown parents (e.g. `class`/`module` rescue,
//!   which crashes RuboCop 1.87.0) are skipped gracefully.
//!   `on_case`: base is the last `when` keyword. `on_case_match`: base is the last
//!   `in` keyword (recovered by token scan since `InPattern` is not keyword-bearing).
//!   Option divergence: RuboCop reads `Layout/EndAlignment: EnforcedStyleAlignWith` for
//!   the variable base; Murphy has no cross-cop config API (and the plugin ABI is
//!   frozen), so this cop carries its own `EnforcedStyleAlignWith` (default `keyword`).
//!   Set both cops to the same value for parity.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct ElseAlignment;

#[derive(CopOptions)]
pub struct ElseAlignmentOptions {
    #[option(
        name = "EnforcedStyleAlignWith",
        default = "keyword",
        description = "Whether `else` aligns with the keyword or, for `if` on the same line as an assignment, the assignment variable."
    )]
    pub enforced_style_align_with: AlignWith,
}

/// `SupportedStylesAlignWith: [keyword, variable, start_of_line]` (mirrors
/// `Layout/EndAlignment`; `variable` and `start_of_line` both mean variable
/// alignment for this cop, matching RuboCop).
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignWith {
    #[option(value = "keyword")]
    Keyword,
    #[option(value = "variable")]
    Variable,
    #[option(value = "start_of_line")]
    StartOfLine,
}

#[cop(
    name = "Layout/ElseAlignment",
    description = "Align elses and elsifs correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = ElseAlignmentOptions
)]
impl ElseAlignment {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let keyword = leading_keyword(node, cx);
        if keyword == Keyword::Elsif || keyword == Keyword::Modifier {
            return;
        }

        let base_start = cx.range(node).start;
        let opening_column = column_of(base_start, cx);
        let opening_word = keyword.as_str().to_owned();

        let style = cx.options_or_default::<ElseAlignmentOptions>().enforced_style_align_with;
        let (base_column, base_word) = if style != AlignWith::Keyword {
            match variable_assignment_base(node, cx) {
                Some(outer) if line_of(cx.range(outer).start, cx) == line_of(base_start, cx) => {
                    let r = cx.range(outer);
                    (column_of(r.start, cx), first_word(cx.raw_source(r)))
                }
                _ => (opening_column, opening_word),
            }
        } else {
            (opening_column, opening_word)
        };

        let mut current = node;
        loop {
            let Some(else_tok) = else_keyword_range(current, cx) else {
                return;
            };
            if begins_its_line(else_tok.start, cx) {
                let else_column = column_of(else_tok.start, cx);
                if else_column != base_column {
                    let else_src = cx.raw_source(else_tok);
                    let message = format!("Align `{else_src}` with `{base_word}`.");
                    cx.emit_offense(else_tok, &message, None);
                    emit_realign(else_tok.start, base_column, cx);
                }
            }

            match elsif_branch(current, cx) {
                Some(next) => current = next,
                None => return,
            }
        }
    }

    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Rescue { else_, .. } = *cx.kind(node) else {
            return;
        };
        if else_.get().is_none() {
            return;
        }
        let Some(else_tok) = rescue_else_range(node, cx) else {
            return;
        };
        if !begins_its_line(else_tok.start, cx) {
            return;
        }
        let Some((base_column, base_word)) = rescue_base(node, cx) else {
            return;
        };
        if column_of(else_tok.start, cx) != base_column {
            let else_src = cx.raw_source(else_tok);
            let message = format!("Align `{else_src}` with `{base_word}`.");
            cx.emit_offense(else_tok, &message, None);
            emit_realign(else_tok.start, base_column, cx);
        }
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Case { whens, else_, .. } = *cx.kind(node) else {
            return;
        };
        let Some(else_body) = else_.get() else {
            return;
        };
        let whens_list = cx.list(whens);
        let Some(&last_when) = whens_list.last() else {
            return;
        };
        let Some(base_kw) = when_keyword(last_when, cx) else {
            return;
        };
        let Some(else_tok) = bounded_else_token(cx.range(last_when).end, cx.range(else_body).start, cx)
        else {
            return;
        };
        check_one_else(else_tok, base_kw, cx);
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::CaseMatch { in_patterns, else_body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(else_body_id) = else_body.get() else {
            return;
        };
        let pats = cx.list(in_patterns);
        let Some(&last_pat) = pats.last() else {
            return;
        };
        let Some(base_kw) = in_keyword(last_pat, cx) else {
            return;
        };
        let Some(else_tok) = bounded_else_token(cx.range(last_pat).end, cx.range(else_body_id).start, cx)
        else {
            return;
        };
        check_one_else(else_tok, base_kw, cx);
    }
}

fn check_one_else(else_tok: Range, base_kw: Range, cx: &Cx<'_>) {
    if !begins_its_line(else_tok.start, cx) {
        return;
    }
    let base_column = column_of(base_kw.start, cx);
    if column_of(else_tok.start, cx) == base_column {
        return;
    }
    let else_src = cx.raw_source(else_tok);
    let base_word = first_word(cx.raw_source(base_kw));
    let message = format!("Align `{else_src}` with `{base_word}`.");
    cx.emit_offense(else_tok, &message, None);
    emit_realign(else_tok.start, base_column, cx);
}


#[derive(PartialEq, Eq, Clone, Copy)]
enum Keyword {
    If,
    Unless,
    Elsif,
    /// Modifier-form `x if cond` / ternary.
    Modifier,
}

impl Keyword {
    fn as_str(self) -> &'static str {
        match self {
            Keyword::If => "if",
            Keyword::Unless => "unless",
            Keyword::Elsif => "elsif",
            Keyword::Modifier => "",
        }
    }
}

fn leading_keyword(node: NodeId, cx: &Cx<'_>) -> Keyword {
    let src = cx.raw_source(cx.range(node));
    if src.starts_with("if") && !is_ident_continuation(src, 2) {
        Keyword::If
    } else if src.starts_with("unless") && !is_ident_continuation(src, 6) {
        Keyword::Unless
    } else if src.starts_with("elsif") && !is_ident_continuation(src, 5) {
        Keyword::Elsif
    } else {
        Keyword::Modifier
    }
}

fn is_ident_continuation(src: &str, idx: usize) -> bool {
    src.as_bytes().get(idx).is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

fn elsif_branch(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::If { else_, .. } = *cx.kind(node) else { return None; };
    let else_id = else_.get()?;
    if matches!(*cx.kind(else_id), NodeKind::If { .. }) && cx.raw_source(cx.range(else_id)).starts_with("elsif") {
        Some(else_id)
    } else { None }
}

fn else_keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let NodeKind::If { cond, then_, else_, } = *cx.kind(node) else { return None; };
    let else_id = else_.get()?;
    if matches!(*cx.kind(else_id), NodeKind::If { .. }) {
        let start = cx.range(else_id).start;
        let tok = cx.token_after(start)?;
        if tok.range.start == start && cx.raw_source(tok.range) == "elsif" { return Some(tok.range); }
    }
    let (lower, upper) = match then_.get() {
        Some(t) => { let (a, b) = (cx.range(t), cx.range(else_id)); if a.start <= b.start { (a.end, b.start) } else { (b.end, a.start) } }
        None => (cx.range(cond).end, cx.range(else_id).start),
    };
    if lower >= upper { return None; }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < lower);
    toks[idx..].iter().take_while(|t| t.range.start < upper).find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "else").map(|t| t.range)
}

fn variable_assignment_base(if_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let mut current = if_node;
    loop {
        let parent = cx.parent(current).get()?;
        if cx.is_assignment(parent) {
            return (cx.children(parent).last() == Some(&current)).then_some(parent);
        }
        if matches!(*cx.kind(parent), NodeKind::Send { .. }) {
            if cx.call_arguments(parent).last() == Some(&current) { return Some(parent); }
            if cx.call_receiver(parent).get() == Some(current) { current = parent; continue; }
            return None;
        }
        if matches!(*cx.kind(parent), NodeKind::Csend { .. }) {
            if cx.call_receiver(parent).get() == Some(current) { current = parent; continue; }
            return None;
        }
        if matches!(*cx.kind(parent), NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }) {
            if cx.block_call(parent).get() == Some(current) { current = parent; continue; }
            return None;
        }
        return None;
    }
}

fn rescue_else_range(rescue: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let NodeKind::Rescue { body, resbodies, else_, } = *cx.kind(rescue) else { return None; };
    let else_body = else_.get()?;
    let else_start = cx.range(else_body).start;
    let lower = {
        let list = cx.list(resbodies);
        if let Some(&last) = list.last() { cx.range(last).end }
        else if let Some(b) = body.get() { cx.range(b).end }
        else { cx.range(rescue).start }
    };
    bounded_else_token(lower, else_start, cx)
}

fn bounded_else_token(lower: u32, upper: u32, cx: &Cx<'_>) -> Option<Range> {
    if lower >= upper { return None; }
    let toks = cx.sorted_tokens();
    let lo = toks.partition_point(|t| t.range.start < lower);
    let hi = toks.partition_point(|t| t.range.end <= upper);
    if lo >= hi { return None; }
    toks[lo..hi].iter().rev().find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "else").map(|t| t.range)
}

fn is_kwbegin(node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(*cx.kind(node), NodeKind::Kwbegin(_)) { return true; }
    if !matches!(*cx.kind(node), NodeKind::Begin(_)) { return false; }
    let start = cx.range(node).start;
    cx.token_after(start).is_some_and(|t| t.kind == SourceTokenKind::Other && t.range.start == start && cx.raw_source(t.range) == "begin")
}

fn rescue_base(node: NodeId, cx: &Cx<'_>) -> Option<(usize, String)> {
    let mut cur = cx.parent(node).get()?;
    if matches!(*cx.kind(cur), NodeKind::Ensure { .. }) { cur = cx.parent(cur).get()?; }
    loop {
        if matches!(*cx.kind(cur), NodeKind::Begin(_)) && !is_kwbegin(cur, cx) { cur = cx.parent(cur).get()?; } else { break; }
    }
    match *cx.kind(cur) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => {
            if let Some(p) = cx.parent(cur).get() {
                if cx.is_access_modifier(p) {
                    let sel = cx.selector(p);
                    if sel != Range::ZERO { return Some((column_of(sel.start, cx), first_word(cx.raw_source(sel)))); }
                }
                if matches!(*cx.kind(p), NodeKind::Send { .. }) && cx.call_receiver(p).get().is_none()
                    && let Some(m) = cx.method_name(p)
                        && matches!(m, "private_class_method" | "public_class_method") {
                            let sel = cx.selector(p);
                            if sel != Range::ZERO { return Some((column_of(sel.start, cx), first_word(cx.raw_source(sel)))); }
                        }
            }
            let kw = cx.loc(cur).keyword();
            if kw == Range::ZERO { return None; }
            if !cx.raw_source(kw).starts_with("def") { return None; }
            Some((column_of(kw.start, cx), first_word(cx.raw_source(kw))))
        }
        NodeKind::Begin(_) | NodeKind::Kwbegin(_) => {
            let kw = cx.loc(cur).keyword();
            if kw != Range::ZERO && cx.raw_source(kw) == "begin" { return Some((column_of(kw.start, cx), "begin".to_owned())); }
            let start = cx.range(cur).start;
            if let Some(t) = cx.token_after(start)
                && t.range.start == start && cx.raw_source(t.range) == "begin" { return Some((column_of(t.range.start, cx), "begin".to_owned())); }
            None
        }
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            if let Some(assign) = cx.parent(cur).get()
                && cx.is_assignment(assign) && line_of(cx.range(cur).start, cx) == line_of(cx.range(assign).start, cx) {
                    let r = cx.range(assign);
                    return Some((column_of(r.start, cx), first_word(cx.raw_source(r))));
                }
            let call = cx.block_call(cur).get()?;
            let r = cx.range(call);
            Some((column_of(r.start, cx), first_word(cx.raw_source(r))))
        }
        _ => None,
    }
}

fn when_keyword(when_node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let kw = cx.loc(when_node).keyword();
    if kw != Range::ZERO && cx.raw_source(kw) == "when" { return Some(kw); }
    let start = cx.range(when_node).start;
    cx.token_after(start).and_then(|t| (t.kind == SourceTokenKind::Other && t.range.start == start && cx.raw_source(t.range) == "when").then_some(t.range))
}

fn in_keyword(pat: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let start = cx.range(pat).start;
    cx.token_after(start).and_then(|t| (t.kind == SourceTokenKind::Other && t.range.start == start && cx.raw_source(t.range) == "in").then_some(t.range))
}

fn first_word(src: &str) -> String {
    let end = src.find(|c: char| c.is_whitespace()).unwrap_or(src.len());
    src[..end].to_owned()
}

fn line_of(offset: u32, cx: &Cx<'_>) -> usize {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    src[..upper].bytes().filter(|&b| b == b'\n').count() + 1
}

fn column_of(offset: u32, cx: &Cx<'_>) -> usize {
    let offset = offset as usize;
    let src = cx.source();
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..offset].chars().count()
}

fn begins_its_line(offset: u32, cx: &Cx<'_>) -> bool {
    let offset = offset as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..offset].iter().rposition(|&b| b == b'\n').map_or(0, |pos| pos + 1);
    src[line_start..offset].iter().all(|&b| b == b' ' || b == b'\t')
}

fn emit_realign(keyword_start: u32, base_column: usize, cx: &Cx<'_>) {
    let offset = keyword_start as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..offset].iter().rposition(|&b| b == b'\n').map_or(0, |pos| pos + 1);
    let range = Range { start: line_start as u32, end: keyword_start, };
    cx.emit_edit(range, &" ".repeat(base_column));
}

murphy_plugin_api::submit_cop!(ElseAlignment);

#[cfg(test)]
mod tests {
    use super::{AlignWith, ElseAlignment, ElseAlignmentOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut sorted: Vec<_> = edits.iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut out = source.to_string();
        for edit in sorted { out.replace_range(edit.range.start as usize..edit.range.end as usize, &edit.replacement); }
        out
    }

    fn variable() -> ElseAlignmentOptions { ElseAlignmentOptions { enforced_style_align_with: AlignWith::Variable } }

    #[test]
    fn accepts_aligned_if_else() {
        let offenses = run_cop::<ElseAlignment>(indoc! {r#"
            if something
              code
            else
              code
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn accepts_aligned_if_elsif_else() {
        let offenses = run_cop::<ElseAlignment>(indoc! {r#"
            if something
              code
            elsif other
              code
            else
              code
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn accepts_ternary() {
        let offenses = run_cop::<ElseAlignment>("x = a ? b : c\n");
        assert!(offenses.is_empty(), "ternary should not fire: {offenses:?}");
    }

    #[test]
    fn accepts_if_without_else() {
        let offenses = run_cop::<ElseAlignment>(indoc! {r#"
            if something
              code
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn accepts_aligned_unless_else() {
        let offenses = run_cop::<ElseAlignment>(indoc! {r#"
            unless something
              code
            else
              code
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn flags_misaligned_else() {
        let src = "if something\n  code\n else\n  code\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `if`.");
    }

    #[test]
    fn flags_misaligned_elsif_once_not_twice() {
        let src = "if something\n  code\n elsif other\n  code\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "elsif must fire exactly once: {offenses:?}");
        assert_eq!(offenses[0].message, "Align `elsif` with `if`.");
    }

    #[test]
    fn flags_misaligned_else_after_elsif() {
        let src = "if a\n  x\nelsif b\n  y\n else\n  z\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `if`.");
    }

    #[test]
    fn flags_both_elsif_and_else_misaligned() {
        let src = "if a\n  x\n elsif b\n  y\n  else\n  z\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn corrects_misaligned_else() {
        let src = "if something\n  code\n else\n  code\nend\n";
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "if something\n  code\nelse\n  code\nend\n");
    }

    #[test]
    fn corrects_misaligned_elsif() {
        let src = "if something\n  code\n elsif other\n  code\nend\n";
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "if something\n  code\nelsif other\n  code\nend\n");
    }

    #[test]
    fn flags_misaligned_else_in_nested_indented_if() {
        let src = "def m\n  if a\n    x\n  else\n    y\n  end\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert!(offenses.is_empty(), "aligned inner else: {offenses:?}");
        let bad = "def m\n  if a\n    x\n else\n    y\n  end\nend\n";
        let bad_offenses = run_cop::<ElseAlignment>(bad);
        assert_eq!(bad_offenses.len(), 1, "got {bad_offenses:?}");
    }
    #[test]
    fn accepts_aligned_rescue_else_with_begin() {
        let offenses = run_cop::<ElseAlignment>("begin\n  foo\nrescue\n  bar\nelse\n  baz\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_rescue_else_with_begin() {
        let src = "begin\n  foo\nrescue\n  bar\n  else\n  baz\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `begin`.");
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(apply(src, &run.edits), "begin\n  foo\nrescue\n  bar\nelse\n  baz\nend\n");
    }
    #[test]
    fn accepts_aligned_rescue_else_with_def() {
        let offenses = run_cop::<ElseAlignment>("def foo\n  bar\nrescue\n  baz\nelse\n  qux\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_rescue_else_with_def() {
        let src = "def foo\n  bar\nrescue\n  baz\n  else\n  qux\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `def`.");
    }
    #[test]
    fn flags_misaligned_rescue_else_with_private_def() {
        let src = "private def foo\n  bar\nrescue\n  baz\n  else\n  qux\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `private`.");
    }
    #[test]
    fn accepts_aligned_rescue_else_with_assignment_begin() {
        let aligned = "x = begin\n  foo\nrescue\n  bar\n    else\n  baz\nend\n";
        assert!(run_cop::<ElseAlignment>(aligned).is_empty(), "else at col 4 must align with begin");
    }
    #[test]
    fn flags_rescue_else_aligned_to_assignment_instead_of_begin() {
        let src = "x = begin\n  foo\nrescue\n  bar\nelse\n  baz\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `begin`.");
    }
    #[test]
    fn accepts_aligned_block_rescue_else_with_assignment() {
        let offenses = run_cop::<ElseAlignment>("x = foo do\n  a\nrescue\n  b\nelse\n  c\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_block_rescue_else_with_assignment() {
        let src = "x = foo do\n  a\nrescue\n  b\n  else\n  c\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `x`.");
    }
    #[test]
    fn flags_misaligned_block_rescue_else_without_assignment() {
        let src = "foo do\n  bar\nrescue\n  baz\n  else\n  qux\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `foo`.");
    }
    #[test]
    fn accepts_rescue_without_else() {
        let offenses = run_cop::<ElseAlignment>("begin\n  foo\nrescue\n  bar\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn accepts_modifier_rescue() {
        let offenses = run_cop::<ElseAlignment>("x = foo rescue bar\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn accepts_aligned_rescue_else_with_ensure() {
        let offenses = run_cop::<ElseAlignment>("begin\n  foo\nrescue\n  bar\nelse\n  baz\nensure\n  qux\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_rescue_else_with_ensure() {
        let src = "begin\n  foo\nrescue\n  bar\n  else\n  baz\nensure\n  qux\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `begin`.");
    }
    #[test]
    fn accepts_aligned_case_else() {
        let offenses = run_cop::<ElseAlignment>("case x\nwhen 1\n  a\nelse\n  b\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_case_else() {
        let src = "case x\nwhen 1\n  a\n  else\n  b\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `when`.");
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(apply(src, &run.edits), "case x\nwhen 1\n  a\nelse\n  b\nend\n");
    }
    #[test]
    fn flags_case_else_against_last_when() {
        let src = "case x\nwhen 1\n  a\nwhen 2\n  b\n  else\n  c\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `when`.");
    }
    #[test]
    fn accepts_case_without_else() {
        let offenses = run_cop::<ElseAlignment>("case x\nwhen 1\n  a\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn accepts_aligned_case_match_else() {
        let offenses = run_cop::<ElseAlignment>("case x\n  in 1\n    a\n  else\n    b\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }
    #[test]
    fn flags_misaligned_case_match_else() {
        let src = "case x\n  in 1\n    a\n    else\n    b\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `in`.");
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(apply(src, &run.edits), "case x\n  in 1\n    a\n  else\n    b\nend\n");
    }
    #[test]
    fn variable_style_aligns_else_with_assignment() {
        let src = "x = if a\n  b\nelse\n  c\nend\n";
        assert!(run_cop::<ElseAlignment>(src).is_empty() == false, "keyword style must flag col-0 else");
        let offenses = run_cop_with_options::<ElseAlignment>(src, &variable());
        assert!(offenses.is_empty(), "variable style: else at col 0 aligns with x: {offenses:?}");
        let bad = "x = if a\n  b\n  else\n  c\nend\n";
        let bad_off = run_cop_with_options::<ElseAlignment>(bad, &variable());
        assert_eq!(bad_off.len(), 1, "got {bad_off:?}");
        assert_eq!(bad_off[0].message, "Align `else` with `x`.");
        let run = run_cop_with_options_and_edits::<ElseAlignment>(bad, &variable());
        assert_eq!(apply(bad, &run.edits), "x = if a\n  b\nelse\n  c\nend\n");
    }
    #[test]
    fn variable_style_falls_back_on_line_break() {
        let src = "x =\n  if a\n    b\n  else\n    c\n  end\n";
        let offenses = run_cop_with_options::<ElseAlignment>(src, &variable());
        assert!(offenses.is_empty(), "line break before keyword falls back to keyword: {offenses:?}");
    }
    #[test]
    fn ignores_inline_else() {
        let offenses = run_cop::<ElseAlignment>("case x\nwhen 1 then a else b\nend\n");
        assert!(offenses.is_empty(), "inline else must not fire: {offenses:?}");
    }
}
