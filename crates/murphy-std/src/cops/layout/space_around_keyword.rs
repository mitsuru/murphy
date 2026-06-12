//! `Layout/SpaceAroundKeyword` — checks the spacing around keywords, flagging
//! a missing space before or after a keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundKeyword
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-qeef
//! notes: >
//!   AST-gated like RuboCop: dispatches on `While`/`Until` (keyword form),
//!   prefix `If`/`Unless`, `Case`, `When`, `Defined`, `Block` (`do`/`end`),
//!   `Return`, `Break`, `Next`, `Yield`, `Super`/`Zsuper`, and the keyword
//!   forms of `And`/`Or`. The keyword token is located via
//!   `cx.loc(node).keyword()` (the token at the node's expression start);
//!   spacing is checked with RuboCop's character classes
//!   (`space_before_missing?` / `space_after_missing?`), including the
//!   `accept (`/`[` opening-delimiter exceptions
//!   (`break defined? next not rescue super yield` for `(`; `super yield`
//!   for `[`), the `super` namespace-operator `::` exception, and the
//!   safe-navigation `&.` exception.
//!
//!   CLOSED (murphy-qeef): the `when` keyword (`on_when`) and `defined?`
//!   keyword (`on_defined?`) are now handled — both sit at the node's
//!   expression start, so `cx.loc(node).keyword()` locates them without any
//!   new loc surface.
//!
//!   REMAINING GAPS (murphy-qeef): modifier forms (`x if y`, `x while y`)
//!   where the keyword sits mid-expression (not `keyword_bearing`, so
//!   `keyword()` returns `Range::ZERO`); the ternary `then` and other
//!   `if`-internal locations (`else`/`begin`/`end`/`then`);
//!   `rescue`/`ensure`/`begin`/`kwbegin`/`for`/`in`-pattern keywords;
//!   pre/postexe (`BEGIN`/`END`), and pattern-matching operators; and the
//!   `preceded_by_operator?` before-space exception. Those keyword locations
//!   require parser loc fields (`.keyword`/`.begin`/`.end`/`.else`) that
//!   Murphy's `NodeLoc` (only `expression` + `name`) does not expose, or
//!   AST-ancestor walks the single-surface ABI does not support. Because
//!   `preceded_by_operator?` is not ported, the before-space check may
//!   false-positive (and autocorrect) on a keyword nested directly in an
//!   operator expression (e.g. `-yield`) where RuboCop suppresses it; this is
//!   rare and tracked under the same gap issue.
//! ```
//!
//! ## Matched shapes
//!
//! - `something 'test'do |x|` — missing space before block `do`.
//! - `while(something)` — missing space after `while`.
//! - `something = 123if test` — `if`/`unless` prefix... (modifier `if` is a gap).
//! - `return(foo + bar)` — missing space after `return` (`return` is not in the
//!   accept-`(` set, so this is flagged, matching RuboCop's docstring).
//!
//! ## Autocorrect
//!
//! Inserts a single space before or after the offending keyword.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAroundKeyword;

#[cop(
    name = "Layout/SpaceAroundKeyword",
    description = "Flag missing space before or after a keyword.",
    default_severity = "warning",
    default_enabled = true,
)]
impl SpaceAroundKeyword {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::While { post, .. } = *cx.kind(node) else {
            return;
        };
        // `begin..end while c` (post / do-while) is a modifier form — gap.
        if post {
            return;
        }
        check_leading_keyword(cx, node, &["while", "until"]);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Until { post, .. } = *cx.kind(node) else {
            return;
        };
        if post {
            return;
        }
        check_leading_keyword(cx, node, &["while", "until"]);
    }

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Only the prefix form: the keyword (`if`/`unless`) must be the first
        // token of the node. Modifier `x if y` has the keyword mid-expression
        // and is a documented gap.
        check_leading_keyword(cx, node, &["if", "unless"]);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["case"]);
    }

    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["return"]);
    }

    #[on_node(kind = "break")]
    fn check_break(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["break"]);
    }

    #[on_node(kind = "next")]
    fn check_next(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["next"]);
    }

    #[on_node(kind = "yield")]
    fn check_yield(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["yield"]);
    }

    #[on_node(kind = "super")]
    fn check_super(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["super"]);
    }

    #[on_node(kind = "zsuper")]
    fn check_zsuper(&self, node: NodeId, cx: &Cx<'_>) {
        check_leading_keyword(cx, node, &["super"]);
    }

    #[on_node(kind = "defined")]
    fn check_defined(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop's `on_defined?` checks the `defined?` keyword. It is in
        // `ACCEPT_LEFT_PAREN`, so `defined?(x)` does not flag a missing
        // after-space. The keyword is at the node's expression start.
        check_leading_keyword(cx, node, &["defined?"]);
    }

    #[on_node(kind = "when")]
    fn check_when(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop's `on_when` checks the `when` keyword (at the node's
        // expression start). `when` is not in `ACCEPT_LEFT_PAREN`, so
        // `when(1)` flags a missing after-space, matching RuboCop.
        check_leading_keyword(cx, node, &["when"]);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        // Locate the `do` opener between the call end and the block body /
        // node end. `{`-blocks have no keyword. Then `end` is the node's last
        // token (only meaningful for a `do` block).
        let _ = body; // `do`/`end` checks do not depend on body presence.
        let node_range = cx.range(node);
        // The `call`'s range spans the whole block, so it cannot bound the
        // search. Search from the last argument's end (after the receiver /
        // method name and all args) so a string/symbol arg containing `do`
        // is not mistaken for the block opener — mirrors the established
        // `each_with_object_argument` block-opener pattern.
        let search_from = cx
            .call_arguments(call)
            .last()
            .map_or(cx.loc(call).name.end.max(cx.range(call).start), |&a| {
                cx.range(a).end
            });
        let toks = cx.sorted_tokens();
        let idx = toks.partition_point(|t| t.range.start < search_from);
        let do_tok = toks[idx..]
            .iter()
            .take_while(|t| t.range.start < node_range.end)
            .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "do");
        let Some(do_tok) = do_tok else {
            return; // brace block — no keyword to check.
        };
        check_keyword_range(cx, do_tok.range);

        // `end` terminator — RuboCop's `check_end` fires only the before-space
        // half (no after-space). The `end` keyword ends exactly at the node's
        // expression end, so `end_keyword()` is the precise range.
        let end_range = cx.loc(node).end_keyword();
        if end_range != Range::ZERO {
            check_keyword_before_only(cx, end_range);
        }
    }

    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        check_word_operator(cx, lhs, rhs, "and");
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        check_word_operator(cx, lhs, rhs, "or");
    }
}

/// Locate the leading keyword token (via `cx.loc(node).keyword()`, which is
/// the token at `expression.start` and returns `Range::ZERO` for modifier
/// forms) and run the full before/after spacing check. Returns silently when
/// the keyword text is not one of `keywords` (a guard against any keyword-bearing
/// node whose leading token differs from what this handler expects).
fn check_leading_keyword(cx: &Cx<'_>, node: NodeId, keywords: &[&str]) {
    let kw_range = cx.loc(node).keyword();
    if kw_range == Range::ZERO {
        return; // modifier form / no leading keyword token.
    }
    let kw = cx.raw_source(kw_range);
    if !keywords.contains(&kw) {
        return;
    }
    check_keyword_range(cx, kw_range);
}

/// Check the keyword form of `and` / `or` (operator `&&`/`||` forms are out of
/// scope for this cop — handled by `SpaceAroundOperators`). Only fires when the
/// gap between lhs and rhs literally contains the word operator.
fn check_word_operator(cx: &Cx<'_>, lhs: NodeId, rhs: NodeId, word: &str) {
    let gap = Range {
        start: cx.range(lhs).end,
        end: cx.range(rhs).start,
    };
    if gap.start >= gap.end {
        return;
    }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < gap.start);
    if let Some(op) = toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= gap.end)
        .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == word)
    {
        check_keyword_range(cx, op.range);
    }
}

/// RuboCop `check_keyword`: emit before- and after-space offenses for the
/// keyword at `range`.
fn check_keyword_range(cx: &Cx<'_>, range: Range) {
    let src = cx.source().as_bytes();
    if space_before_missing(src, range) {
        emit_before(cx, range);
    }
    if space_after_missing(cx, src, range) {
        emit_after(cx, range);
    }
}

/// RuboCop `check_end`: only the before-space half (used for `end`).
fn check_keyword_before_only(cx: &Cx<'_>, range: Range) {
    let src = cx.source().as_bytes();
    if space_before_missing(src, range) {
        emit_before(cx, range);
    }
}

fn emit_before(cx: &Cx<'_>, range: Range) {
    let kw = cx.raw_source(range);
    cx.emit_offense(range, &format!("Space before keyword `{kw}` is missing."), None);
    cx.emit_edit(
        Range {
            start: range.start,
            end: range.start,
        },
        " ",
    );
}

fn emit_after(cx: &Cx<'_>, range: Range) {
    let kw = cx.raw_source(range);
    cx.emit_offense(range, &format!("Space after keyword `{kw}` is missing."), None);
    cx.emit_edit(
        Range {
            start: range.end,
            end: range.end,
        },
        " ",
    );
}

/// RuboCop `space_before_missing?`: the char immediately before the keyword is
/// not whitespace / `(` / `|` / `{` / `[` / `;` / `,` / `*` / `=`.
fn space_before_missing(src: &[u8], range: Range) -> bool {
    let start = range.start as usize;
    if start == 0 {
        return false;
    }
    !matches!(
        src[start - 1],
        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b'|' | b'{' | b'[' | b';' | b',' | b'*' | b'='
    )
}

/// RuboCop `space_after_missing?`: the char immediately after the keyword is
/// not whitespace / `;` / `,` / `#` / `\` / `)` / `}` / `]` / `.`, with the
/// opening-delimiter, safe-navigation, and namespace-operator exceptions.
fn space_after_missing(cx: &Cx<'_>, src: &[u8], range: Range) -> bool {
    let pos = range.end as usize;
    if pos >= src.len() {
        return false;
    }
    let ch = src[pos];
    let kw = cx.raw_source(range);

    // `accepted_opening_delimiter?` — certain keywords may abut `(` or `[`.
    if accept_left_paren(kw) && ch == b'(' {
        return false;
    }
    if accept_left_bracket(kw) && ch == b'[' {
        return false;
    }
    // `safe_navigation_call?` — `&.` immediately after.
    if ch == b'&' && pos + 1 < src.len() && src[pos + 1] == b'.' {
        return false;
    }
    // `accept_namespace_operator?` — `super::Foo`.
    if kw == "super" && ch == b':' && pos + 1 < src.len() && src[pos + 1] == b':' {
        return false;
    }

    !matches!(
        ch,
        b' ' | b'\t' | b'\r' | b'\n' | b';' | b',' | b'#' | b'\\' | b')' | b'}' | b']' | b'.'
    )
}

/// RuboCop `ACCEPT_LEFT_PAREN`.
fn accept_left_paren(kw: &str) -> bool {
    matches!(
        kw,
        "break" | "defined?" | "next" | "not" | "rescue" | "super" | "yield"
    )
}

/// RuboCop `ACCEPT_LEFT_SQUARE_BRACKET`.
fn accept_left_bracket(kw: &str) -> bool {
    matches!(kw, "super" | "yield")
}

#[cfg(test)]
mod tests {
    use super::SpaceAroundKeyword;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---------- block `do` (the headline example) ----------

    #[test]
    fn flags_missing_space_before_block_do() {
        test::<SpaceAroundKeyword>()
            .expect_offense(indoc! {r#"
                something 'test'do |x|
                                ^^ Space before keyword `do` is missing.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    something 'test'do |x|
                                    ^^ Space before keyword `do` is missing.
                    end
                "#},
                "something 'test' do |x|\nend\n",
            );
    }

    #[test]
    fn accepts_well_spaced_block_do() {
        test::<SpaceAroundKeyword>().expect_no_offenses(indoc! {r#"
            something 'test' do |x|
            end
        "#});
    }

    #[test]
    fn ignores_brace_block() {
        test::<SpaceAroundKeyword>().expect_no_offenses("[].each { |x| x }\n");
    }

    // ---------- while / until ----------

    #[test]
    fn flags_missing_space_after_while() {
        test::<SpaceAroundKeyword>()
            .expect_offense(indoc! {r#"
                while(something)
                ^^^^^ Space after keyword `while` is missing.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    while(something)
                    ^^^^^ Space after keyword `while` is missing.
                    end
                "#},
                "while (something)\nend\n",
            );
    }

    #[test]
    fn accepts_well_spaced_while() {
        test::<SpaceAroundKeyword>().expect_no_offenses(indoc! {r#"
            while (something)
            end
        "#});
    }

    // ---------- if / unless prefix ----------

    #[test]
    fn flags_missing_space_after_if_prefix() {
        test::<SpaceAroundKeyword>().expect_offense(indoc! {r#"
            if(x)
            ^^ Space after keyword `if` is missing.
              y
            end
        "#});
    }

    #[test]
    fn modifier_if_is_a_gap_no_offense() {
        // `something = 123if test` — the keyword sits mid-expression (modifier
        // form); documented gap, no offense.
        test::<SpaceAroundKeyword>().expect_no_offenses("something = 123if test\n");
    }

    // ---------- return ----------

    #[test]
    fn flags_missing_space_after_return() {
        test::<SpaceAroundKeyword>()
            .expect_offense(indoc! {r#"
                return(foo + bar)
                ^^^^^^ Space after keyword `return` is missing.
            "#})
            .expect_correction(
                indoc! {r#"
                    return(foo + bar)
                    ^^^^^^ Space after keyword `return` is missing.
                "#},
                "return (foo + bar)\n",
            );
    }

    // ---------- accept-`(` exceptions ----------

    #[test]
    fn accepts_yield_paren() {
        // `yield` is in ACCEPT_LEFT_PAREN — `yield(x)` is fine.
        test::<SpaceAroundKeyword>().expect_no_offenses("def f; yield(x); end\n");
    }

    #[test]
    fn accepts_next_paren() {
        test::<SpaceAroundKeyword>().expect_no_offenses(indoc! {r#"
            [].each do
              next(1)
            end
        "#});
    }

    // ---------- and / or keyword forms ----------

    #[test]
    fn accepts_well_spaced_and_or() {
        test::<SpaceAroundKeyword>().expect_no_offenses(indoc! {r#"
            a and b
            c or d
        "#});
    }

    // ---------- case / when ----------

    #[test]
    fn accepts_well_spaced_case() {
        test::<SpaceAroundKeyword>().expect_no_offenses(indoc! {r#"
            case x
            when 1
              2
            end
        "#});
    }

    #[test]
    fn flags_missing_space_after_when() {
        // `when` is not in ACCEPT_LEFT_PAREN, so `when(1)` flags.
        test::<SpaceAroundKeyword>()
            .expect_offense(indoc! {r#"
                case x
                when(1)
                ^^^^ Space after keyword `when` is missing.
                  2
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    case x
                    when(1)
                    ^^^^ Space after keyword `when` is missing.
                      2
                    end
                "#},
                "case x\nwhen (1)\n  2\nend\n",
            );
    }

    // ---------- defined? ----------

    #[test]
    fn accepts_defined_with_paren() {
        // `defined?` is in ACCEPT_LEFT_PAREN — `defined?(x)` is fine.
        test::<SpaceAroundKeyword>().expect_no_offenses("defined?(x)\n");
    }

    #[test]
    fn accepts_defined_with_space() {
        test::<SpaceAroundKeyword>().expect_no_offenses("defined? x\n");
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAroundKeyword>().expect_no_corrections(indoc! {r#"
            something 'test' do |x|
              return (x)
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(SpaceAroundKeyword);
