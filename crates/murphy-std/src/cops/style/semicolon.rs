//! `Style/Semicolon` — flags semicolons used to terminate expressions or
//! separate multiple expressions on the same line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Semicolon
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags semicolons in the following positions:
//!     - Trailing: the last significant token on a line is `;`
//!     - Leading: the first token on a line is `;`
//!     - After `{` (brace block or hash opener): `foo {; bar }`
//!     - Before `}` (closing brace): `foo { bar; }`
//!     - Expression separator: `;` between expressions on the same line,
//!       when `AllowAsExpressionSeparator` is false (the default).
//!   AllowAsExpressionSeparator (default: false): when true, semicolons that
//!   separate expressions on the same line are permitted (but trailing,
//!   leading, and brace-adjacent semicolons are still flagged).
//!   Autocorrect is not implemented (RuboCop's corrector has several edge
//!   cases around ranges, lambdas, and hash value omission).
//!   Gap: the `check_for_line_terminator_or_opener` / `on_begin` split from
//!   RuboCop is re-implemented via pure token scanning — the `begin` node
//!   AST approach cannot be combined with `#[on_new_investigation]` in a
//!   single cop impl.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! puts "this is a test";
//! puts "test1"; puts "test2"
//! foo { bar; }
//! foo {; bar }
//!
//! # good
//! puts "this is a test"
//! puts "test1"
//! puts "test2"
//! def foo(a); z(3) end
//! class Foo; end
//! ```

use murphy_plugin_api::{CopOptions, Cx, SourceTokenKind, cop};

const MSG: &str = "Do not use semicolons to terminate expressions.";

/// Configuration options for `Style/Semicolon`.
#[derive(CopOptions)]
pub struct SemicolonOptions {
    #[option(
        name = "AllowAsExpressionSeparator",
        default = false,
        description = "Allow semicolons to separate multiple expressions on the same line."
    )]
    pub allow_as_expression_separator: bool,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct Semicolon;

#[cop(
    name = "Style/Semicolon",
    description = "Do not use semicolons to terminate expressions.",
    default_severity = "warning",
    default_enabled = true,
    options = SemicolonOptions,
)]
impl Semicolon {
    /// Scan all `;` tokens and flag those that are structural offenses or
    /// expression separators (when `AllowAsExpressionSeparator: false`).
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<SemicolonOptions>();
        let source = cx.source();
        let bytes = source.as_bytes();
        let tokens = cx.sorted_tokens();

        for (i, tok) in tokens.iter().enumerate() {
            if !is_semicolon(tok, bytes) {
                continue;
            }
            let flag = should_flag(i, tokens, bytes, opts.allow_as_expression_separator);
            if flag {
                cx.emit_offense(tok.range, MSG, None);
            }
        }
    }
}

/// Returns true if `tok` is an `Other` token with text `b";"`.
fn is_semicolon(tok: &murphy_plugin_api::SourceToken, bytes: &[u8]) -> bool {
    tok.kind == SourceTokenKind::Other
        && &bytes[tok.range.start as usize..tok.range.end as usize] == b";"
}

/// Returns true if the semicolon at `tokens[idx]` should be flagged.
///
/// A semicolon is flagged when it is:
/// - trailing (last significant token on its line)
/// - leading (first token on its line)
/// - immediately after `{`
/// - immediately before `}`
/// - an expression separator AND `allow_as_expression_separator` is false
fn should_flag(
    idx: usize,
    tokens: &[murphy_plugin_api::SourceToken],
    bytes: &[u8],
    allow_separator: bool,
) -> bool {
    let semi_start = tokens[idx].range.start as usize;

    // (1) Leading: first non-whitespace on its line.
    let line_start = line_start_before(bytes, semi_start);
    if bytes[line_start..semi_start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
    {
        return true;
    }

    let prev = prev_token_same_line(idx, tokens, bytes);
    let next = next_token_same_line(idx, tokens, bytes);
    // (2) After opening `{`.
    if prev.is_some_and(|p| tokens[p].kind == SourceTokenKind::LeftBrace) {
        return true;
    }

    // (3) Before closing `}`.
    if next.is_some_and(|n| tokens[n].kind == SourceTokenKind::RightBrace) {
        return true;
    }

    // (4) Trailing: nothing significant after `;` on this line.
    if next.is_none() {
        return true;
    }

    // (5) Expression separator (non-structural, non-def-like).
    if !allow_separator && !is_def_like_separator(idx, tokens, bytes) {
        return true;
    }

    false
}

/// Returns the index of the closest previous significant token on the same
/// source line (skipping comments).
fn prev_token_same_line(
    idx: usize,
    tokens: &[murphy_plugin_api::SourceToken],
    bytes: &[u8],
) -> Option<usize> {
    let semi_line = line_of(bytes, tokens[idx].range.start as usize);
    for i in (0..idx).rev() {
        let tok = &tokens[i];
        if line_of(bytes, tok.range.start as usize) < semi_line {
            break;
        }
        if tok.kind == SourceTokenKind::Comment {
            continue;
        }
        return Some(i);
    }
    None
}

/// Returns the index of the next significant token on the same source line
/// (skipping comments and newlines).
fn next_token_same_line(
    idx: usize,
    tokens: &[murphy_plugin_api::SourceToken],
    bytes: &[u8],
) -> Option<usize> {
    let semi_line = line_of(bytes, tokens[idx].range.start as usize);
    for (i, tok) in tokens.iter().enumerate().skip(idx + 1) {
        let tok_line = line_of(bytes, tok.range.start as usize);
        if tok_line > semi_line {
            break;
        }
        // Skip newline-like tokens. Also handles `Other` tokens that are
        // literally `\n` bytes (e.g. trailing newline at end of file emitted
        // by Prism with kind=Other instead of Newline).
        match tok.kind {
            SourceTokenKind::Comment
            | SourceTokenKind::Newline
            | SourceTokenKind::IgnoredNewline => continue,
            SourceTokenKind::Other => {
                // If this `Other` token's text starts with `\n`, it is a
                // newline in disguise — treat it as end-of-line and stop.
                let start = tok.range.start as usize;
                let end = tok.range.end as usize;
                if bytes.get(start..end) == Some(b"\n") {
                    break;
                }
                return Some(i);
            }
            _ => return Some(i),
        }
    }
    None
}

/// Returns the byte offset of the first byte on the line containing `pos`.
fn line_start_before(bytes: &[u8], pos: usize) -> usize {
    bytes[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// Returns the 0-based line number of the byte at `pos`.
fn line_of(bytes: &[u8], pos: usize) -> usize {
    bytes[..pos].iter().filter(|&&b| b == b'\n').count()
}

/// Returns true if the `;` is a `def`/`class`/`module` one-liner signature
/// separator that should NOT be flagged.
///
/// Heuristic: scan backwards on the same line to find a `def`/`class`/`module`
/// keyword. Then count the number of distinct paren-pair openings (`(`) between
/// that keyword and this `;`. If there is at most ONE opening paren (covering
/// the method parameter list), this `;` is the signature separator and should
/// be allowed. Two or more paren openings indicate that at least one method
/// call expression was made in the body before the `;`.
fn is_def_like_separator(
    idx: usize,
    tokens: &[murphy_plugin_api::SourceToken],
    bytes: &[u8],
) -> bool {
    let semi_line = line_of(bytes, tokens[idx].range.start as usize);

    // Find the leftmost `def`/`class`/`module` keyword on this line.
    let mut kw_idx: Option<usize> = None;
    for i in (0..idx).rev() {
        let tok = &tokens[i];
        if line_of(bytes, tok.range.start as usize) < semi_line {
            break;
        }
        if tok.kind == SourceTokenKind::Other {
            let text = &bytes[tok.range.start as usize..tok.range.end as usize];
            if matches!(text, b"def" | b"class" | b"module") {
                kw_idx = Some(i);
                // Keep scanning for an earlier kw (e.g. `def self.foo`).
                // We want the first one on the line. Actually, only one
                // def/class/module can begin a method signature on a line.
                break;
            }
        }
    }

    let kw_idx = match kw_idx {
        Some(i) => i,
        None => return false,
    };

    // Scan between the keyword and this `;`, counting:
    // - paren_opens: number of LeftParen tokens (0 or 1 for the param list)
    // - prior_semis: number of `;` tokens that appeared before this one
    //
    // If there are 0 or 1 `(` AND no prior `;`, this is the signature
    // separator. Prior semicolons indicate the body has already started.
    //
    // Examples:
    //   `def foo;` → 0 parens, 0 prior semis → allowed
    //   `def foo(a);` → 1 paren, 0 prior semis → allowed
    //   `def foo; bar; baz` → second `;`: 0 parens, 1 prior semi → NOT allowed
    //   `def foo(a) x(1);` → 2 parens, 0 prior semis → NOT allowed
    let mut paren_opens = 0usize;
    let mut prior_semis = 0usize;
    for tok in &tokens[(kw_idx + 1)..idx] {
        if line_of(bytes, tok.range.start as usize) != semi_line {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => paren_opens += 1,
            SourceTokenKind::Other => {
                let text = &bytes[tok.range.start as usize..tok.range.end as usize];
                if text == b";" {
                    prior_semis += 1;
                }
            }
            _ => {}
        }
    }

    paren_opens <= 1 && prior_semis == 0
}

#[cfg(test)]
mod tests {
    use super::{Semicolon, SemicolonOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Trailing semicolons -----

    #[test]
    fn flags_trailing_semicolon() {
        test::<Semicolon>().expect_offense(indoc! {r#"
            puts "this is a test";
                                 ^ Do not use semicolons to terminate expressions.
        "#});
    }

    #[test]
    fn flags_trailing_semicolon_after_end() {
        // `module Foo; end;` — the first `;` is the signature separator
        // (allowed), the trailing `;` after `end` is flagged.
        test::<Semicolon>().expect_offense(indoc! {r#"
            module Foo; end;
                           ^ Do not use semicolons to terminate expressions.
        "#});
    }

    // ----- Expression separators -----

    #[test]
    fn flags_semicolon_separating_expressions() {
        test::<Semicolon>().expect_offense(indoc! {r#"
            puts "test1"; puts "test2"
                        ^ Do not use semicolons to terminate expressions.
        "#});
    }

    #[test]
    fn flags_all_semicolons_in_multiexpr_oneliner() {
        // Three semicolons between expressions in `def`; all are offenses.
        // The def has no explicit paren arg list so `def foo(a)` space is
        // actually `def foo(a) x(1)` — two paren-opens → not signature sep.
        test::<Semicolon>().expect_offense(indoc! {r#"
            def foo(a) x(1); y(2); z(3); end
                           ^ Do not use semicolons to terminate expressions.
                                 ^ Do not use semicolons to terminate expressions.
                                       ^ Do not use semicolons to terminate expressions.
        "#});
    }

    #[test]
    fn flags_second_semicolon_in_def_multiexpr_body() {
        // `def foo; bar; baz end` — first `;` is the signature separator
        // (allowed), second `;` separates `bar` from `baz` (expression
        // separator — flagged).
        test::<Semicolon>().expect_offense(indoc! {r#"
            def foo; bar; baz end
                        ^ Do not use semicolons to terminate expressions.
        "#});
    }

    #[test]
    fn flags_second_semicolon_when_arg_list_present() {
        // `def foo(a); bar; baz end` — first `;` (after `(a)`) is allowed,
        // second `;` (between `bar` and `baz`) must be flagged.
        test::<Semicolon>().expect_offense(indoc! {r#"
            def foo(a); bar; baz end
                           ^ Do not use semicolons to terminate expressions.
        "#});
    }

    // ----- Brace context -----

    #[test]
    fn flags_semicolon_before_right_brace() {
        test::<Semicolon>().expect_offense(indoc! {r#"
            foo { bar; }
                     ^ Do not use semicolons to terminate expressions.
        "#});
    }

    #[test]
    fn flags_semicolon_after_left_brace() {
        test::<Semicolon>().expect_offense(indoc! {r#"
            foo {; bar }
                 ^ Do not use semicolons to terminate expressions.
        "#});
    }

    // ----- Leading semicolons -----

    #[test]
    fn flags_leading_semicolon() {
        test::<Semicolon>().expect_offense(indoc! {r#"
            ; puts 1
            ^ Do not use semicolons to terminate expressions.
        "#});
    }

    // ----- Allowed cases -----

    #[test]
    fn accepts_single_expression_def_body() {
        test::<Semicolon>().expect_no_offenses("def foo(a); z(3) end\n");
    }

    #[test]
    fn accepts_single_expression_def_no_args() {
        test::<Semicolon>().expect_no_offenses("def foo1; x(3) end\n");
    }

    #[test]
    fn accepts_empty_def() {
        test::<Semicolon>().expect_no_offenses("def initialize(*_); end\n");
    }

    #[test]
    fn accepts_class_with_superclass() {
        test::<Semicolon>().expect_no_offenses("class Foo < Exception; end\n");
    }

    #[test]
    fn accepts_bare_class_oneliner() {
        test::<Semicolon>().expect_no_offenses("class Bar; end\n");
    }

    #[test]
    fn accepts_module_oneliner() {
        test::<Semicolon>().expect_no_offenses("module Foo; end\n");
    }

    #[test]
    fn accepts_semicolon_in_string() {
        test::<Semicolon>().expect_no_offenses("puts \"x;y\"\n");
    }

    #[test]
    fn accepts_semicolon_in_comment() {
        test::<Semicolon>().expect_no_offenses("x = 1 # ; semicolon in comment\n");
    }

    // ----- AllowAsExpressionSeparator: true -----

    #[test]
    fn allow_separator_permits_expression_separator() {
        let opts = SemicolonOptions {
            allow_as_expression_separator: true,
        };
        test::<Semicolon>()
            .with_options(&opts)
            .expect_no_offenses("puts \"test1\"; puts \"test2\"\n");
    }

    #[test]
    fn allow_separator_permits_multiexpr_def_oneliner() {
        let opts = SemicolonOptions {
            allow_as_expression_separator: true,
        };
        test::<Semicolon>()
            .with_options(&opts)
            .expect_no_offenses("def foo(a) x(1); y(2); z(3); end\n");
    }

    #[test]
    fn allow_separator_still_flags_brace_before_close() {
        let opts = SemicolonOptions {
            allow_as_expression_separator: true,
        };
        test::<Semicolon>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                foo { bar; }
                         ^ Do not use semicolons to terminate expressions.
            "#});
    }

    #[test]
    fn allow_separator_still_flags_trailing() {
        let opts = SemicolonOptions {
            allow_as_expression_separator: true,
        };
        test::<Semicolon>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                puts "this is a test";
                                     ^ Do not use semicolons to terminate expressions.
            "#});
    }
}

murphy_plugin_api::submit_cop!(Semicolon);
