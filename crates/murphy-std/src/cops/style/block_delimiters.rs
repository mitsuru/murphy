//! `Style/BlockDelimiters` — enforces brace or do/end delimiters for blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BlockDelimiters
//! upstream_version_checked: 1.86.2
//! version_added: "0.30"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements EnforcedStyle: line_count_based (default), braces_for_chaining,
//!   and always_braces. The `semantic` style is not implemented — it requires
//!   return-value analysis (functional_block?, return_value_used?,
//!   return_value_of_scope?) which depends on parent-kind classification not
//!   readily available from block dispatch alone.
//!
//!   AllowedMethods (default: lambda, proc, it) and BracesRequiredMethods
//!   (default: []) are fully supported via Vec<String> options.
//!
//!   AllowedPatterns (regex) is not implemented — derive only covers Vec<String>.
//!   AllowBracesOnProceduralOneLiners is only relevant to the `semantic` style.
//!   ProceduralMethods and FunctionalMethods are only relevant to `semantic`.
//!
//!   Autocorrect is not implemented. The `{}` to `do...end` direction is
//!   generally safe, but `do...end` to `{}` changes operator precedence when
//!   the block call has unparenthesised arguments (RuboCop's
//!   `correction_would_break_code?`), and converting do-end blocks that
//!   contain rescue/ensure requires wrapping in `begin`/`end`. These rewrites
//!   need source-layout awareness beyond what Murphy's current emit_edit API
//!   can express safely.
//!
//!   Block binding: when a block is passed as an argument to a method call that
//!   is not parenthesised, changing `{...}` or `do...end` changes which method
//!   the block binds to (Ruby precedence). RuboCop's on_send/ignore_node handles
//!   this; Murphy replicates it via ancestor-walking in is_in_non_parenthesized_arg.
//! ```
//!
//! ## Matched shapes
//!
//! `block`, `numblock`, and `itblock` nodes, except those that:
//! - Are passed as arguments to a non-parenthesised call (binding ambiguity)
//! - Name the block's method in `AllowedMethods`
//! - Are covered by `BracesRequiredMethods` and already use braces
//!
//! ## Styles
//!
//! - `line_count_based` (default): single-line uses braces; multi-line uses do/end.
//! - `braces_for_chaining`: like `line_count_based`, but multi-line chained
//!   blocks prefer braces.
//! - `always_braces`: always prefer braces.
//! - `semantic`: not implemented (partial gap).
//!
//! ## Offense location
//!
//! The offense range is the block's opening delimiter token (`{` or `do`),
//! matching RuboCop's `add_offense(node.loc.begin, ...)`.
//!
//! ## No autocorrect
//!
//! Autocorrect is intentionally omitted due to operator-precedence and
//! begin/rescue wrapping hazards (see notes in parity block above).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Which block-delimiter style to enforce.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Single-line uses braces; multi-line uses do/end.
    #[default]
    #[option(value = "line_count_based")]
    LineCountBased,
    /// Always prefer braces.
    #[option(value = "always_braces")]
    AlwaysBraces,
    /// Single-line uses braces; multi-line uses do/end, except for chained blocks
    /// which prefer braces.
    #[option(value = "braces_for_chaining")]
    BracesForChaining,
    /// Semantic (functional vs procedural): not implemented.
    #[option(value = "semantic")]
    Semantic,
}

/// Options for `Style/BlockDelimiters`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "line_count_based",
        description = "Which block-delimiter style to enforce."
    )]
    pub enforced_style: EnforcedStyle,

    #[option(
        name = "AllowedMethods",
        default = ["lambda", "proc", "it"],
        description = "Block method names that are always allowed regardless of style."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "BracesRequiredMethods",
        default = [],
        description = "Block method names that always require brace delimiters."
    )]
    pub braces_required_methods: Vec<String>,
}

// ---------------------------------------------------------------------------
// Cop
// ---------------------------------------------------------------------------

/// Stateless unit struct.
#[derive(Default)]
pub struct BlockDelimiters;

#[cop(
    name = "Style/BlockDelimiters",
    description = "Enforces consistent block delimiter style (braces or do/end).",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl BlockDelimiters {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();

    // If this block is an argument to a non-parenthesised call, changing
    // delimiters would alter Ruby's block-binding precedence. Skip it.
    if is_in_non_parenthesized_arg(node, cx) {
        return;
    }

    let method = cx.method_name(node).unwrap_or("");

    // AllowedMethods: skip entirely.
    if opts.allowed_methods.iter().any(|m| m == method) {
        return;
    }

    // BracesRequiredMethods: flag if NOT using braces.
    if opts.braces_required_methods.iter().any(|m| m == method) {
        if !is_brace_block(node, cx) {
            let msg = format!(
                "Brace delimiters `{{...}}` required for '{}' method.",
                method
            );
            if let Some(opener) = find_block_opener(node, cx) {
                cx.emit_offense(opener, &msg, None);
            }
        }
        return;
    }

    // Semantic style: not implemented — skip without offense.
    if opts.enforced_style == EnforcedStyle::Semantic {
        return;
    }

    if is_proper_block_style(node, &opts, cx) {
        return;
    }

    let msg = message(node, &opts, cx);
    if let Some(opener) = find_block_opener(node, cx) {
        cx.emit_offense(opener, &msg, None);
    }
}

/// Whether the block's current delimiter style matches what the enforced style requires.
fn is_proper_block_style(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    let uses_braces = is_brace_block(node, cx);
    let multiline = cx.is_multiline(node);

    match opts.enforced_style {
        EnforcedStyle::LineCountBased => {
            // Single-line uses braces; multi-line uses do/end.
            // XOR: multiline and uses_braces are opposite when proper.
            multiline ^ uses_braces
        }
        EnforcedStyle::AlwaysBraces => uses_braces,
        EnforcedStyle::BracesForChaining => {
            if multiline {
                if cx.is_chained(node) {
                    // Multi-line chained uses braces.
                    uses_braces
                } else {
                    // Multi-line unchained uses do/end.
                    !uses_braces
                }
            } else {
                // Single-line uses braces.
                uses_braces
            }
        }
        // Semantic: not enforced (handled earlier with early return).
        EnforcedStyle::Semantic => true,
    }
}

/// Build the offense message for the given block and style.
fn message(node: NodeId, opts: &Options, cx: &Cx<'_>) -> String {
    let uses_braces = is_brace_block(node, cx);
    let multiline = cx.is_multiline(node);

    match opts.enforced_style {
        EnforcedStyle::LineCountBased => {
            if multiline {
                "Avoid using `{...}` for multi-line blocks.".to_string()
            } else {
                "Prefer `{...}` over `do...end` for single-line blocks.".to_string()
            }
        }
        EnforcedStyle::AlwaysBraces => "Prefer `{...}` over `do...end` for blocks.".to_string(),
        EnforcedStyle::BracesForChaining => {
            if multiline {
                if cx.is_chained(node) {
                    "Prefer `{...}` over `do...end` for multi-line chained blocks.".to_string()
                } else {
                    "Prefer `do...end` for multi-line blocks without chaining.".to_string()
                }
            } else {
                "Prefer `{...}` over `do...end` for single-line blocks.".to_string()
            }
        }
        // Semantic: should not reach here (early return in check()).
        EnforcedStyle::Semantic => {
            if uses_braces {
                "Prefer `do...end` over `{...}` for procedural blocks.".to_string()
            } else {
                "Prefer `{...}` over `do...end` for functional blocks.".to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Token helpers
// ---------------------------------------------------------------------------

/// Returns the start of the block body, or the block node end for empty bodies.
fn body_start(node: NodeId, cx: &Cx<'_>) -> u32 {
    match *cx.kind(node) {
        NodeKind::Block { body, .. }
        | NodeKind::Numblock { body, .. }
        | NodeKind::Itblock { body, .. } => {
            body.get().map_or(cx.range(node).end, |b| cx.range(b).start)
        }
        _ => cx.range(node).end,
    }
}


/// Checks if the block uses brace delimiters (`{...}`).
///
/// Scans tokens between the block node start and body start for the first
/// `{` (LeftBrace) or `do` keyword token. Uses paren-depth tracking to skip
/// brace tokens that appear inside the call's parenthesised arguments
/// (e.g. hash literals: `foo(a: { b: 1 }) do...end`).
fn is_brace_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let from = cx.range(node).start;
    let to = body_start(node, cx);

    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    let mut paren_depth: i32 = 0;
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => {
                paren_depth += 1;
            }
            SourceTokenKind::RightParen => {
                paren_depth -= 1;
            }
            SourceTokenKind::LeftBrace if paren_depth == 0 => return true,
            SourceTokenKind::Other
                if paren_depth == 0
                    && &src[tok.range.start as usize..tok.range.end as usize] == b"do" =>
            {
                return false;
            }
            _ => {}
        }
    }
    // No opener found — assume brace block (conservative).
    true
}

/// Finds the block's opening delimiter token range (`{` or `do` keyword).
///
/// Uses paren-depth tracking to skip brace tokens inside parenthesised
/// call arguments (e.g. hash literals). Returns `None` if not found.
fn find_block_opener(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let from = cx.range(node).start;
    let to = body_start(node, cx);

    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    let mut paren_depth: i32 = 0;
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => {
                paren_depth += 1;
            }
            SourceTokenKind::RightParen => {
                paren_depth -= 1;
            }
            SourceTokenKind::LeftBrace if paren_depth == 0 => return Some(tok.range),
            SourceTokenKind::Other
                if paren_depth == 0
                    && &src[tok.range.start as usize..tok.range.end as usize] == b"do" =>
            {
                return Some(tok.range);
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Ignore-node equivalent: check if block is in a non-parenthesised arg position
// ---------------------------------------------------------------------------

/// Mirrors RuboCop's `on_send` ignore logic.
///
/// RuboCop's `on_send` calls `get_blocks(arg)` for each argument of an
/// unparenthesised call and marks those blocks as ignored. This function
/// replicates that check from the block's perspective: walk ancestors to
/// determine if the block is reachable from an unparenthesised call's
/// argument list (without crossing a block-body boundary).
///
/// If true, changing `{...}` or `do...end` would alter which method the block
/// binds to (Ruby operator-precedence rule).
fn is_in_non_parenthesized_arg(block: NodeId, cx: &Cx<'_>) -> bool {
    // Walk ancestors up from the block. We look for a qualifying Send/Csend
    // where the block appears in the argument subtree.
    // ancestors() starts from the parent, so the block itself is not included.
    let block_range = cx.range(block);

    for ancestor in cx.ancestors(block) {
        match *cx.kind(ancestor) {
            NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => {
                // The ancestor call itself must have arguments and be non-parenthesised.
                let arg_list = cx.list(args);
                if arg_list.is_empty() {
                    // No args on this call: continue ascending.
                    continue;
                }
                if cx.is_parenthesized(ancestor) {
                    // Parenthesised call: braces/do-end binding is unambiguous.
                    return false;
                }
                if cx.is_assignment_method(ancestor) {
                    return false;
                }
                // Check single_argument_operator_method: operator method with one arg
                // that is a block kind — this one is allowed.
                if cx.is_operator_method(ancestor)
                    && arg_list.len() == 1
                    && is_block_kind(arg_list[0], cx)
                {
                    return false;
                }
                // The block must be reachable as an argument (not as the receiver).
                // Since we walked up from the block and hit this Send, the block is
                // in the argument subtree iff the block's range is within any arg's range.
                let block_in_args = arg_list.iter().any(|&arg_id| {
                    let arg_range = cx.range(arg_id);
                    arg_range.start <= block_range.start && block_range.end <= arg_range.end
                });
                if block_in_args {
                    return true;
                }
                // Block is the receiver of this Send (chained call), not an arg.
                return false;
            }
            // Don't cross into a block body: if we reach another block-like node
            // while ascending, the block is nested and the outer block's body
            // is a new scope. Stop.
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
                return false;
            }
            // Hash or pair: RuboCop's get_blocks recurses into non-braced hashes and pairs.
            // For our upward walk, continue ascending through these containers.
            NodeKind::Hash(_) | NodeKind::Pair { .. } => {}
            // Any other node: continue ascending.
            _ => {}
        }
    }
    false
}

/// Returns true if the node is any block kind.
fn is_block_kind(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{BlockDelimiters, EnforcedStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // -------------------------------------------------------------------------
    // line_count_based (default) — multi-line with braces → flag
    // -------------------------------------------------------------------------

    #[test]
    fn flags_multiline_brace_block() {
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            things.map { |thing|
                       ^ Avoid using `{...}` for multi-line blocks.
              something = thing.some_method
              process(something)
            }
        "#});
    }

    #[test]
    fn flags_single_line_do_end_block() {
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            items.each do |item| item / 5 end
                       ^^ Prefer `{...}` over `do...end` for single-line blocks.
        "#});
    }

    #[test]
    fn accepts_single_line_brace_block() {
        test::<BlockDelimiters>().expect_no_offenses("items.each { |item| item / 5 }\n");
    }

    #[test]
    fn accepts_multiline_do_end_block() {
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            things.map do |thing|
              something = thing.some_method
              process(something)
            end
        "#});
    }

    // -------------------------------------------------------------------------
    // AllowedMethods: default lambda, proc, it
    // -------------------------------------------------------------------------

    #[test]
    fn allows_lambda_do_end_single_line() {
        // lambda is in AllowedMethods — no offense even with do/end on a single line.
        test::<BlockDelimiters>().expect_no_offenses("foo = lambda do |x| x * 100 end\n");
    }

    #[test]
    fn allows_lambda_do_end_multiline() {
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            foo = lambda do |x|
              puts "Hello, #{x}"
            end
        "#});
    }

    #[test]
    fn allows_proc_do_end() {
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            foo = proc do
              something
            end
        "#});
    }

    // -------------------------------------------------------------------------
    // BracesRequiredMethods
    // -------------------------------------------------------------------------

    #[test]
    fn flags_braces_required_method_with_do_end() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                braces_required_methods: vec!["sig".to_string()],
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                sig do
                    ^^ Brace delimiters `{...}` required for 'sig' method.
                  params(foo: String).void
                end
            "#});
    }

    #[test]
    fn accepts_braces_required_method_with_braces() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                braces_required_methods: vec!["sig".to_string()],
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                sig {
                  params(foo: String).void
                }
            "#});
    }

    // BracesRequiredMethods takes precedence over EnforcedStyle — a multiline
    // brace block for a braces-required method must NOT be flagged.
    #[test]
    fn braces_required_overrides_enforced_style_multiline() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                braces_required_methods: vec!["sig".to_string()],
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                sig {
                  params(foo: String).void
                }
            "#});
    }

    // -------------------------------------------------------------------------
    // Non-parenthesised argument: block binding ambiguity → skip
    // -------------------------------------------------------------------------

    #[test]
    fn skips_block_in_non_parenthesized_call_args() {
        // `foo bar { }` — the braces bind to `bar`, not `foo`.
        // Changing to `do...end` would bind to `foo`. Skip the offense.
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            foo bar do
              baz
            end
        "#});
    }

    #[test]
    fn flags_block_in_parenthesized_call_args() {
        // `foo(bar do...end)` — parenthesised, so binding is unambiguous.
        // A single-line do-end should be flagged.
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            foo(bar do |x| x end)
                    ^^ Prefer `{...}` over `do...end` for single-line blocks.
        "#});
    }

    // -------------------------------------------------------------------------
    // always_braces
    // -------------------------------------------------------------------------

    #[test]
    fn always_braces_flags_multiline_do_end() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::AlwaysBraces,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                words.each do |word|
                           ^^ Prefer `{...}` over `do...end` for blocks.
                  word.flip.flop
                end
            "#});
    }

    #[test]
    fn always_braces_accepts_brace_block() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::AlwaysBraces,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                words.each { |word|
                  word.flip.flop
                }
            "#});
    }

    // -------------------------------------------------------------------------
    // braces_for_chaining
    // -------------------------------------------------------------------------

    #[test]
    fn braces_for_chaining_flags_multiline_brace_without_chain() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::BracesForChaining,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                words.each { |word|
                           ^ Prefer `do...end` for multi-line blocks without chaining.
                  word.flip.flop
                }
            "#});
    }

    #[test]
    fn braces_for_chaining_flags_multiline_do_end_chained() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::BracesForChaining,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                words.each do |word|
                           ^^ Prefer `{...}` over `do...end` for multi-line chained blocks.
                  word.flip.flop
                end.join("-")
            "#});
    }

    #[test]
    fn braces_for_chaining_flags_single_line_do_end() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::BracesForChaining,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                words.each do |word| word.flip end
                           ^^ Prefer `{...}` over `do...end` for single-line blocks.
            "#});
    }

    #[test]
    fn braces_for_chaining_accepts_multiline_brace_chained() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::BracesForChaining,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                words.each { |word|
                  word.flip.flop
                }.join("-")
            "#});
    }

    #[test]
    fn braces_for_chaining_accepts_multiline_do_end_unchained() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::BracesForChaining,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                words.each do |word|
                  word.flip.flop
                end
            "#});
    }

    // -------------------------------------------------------------------------
    // semantic style: no offense emitted (not implemented)
    // -------------------------------------------------------------------------

    #[test]
    fn semantic_style_no_offense() {
        test::<BlockDelimiters>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Semantic,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                foo = map do |x|
                  x
                end
            "#});
    }

    // -------------------------------------------------------------------------
    // numblock and itblock
    // -------------------------------------------------------------------------

    #[test]
    fn flags_numblock_multiline_brace() {
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            items.map { _1 * 2
                      ^ Avoid using `{...}` for multi-line blocks.
              3
            }
        "#});
    }

    #[test]
    fn flags_numblock_single_line_do_end() {
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            items.map do _1 * 2 end
                      ^^ Prefer `{...}` over `do...end` for single-line blocks.
        "#});
    }

    #[test]
    fn accepts_numblock_single_line_brace() {
        test::<BlockDelimiters>().expect_no_offenses("items.map { _1 * 2 }\n");
    }

    #[test]
    fn accepts_numblock_multiline_do_end() {
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            items.map do
              _1 * 2
            end
        "#});
    }

    // -------------------------------------------------------------------------
    // Hash args with brace syntax: should not affect block delimiter detection
    // -------------------------------------------------------------------------

    #[test]
    fn flags_single_line_do_end_block_after_hash_arg() {
        // `foo(a: { b: 1 }) do |x| x end` — the hash `{` is inside parens
        // and should not be detected as the block opener. The block is a
        // single-line do-end and should be flagged.
        test::<BlockDelimiters>().expect_offense(indoc! {r#"
            foo(a: { b: 1 }) do |x| x end
                             ^^ Prefer `{...}` over `do...end` for single-line blocks.
        "#});
    }

    #[test]
    fn accepts_multiline_do_end_block_after_hash_arg() {
        // `foo(a: { b: 1 }) do...end` — multiline do-end is correct style.
        test::<BlockDelimiters>().expect_no_offenses(indoc! {r#"
            foo(a: { b: 1 }) do |x|
              x
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(BlockDelimiters);

