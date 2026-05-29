//! `Layout/DotPosition` — flags misplaced `.` / `&.` operators in
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/DotPosition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-6udc
//! notes: >
//!   All RuboCop spec cases covered including implicit-call nodes with
//!   no method name (l.\n(1) / l\n.(1)).
//! ```
//!
//! multi-line method chains. Mirrors RuboCop's same-named cop;
//! `EnforcedStyle: leading | trailing` (default `leading`) selects
//! which side of the chain the operator should sit on.
//!
//! ## Matched shapes
//!
//! `Send` / `Csend` nodes with an explicit dot operator (`.` or `&.`)
//! whose dot does not match the configured style. Single-line
//! `foo.bar` is ignored.
//!
//! Skips when there is an intervening comment or blank line between
//! the receiver/dot and the selector — that is the user's existing
//! whitespace, not a positioning bug.
//!
//! ## Why this shape
//!
//! Murphy's `Cx::call_operator_loc` computes the dot's source range
//! on demand by scanning the bytes between `receiver.expression.end`
//! and `loc.name.start`. For receivers that contain a heredoc (str
//! literal whose `expression.end` is just the opener), the dot's
//! physical line is reached after the heredoc body — the cop uses the
//! `HeredocEnd` `SourceToken`s between receiver-start and selector-start
//! to compute the *effective* receiver-end line, matching RuboCop's
//! `last_heredoc_line` correction.
//!
//! For implicit-call nodes (`l.(1)`) whose `loc.name == Range::ZERO`,
//! `call_operator_loc` degenerates to an empty scan window. Murphy
//! mirrors RuboCop's `selector_range` substitution: it finds the first
//! `SourceTokenKind::LeftParen` token after the receiver and scans
//! between the receiver end and that paren for the dot.
//!
//! ## Autocorrect
//!
//! - Remove the dot from its current position. If the dot is the only
//!   non-whitespace on its line, remove the entire line including the
//!   trailing newline; otherwise remove just the operator bytes.
//! - Insert the dot text (`.` or `&.`) at the target site: before the
//!   opening paren for `leading` (implicit-call), before the
//!   selector for `leading` (named selector), or after the receiver
//!   for `trailing`.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, cop,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DotPosition;

/// Options for [`DotPosition`]. The `EnforcedStyle` key matches
/// RuboCop verbatim; `leading` is the project-wide default mirroring
/// the upstream cop's default.
#[derive(CopOptions)]
pub struct DotPositionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "leading",
        description = "Where the `.` / `&.` operator sits in a multi-line method chain."
    )]
    pub enforced_style: DotPositionStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum DotPositionStyle {
    /// `something\n  .method_name` — dot leads the selector line.
    #[option(value = "leading")]
    Leading,
    /// `something.\n  method_name` — dot trails the receiver line.
    #[option(value = "trailing")]
    Trailing,
}

#[cop(
    name = "Layout/DotPosition",
    description = "Enforce dot operator placement in multi-line method chains.",
    default_severity = "warning",
    default_enabled = true,
    options = DotPositionOptions,
)]
impl DotPosition {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // The host wires `[cops.rules."Layout/DotPosition"]` (and in
    // particular `EnforcedStyle`) through `CxRaw::options_json` —
    // see `murphy_cli::main::dispatch_lint` calling
    // `dispatch::run_cops_with_options` with `config.cop_options_json`.
    // `cx.options_or_default::<DotPositionOptions>()` decodes it and
    // falls back to the `Default` (`leading`) when no override is set.
    let opts = cx.options_or_default::<DotPositionOptions>();
    let style = opts.enforced_style;

    // Detect implicit-call nodes: `loc.name == Range::ZERO` means the
    // call has no method-name token (e.g. `l.(1)` or `l\n.(1)`).
    // `call_operator_loc` degenerates to None for these because its
    // scan window is empty. Mirror RuboCop's `selector_range`
    // substitution: find the first LeftParen token after the receiver
    // and use its start as the "name_start" proxy.
    let name_loc = cx.loc(node).name;
    if name_loc == Range::ZERO {
        check_implicit_call(node, cx, style);
        return;
    }

    let Some(dot_range) = cx.call_operator_loc(node) else {
        return;
    };

    let receiver = match *cx.kind(node) {
        NodeKind::Send {
            receiver: OptNodeId(idx),
            ..
        } if idx != u32::MAX => NodeId(idx),
        NodeKind::Csend { receiver, .. } => receiver,
        _ => return,
    };

    let source = cx.source();
    let name_start = cx.loc(node).name.start;
    let receiver_naive_end = cx.range(receiver).end;

    // Single-line call? `recv_end` and the selector sit on the same
    // physical line, so neither leading nor trailing style is
    // violated regardless of where the dot is.
    let receiver_to_selector = slice_or_empty(source, receiver_naive_end, name_start);
    if !contains_newline(receiver_to_selector) {
        return;
    }

    // Intervening blank or comment line? Use the effective receiver
    // end (heredoc-aware) plus the dot end as the lower bound, then
    // count newlines through to the selector. >=2 newlines means at
    // least one entirely intermediate line — skip (matches RuboCop's
    // `line_between?` check).
    let receiver_effective_end = effective_receiver_end(cx, receiver_naive_end, name_start);
    let pivot = receiver_effective_end.max(dot_range.end);
    let pivot_to_selector = slice_or_empty(source, pivot, name_start);
    if count_newlines(pivot_to_selector) >= 2 {
        return;
    }

    // Is the dot sitting on the selector's line? (No newlines between
    // dot end and selector start => same line.)
    let dot_on_selector_line = !contains_newline(slice_or_empty(source, dot_range.end, name_start));

    let offense_for_this_style = match style {
        DotPositionStyle::Leading => !dot_on_selector_line,
        DotPositionStyle::Trailing => dot_on_selector_line,
    };
    if !offense_for_this_style {
        return;
    }

    let dot_text = cx.raw_source(dot_range);
    let message = match style {
        DotPositionStyle::Leading => {
            format!("Place the {dot_text} on the next line, together with the method name.")
        }
        DotPositionStyle::Trailing => format!(
            "Place the {dot_text} on the previous line, together with the method call receiver."
        ),
    };
    cx.emit_offense(dot_range, &message, None);

    let removal = removal_range(source, dot_range);
    cx.emit_edit(removal, "");
    let insert_at = match style {
        DotPositionStyle::Leading => name_start,
        DotPositionStyle::Trailing => receiver_naive_end,
    };
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        dot_text,
    );
}

/// Handle implicit-call nodes (`l.(1)` / `l\n.(1)`) where `loc.name ==
/// Range::ZERO`. RuboCop substitutes `node.loc.begin` (the opening
/// parenthesis) as the selector range in this case. We locate the first
/// `SourceTokenKind::LeftParen` token whose start is >= receiver_end and
/// < node_end, then scan between receiver_end and paren_start for the dot,
/// and use paren_start as the name_start proxy for all subsequent checks.
fn check_implicit_call(node: NodeId, cx: &Cx<'_>, style: DotPositionStyle) {
    let receiver = match *cx.kind(node) {
        NodeKind::Send {
            receiver: OptNodeId(idx),
            ..
        } if idx != u32::MAX => NodeId(idx),
        NodeKind::Csend { receiver, .. } => receiver,
        _ => return,
    };

    let source = cx.source();
    let receiver_naive_end = cx.range(receiver).end;
    let node_end = cx.range(node).end;

    // Find the first LeftParen token after the receiver end.
    // sorted_tokens is sorted by start position, so binary_search_by_key
    // lets us jump directly to receiver_naive_end instead of scanning from 0.
    let tokens = cx.sorted_tokens();
    let start_idx = tokens
        .binary_search_by_key(&receiver_naive_end, |tok| tok.range.start)
        .unwrap_or_else(|idx| idx);
    let paren_start = tokens[start_idx..]
        .iter()
        .take_while(|tok| tok.range.start < node_end)
        .find(|tok| tok.kind == SourceTokenKind::LeftParen)
        .map(|tok| tok.range.start);
    let Some(paren_start) = paren_start else {
        return;
    };

    // Scan receiver_naive_end..paren_start for the dot operator.
    let dot_range = scan_dot(source, receiver_naive_end, paren_start);
    let Some(dot_range) = dot_range else {
        return;
    };

    // Single-line call: no newline between receiver end and paren start.
    let receiver_to_paren = slice_or_empty(source, receiver_naive_end, paren_start);
    if !contains_newline(receiver_to_paren) {
        return;
    }

    // Intervening blank or comment line check (same as named-selector path).
    let receiver_effective_end = effective_receiver_end(cx, receiver_naive_end, paren_start);
    let pivot = receiver_effective_end.max(dot_range.end);
    let pivot_to_paren = slice_or_empty(source, pivot, paren_start);
    if count_newlines(pivot_to_paren) >= 2 {
        return;
    }

    // Is the dot on the paren's line?
    let dot_on_paren_line = !contains_newline(slice_or_empty(source, dot_range.end, paren_start));

    let offense_for_this_style = match style {
        DotPositionStyle::Leading => !dot_on_paren_line,
        DotPositionStyle::Trailing => dot_on_paren_line,
    };
    if !offense_for_this_style {
        return;
    }

    let dot_text = cx.raw_source(dot_range);
    let message = match style {
        DotPositionStyle::Leading => {
            format!("Place the {dot_text} on the next line, together with the method name.")
        }
        DotPositionStyle::Trailing => format!(
            "Place the {dot_text} on the previous line, together with the method call receiver."
        ),
    };
    cx.emit_offense(dot_range, &message, None);

    let removal = removal_range(source, dot_range);
    cx.emit_edit(removal, "");
    // For Leading: insert dot before the opening paren (RuboCop's
    // `selector_range.begin_pos` substitute). For Trailing: insert
    // after receiver end (same as the named-selector path).
    let insert_at = match style {
        DotPositionStyle::Leading => paren_start,
        DotPositionStyle::Trailing => receiver_naive_end,
    };
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        dot_text,
    );
}

/// Scan `source[start..end]` for a `.` or `&.` operator, skipping
/// `#`-to-newline line comments. Returns the byte range of the dot if
/// found, otherwise `None`.
fn scan_dot(source: &str, start: u32, end: u32) -> Option<Range> {
    if start >= end {
        return None;
    }
    let src = source.as_bytes();
    let window = src.get(start as usize..end as usize)?;
    let mut i = 0;
    let mut in_comment = false;
    while i < window.len() {
        let b = window[i];
        if b == b'\n' {
            in_comment = false;
            i += 1;
            continue;
        }
        if in_comment {
            i += 1;
            continue;
        }
        if b == b'#' {
            in_comment = true;
            i += 1;
            continue;
        }
        if b == b'&' && i + 1 < window.len() && window[i + 1] == b'.' {
            let dot_start = start + i as u32;
            return Some(Range {
                start: dot_start,
                end: dot_start + 2,
            });
        }
        if b == b'.' {
            let dot_start = start + i as u32;
            return Some(Range {
                start: dot_start,
                end: dot_start + 1,
            });
        }
        i += 1;
    }
    None
}

/// Effective end of the receiver for line-distance math. When the
/// receiver contains a heredoc (str literal whose source range covers
/// only the opener), Prism reports the body and closer through
/// `SourceTokenKind::HeredocEnd` tokens that fall between the
/// receiver-start and the selector-start. We take the last such
/// `HeredocEnd`'s end as the effective receiver-end line so the
/// "intervening blank" skip does not silently suppress offenses
/// on heredoc-bearing chains.
fn effective_receiver_end(cx: &Cx<'_>, naive_end: u32, name_start: u32) -> u32 {
    let mut effective = naive_end;
    for tok in cx.sorted_tokens() {
        if tok.kind == SourceTokenKind::HeredocEnd
            && tok.range.start >= naive_end
            && tok.range.end <= name_start
        {
            effective = effective.max(tok.range.end);
        }
    }
    effective
}

/// Range to delete when removing the dot. If the dot's line consists
/// only of whitespace + the dot, expand to the whole line including
/// the trailing newline — otherwise the corrected output would leave a
/// blank line behind. Pass through the dot range unchanged otherwise.
fn removal_range(source: &str, dot_range: Range) -> Range {
    let bytes = source.as_bytes();
    let line_start = bytes[..dot_range.start as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32;
    let line_end_excl = bytes[dot_range.end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| dot_range.end as usize + i + 1)
        .unwrap_or(bytes.len()) as u32;

    let before = &bytes[line_start as usize..dot_range.start as usize];
    let after = &bytes[dot_range.end as usize..line_end_excl as usize];
    let only_dot_on_line = before.iter().all(|b| matches!(b, b' ' | b'\t'))
        && after
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));

    if only_dot_on_line {
        Range {
            start: line_start,
            end: line_end_excl,
        }
    } else {
        dot_range
    }
}

fn slice_or_empty(source: &str, start: u32, end: u32) -> &str {
    if start >= end {
        return "";
    }
    &source[start as usize..end as usize]
}

fn contains_newline(s: &str) -> bool {
    s.bytes().any(|b| b == b'\n')
}

fn count_newlines(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::{DotPosition, DotPositionOptions, DotPositionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    /// Pinned trailing-style options for the trailing-mode test block.
    /// Constructing the typed `DotPositionOptions` here mirrors what a
    /// user would write in `murphy.toml` (`EnforcedStyle = "trailing"`);
    /// the harness routes it through `Cx::options_or_default` exactly
    /// like the real host does.
    fn trailing() -> DotPositionOptions {
        DotPositionOptions {
            enforced_style: DotPositionStyle::Trailing,
        }
    }

    // ----- EnforcedStyle = leading (default) ------------------------

    #[test]
    fn flags_trailing_dot_in_multiline_call() {
        test::<DotPosition>().expect_offense(indoc! {"
            something.
                     ^ Place the . on the next line, together with the method name.
              method_name
        "});
    }

    #[test]
    fn corrects_trailing_dot_to_leading() {
        test::<DotPosition>().expect_correction(
            indoc! {"
                something.
                         ^ Place the . on the next line, together with the method name.
                  method_name
            "},
            indoc! {"
                something
                  .method_name
            "},
        );
    }

    #[test]
    fn accepts_leading_dot_chain() {
        test::<DotPosition>().expect_no_offenses(indoc! {"
            something
              .method_name
        "});
    }

    #[test]
    fn accepts_same_line_call() {
        test::<DotPosition>().expect_no_offenses("something.method_name\n");
    }

    #[test]
    fn accepts_method_with_no_dots() {
        test::<DotPosition>().expect_no_offenses("puts something\n");
    }

    #[test]
    fn accepts_intervening_line_comment() {
        test::<DotPosition>().expect_no_offenses(indoc! {"
            something.
            # a comment here
              method_name
        "});
    }

    #[test]
    fn accepts_intervening_blank_line() {
        test::<DotPosition>().expect_no_offenses(indoc! {"
            something.

              method_name
        "});
    }

    #[test]
    fn flags_safe_navigation_trailing() {
        test::<DotPosition>().expect_offense(indoc! {"
            something&.
                     ^^ Place the &. on the next line, together with the method name.
              method_name
        "});
    }

    #[test]
    fn corrects_safe_navigation_trailing_to_leading() {
        test::<DotPosition>().expect_correction(
            indoc! {"
                something&.
                         ^^ Place the &. on the next line, together with the method name.
                  method_name
            "},
            indoc! {"
                something
                  &.method_name
            "},
        );
    }

    #[test]
    fn flags_paren_args_with_trailing_dot() {
        test::<DotPosition>().expect_correction(
            indoc! {"
                something(
                  foo, bar
                ).
                 ^ Place the . on the next line, together with the method name.
                  method_name
            "},
            indoc! {"
                something(
                  foo, bar
                )
                  .method_name
            "},
        );
    }

    #[test]
    fn corrects_lone_dot_line_by_removing_whole_line() {
        // `foo\n  .bar\n  .\n  baz` — the outer Send `(foo.bar).baz` has
        // its dot alone on line 3. The fix removes that whole line and
        // inserts the dot before `baz`.
        test::<DotPosition>().expect_correction(
            indoc! {"
                foo
                  .bar
                  .
                  ^ Place the . on the next line, together with the method name.
                  baz
            "},
            indoc! {"
                foo
                  .bar
                  .baz
            "},
        );
    }

    #[test]
    fn flags_heredoc_receiver() {
        test::<DotPosition>().expect_correction(
            indoc! {"
                <<~HEREDOC.
                          ^ Place the . on the next line, together with the method name.
                  something
                HEREDOC
                  method_name
            "},
            indoc! {"
                <<~HEREDOC
                  something
                HEREDOC
                  .method_name
            "},
        );
    }

    #[test]
    fn accepts_heredoc_arg_with_same_line_chain() {
        // `foo(<<~HEREDOC).squish` — the chain is on the same line as
        // the closing `)`. No offense regardless of the heredoc body
        // that follows.
        test::<DotPosition>().expect_no_offenses(indoc! {"
            foo(<<~HEREDOC).squish
              something
            HEREDOC
        "});
    }

    #[test]
    fn flags_multiple_offenses_on_chain() {
        // RuboCop spec: every trailing-dot link in a chain fires
        // independently. The autocorrect rewrites the whole chain.
        test::<DotPosition>().expect_correction(
            indoc! {"
                @objects = @objects.
                                   ^ Place the . on the next line, together with the method name.
                  with_relation.
                               ^ Place the . on the next line, together with the method name.
                  paginate
            "},
            indoc! {"
                @objects = @objects
                  .with_relation
                  .paginate
            "},
        );
    }

    #[test]
    fn flags_dynamic_heredoc_receiver() {
        // Heredoc with `#{...}` interpolation: the str node carries
        // interpolation parts but the `HeredocEnd` token still spans the
        // closer line, so `effective_receiver_end` lifts past the body.
        test::<DotPosition>().expect_correction(
            indoc! {r#"
                <<~HEREDOC.
                          ^ Place the . on the next line, together with the method name.
                  #{value}
                HEREDOC
                  method_name
            "#},
            indoc! {r#"
                <<~HEREDOC
                  #{value}
                HEREDOC
                  .method_name
            "#},
        );
    }

    #[test]
    fn skips_double_colon_call() {
        // `::` is not a dot operator. `call_operator_loc` returns `None`
        // and the cop never sees the call — RuboCop matches this stance
        // (`DotPosition` only fires on `node.dot?` / `safe_navigation?`).
        test::<DotPosition>()
            .expect_no_offenses(indoc! {"
                Foo::bar
            "})
            .expect_no_offenses(indoc! {"
                Foo
                  ::bar
            "});
    }

    #[test]
    fn flags_implicit_call_leading_style_trailing_dot() {
        // RuboCop spec: `l.\n(1)` — dot trails the receiver line, offense
        // expected on the `.` under leading style. Previously a v1 gap.
        test::<DotPosition>().expect_correction(
            indoc! {"
                l.
                 ^ Place the . on the next line, together with the method name.
                (1)
            "},
            indoc! {"
                l
                .(1)
            "},
        );
    }

    #[test]
    fn flags_implicit_call_trailing_style_leading_dot() {
        // RuboCop spec: `l\n.(1)` — dot leads the next line, offense
        // expected on the `.` under trailing style.
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_correction(
                indoc! {"
                    l
                    .(1)
                    ^ Place the . on the previous line, together with the method call receiver.
                "},
                indoc! {"
                    l.
                    (1)
                "},
            );
    }

    #[test]
    fn flags_xstr_heredoc_receiver() {
        // Backtick-flavoured heredoc — same `HeredocEnd` token shape, so
        // the effective-receiver-end lift works the same as for plain
        // heredocs.
        test::<DotPosition>().expect_correction(
            indoc! {"
                <<~`HEREDOC`.
                            ^ Place the . on the next line, together with the method name.
                  ls -la
                HEREDOC
                  method_name
            "},
            indoc! {"
                <<~`HEREDOC`
                  ls -la
                HEREDOC
                  .method_name
            "},
        );
    }

    #[test]
    fn flags_multiple_heredoc_args() {
        // Two heredocs in the same arg list: `effective_receiver_end`
        // takes the max over `HeredocEnd` tokens, so it lifts past the
        // last (`THERE`) closer rather than the first (`HERE`).
        test::<DotPosition>().expect_correction(
            indoc! {"
                my_method.
                         ^ Place the . on the next line, together with the method name.
                  something(<<~HERE, <<~THERE).
                                              ^ Place the . on the next line, together with the method name.
                    a
                  HERE
                    b
                  THERE
                  somethingelse
            "},
            indoc! {"
                my_method
                  .something(<<~HERE, <<~THERE)
                    a
                  HERE
                    b
                  THERE
                  .somethingelse
            "},
        );
    }

    #[test]
    fn flags_heredoc_arg_with_trailing_dot() {
        // The outer `.somethingelse` call has receiver
        // `my_method.\n  something(<<~HERE)` whose heredoc body sits
        // between the dot and the selector. The effective receiver-end
        // line is the HEREDOC closer line, so the "intervening blank"
        // skip must not fire.
        test::<DotPosition>().expect_correction(
            indoc! {"
                my_method.
                         ^ Place the . on the next line, together with the method name.
                  something(<<~HERE).
                                    ^ Place the . on the next line, together with the method name.
                    something
                  HERE
                  somethingelse
            "},
            indoc! {"
                my_method
                  .something(<<~HERE)
                    something
                  HERE
                  .somethingelse
            "},
        );
    }

    // ----- EnforcedStyle = trailing ---------------------------------

    #[test]
    fn trailing_flags_leading_dot_in_multiline_call() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_offense(indoc! {"
                something
                  .method_name
                  ^ Place the . on the previous line, together with the method call receiver.
            "});
    }

    #[test]
    fn trailing_corrects_leading_dot_to_trailing() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_correction(
                indoc! {"
                something
                  .method_name
                  ^ Place the . on the previous line, together with the method call receiver.
            "},
                indoc! {"
                something.
                  method_name
            "},
            );
    }

    #[test]
    fn trailing_accepts_trailing_dot_chain() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_no_offenses(indoc! {"
                something.
                  method_name
            "});
    }

    #[test]
    fn trailing_accepts_same_line_call() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_no_offenses("something.method_name\n");
    }

    #[test]
    fn trailing_accepts_method_with_no_dots() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_no_offenses("puts something\n");
    }

    #[test]
    fn trailing_accepts_intervening_blank_line() {
        // Same intervening-blank skip applies in trailing mode.
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_no_offenses(indoc! {"
                something

                  .method_name
            "});
    }

    #[test]
    fn trailing_corrects_safe_navigation_to_trailing() {
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_correction(
                indoc! {"
                something
                  &.method_name
                  ^^ Place the &. on the previous line, together with the method call receiver.
            "},
                indoc! {"
                something&.
                  method_name
            "},
            );
    }

    #[test]
    fn trailing_accepts_multi_line_paren_args_with_trailing_dot() {
        // RuboCop's "does not err on method call with multi-line arguments"
        // case: receiver ends at `)` and selector follows on the same
        // physical line — no offense regardless of style.
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_no_offenses(indoc! {"
                foo(
                  bar
                ).baz
            "});
    }

    #[test]
    fn trailing_flags_heredoc_receiver_leading_dot() {
        // RuboCop spec: trailing-style on a leading-dot heredoc receiver
        // emits an offense and the correction lifts the dot onto the
        // opener line.
        test::<DotPosition>()
            .with_options(&trailing())
            .expect_correction(
                indoc! {"
                <<~HEREDOC
                  something
                HEREDOC
                  .method_name
                  ^ Place the . on the previous line, together with the method call receiver.
            "},
                indoc! {"
                <<~HEREDOC.
                  something
                HEREDOC
                  method_name
            "},
            );
    }
}
