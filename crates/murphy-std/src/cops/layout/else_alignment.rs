//! `Layout/ElseAlignment` — align `else`/`elsif` keywords with the opening
//! `if`/`unless` keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ElseAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-xc7b]
//! notes: >
//!   Ports the `on_if` path: for an `if`/`unless` chain, the base column is the
//!   opening `if`/`unless` keyword's column. Every `else`/`elsif` keyword in the
//!   chain that begins its own line must share that column; a mismatch emits
//!   "Align `<else>` with `<base>`." and autocorrects by rewriting the leading
//!   whitespace to the base column.
//!
//!   The elsif/else chain is walked downward from the chain head. Because cops
//!   are stateless (no `ignored_node?` bookkeeping), nested `elsif` `If` nodes
//!   are skipped when visited directly (their `expression.start` lands on the
//!   `elsif` keyword) — they are handled while walking down from the head,
//!   which avoids RuboCop's double-processing without per-cop state.
//!
//!   `unless` is branch-swapped in Murphy's AST (translate.rs): the else-clause
//!   body lives in `then_` and the unless body in `else_`. The `else`/`elsif`
//!   keyword is located positionally (between the two branch bodies in source
//!   order), so the swap is handled transparently. Ternaries are `If` nodes but
//!   have no own-line `else` token, so they are naturally excluded.
//!
//!   Documented gaps (filed as murphy-xc7b):
//!     - `on_rescue` / `on_case` / `on_case_match` else-alignment are not yet
//!       implemented (they need the rescue/when/in keyword locations and the
//!       method-definition / kwbegin base computation).
//!     - The `check_assignment` variable-alignment base (RuboCop's
//!       `EnforcedStyleAlignWith: variable`) is not modelled; under the default
//!       `Layout/EndAlignment` style (`keyword`) this does not change behavior.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct ElseAlignment;

#[cop(
    name = "Layout/ElseAlignment",
    description = "Align elses and elsifs correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ElseAlignment {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip nested `elsif` If nodes — they are processed while walking down
        // from the chain head. Their `expression.start` lands on `elsif`.
        let keyword = leading_keyword(node, cx);
        if keyword == Keyword::Elsif || keyword == Keyword::Modifier {
            return;
        }

        // Base column = the opening `if`/`unless` keyword's column.
        let base_start = cx.range(node).start;
        let base_column = column_of(base_start, cx);
        let base_keyword_src = keyword.as_str();

        // Walk the else/elsif chain downward, holding the base column constant.
        let mut current = node;
        loop {
            // Find this If's own `else`/`elsif` keyword (between its two branch
            // bodies in source order).
            let Some(else_tok) = else_keyword_range(current, cx) else {
                return;
            };
            if begins_its_line(else_tok.start, cx) {
                let else_column = column_of(else_tok.start, cx);
                if else_column != base_column {
                    let else_src = cx.raw_source(else_tok);
                    let message = format!("Align `{else_src}` with `{base_keyword_src}`.");
                    cx.emit_offense(else_tok, &message, None);
                    emit_realign(else_tok.start, base_column, cx);
                }
            }

            // If the else branch is itself an If (`elsif`), continue the chain.
            match elsif_branch(current, cx) {
                Some(next) => current = next,
                None => return,
            }
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Keyword {
    If,
    Unless,
    Elsif,
    /// Modifier-form `x if cond` / ternary — no own-line keyword chain.
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

/// Classify the leading keyword of an `If` node from the source byte at its
/// start. Modifier-form ifs and ternaries do not start with a keyword.
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

/// True if the byte at `idx` continues an identifier (so `iffy` is not `if`).
fn is_ident_continuation(src: &str, idx: usize) -> bool {
    src.as_bytes()
        .get(idx)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The `else_` field's node if it is a nested `If` (an `elsif` clause).
fn elsif_branch(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::If { else_, .. } = *cx.kind(node) else {
        return None;
    };
    let else_id = else_.get()?;
    // For `unless`, `else_` holds the unless body (never an elsif), so a nested
    // If there would not be an `elsif` keyword and is excluded by the
    // positional `else_keyword_range` scan anyway. Only treat it as a chain
    // continuation when its source actually starts with `elsif`.
    if matches!(*cx.kind(else_id), NodeKind::If { .. })
        && cx.raw_source(cx.range(else_id)).starts_with("elsif")
    {
        Some(else_id)
    } else {
        None
    }
}

/// The source range of this `If` node's own `else`/`elsif` keyword, located
/// positionally between its two branch bodies. Returns `None` for an `If` with
/// no else branch, a ternary, or a same-line else (which `begins_its_line`
/// would reject anyway, but is excluded here for clarity).
fn else_keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let NodeKind::If {
        cond,
        then_,
        else_,
    } = *cx.kind(node)
    else {
        return None;
    };
    let else_id = else_.get()?;

    // `elsif` case: the nested If's `expression.start` lands exactly on the
    // `elsif` keyword (parser-gem convention, see translate.rs). The keyword
    // token therefore begins at `else_id`'s start — return it directly. (The
    // positional gap scan below would miss it, since the token starts at the
    // gap's upper bound, not strictly inside it.)
    if matches!(*cx.kind(else_id), NodeKind::If { .. }) {
        let start = cx.range(else_id).start;
        let tok = cx.token_after(start)?;
        if tok.range.start == start && cx.raw_source(tok.range) == "elsif" {
            return Some(tok.range);
        }
        // A nested If in `else_` that is not an `elsif` (e.g. `unless`'s
        // swapped body) — fall through to the positional `else`-token scan.
    }

    // `else` case: the keyword sits strictly between the two branch bodies in
    // source order — independent of which AST field (`then_`/`else_`) each holds
    // (handles the `unless` branch swap). When `then_` is absent (empty
    // then-clause), the gap begins at the condition's end.
    let (lower, upper) = match then_.get() {
        Some(t) => {
            let (a, b) = (cx.range(t), cx.range(else_id));
            // Order the two bodies by source position.
            if a.start <= b.start {
                (a.end, b.start)
            } else {
                (b.end, a.start)
            }
        }
        None => (cx.range(cond).end, cx.range(else_id).start),
    };
    // Guard against degenerate ordering.
    if lower >= upper {
        return None;
    }

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < lower);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < upper)
        .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "else")
        .map(|t| t.range)
}

/// Column (in characters) of the byte at `offset` from the start of its line.
fn column_of(offset: u32, cx: &Cx<'_>) -> usize {
    let offset = offset as usize;
    let src = cx.source();
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..offset].chars().count()
}

/// True if only spaces/tabs precede `offset` on its line.
fn begins_its_line(offset: u32, cx: &Cx<'_>) -> bool {
    let offset = offset as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    src[line_start..offset].iter().all(|&b| b == b' ' || b == b'\t')
}

/// Rewrite the leading whitespace of the keyword's line to `base_column`
/// spaces. The keyword is guaranteed to begin its line, so this is a single
/// surgical edit that never touches string/heredoc interiors.
fn emit_realign(keyword_start: u32, base_column: usize, cx: &Cx<'_>) {
    let offset = keyword_start as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let range = Range {
        start: line_start as u32,
        end: keyword_start,
    };
    cx.emit_edit(range, &" ".repeat(base_column));
}

murphy_plugin_api::submit_cop!(ElseAlignment);

#[cfg(test)]
mod tests {
    use super::ElseAlignment;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        // Apply edits right-to-left so offsets stay valid.
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

    // ── Clean ────────────────────────────────────────────────────────────────

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

    // ── Offenses ─────────────────────────────────────────────────────────────

    #[test]
    fn flags_misaligned_else() {
        // `else` indented one space — exactly one offense.
        let src = "if something\n  code\n else\n  code\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Align `else` with `if`.");
    }

    #[test]
    fn flags_misaligned_elsif_once_not_twice() {
        // Regression: the nested `elsif` If node must not double-fire.
        let src = "if something\n  code\n elsif other\n  code\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert_eq!(offenses.len(), 1, "elsif must fire exactly once: {offenses:?}");
        assert_eq!(offenses[0].message, "Align `elsif` with `if`.");
    }

    #[test]
    fn flags_misaligned_else_after_elsif() {
        // `elsif` aligned, `else` misaligned — one offense for the else.
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
        assert_eq!(
            apply(src, &run.edits),
            "if something\n  code\nelse\n  code\nend\n"
        );
    }

    #[test]
    fn corrects_misaligned_elsif() {
        let src = "if something\n  code\n elsif other\n  code\nend\n";
        let run = run_cop_with_edits::<ElseAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "if something\n  code\nelsif other\n  code\nend\n"
        );
    }

    #[test]
    fn flags_misaligned_else_in_nested_indented_if() {
        // Inner if/else, indented two spaces; the inner `else` must align with
        // the inner `if`, not column 0.
        let src = "def m\n  if a\n    x\n  else\n    y\n  end\nend\n";
        let offenses = run_cop::<ElseAlignment>(src);
        assert!(offenses.is_empty(), "aligned inner else: {offenses:?}");

        let bad = "def m\n  if a\n    x\n else\n    y\n  end\nend\n";
        let bad_offenses = run_cop::<ElseAlignment>(bad);
        assert_eq!(bad_offenses.len(), 1, "got {bad_offenses:?}");
    }
}
