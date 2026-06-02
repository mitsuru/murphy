//! `Style/Next` — use `next` to skip iteration instead of wrapping in a condition.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Next
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `if` conditions at the end of block/loop bodies and suggests
//!   using `next` to skip iteration instead.
//!
//!   Supported:
//!   - Block nodes (for enumerator methods like `each`, `map`, etc. and
//!     methods starting with `each_`)
//!   - Numblock nodes (same restriction)
//!   - `while`, `until`, `for` loops
//!   - EnforcedStyle: skip_modifier_ifs (default) — modifier ifs are allowed
//!   - EnforcedStyle: always — flag all trailing ifs including modifier form
//!   - MinBodyLength option (default: 3) — minimum lines in if body to flag
//!   - AllowConsecutiveConditionals option (default: false)
//!   - Autocorrect for modifier form: `body if cond` → `next unless cond\nbody`
//!   - Autocorrect for block form: `if cond\n  body\nend` → `next unless cond\nbody`
//!     with one level of indentation removed from body lines.
//!
//!   Gaps:
//!   - Heredoc detection in body lines (RuboCop skips heredoc lines during
//!     reindentation; Murphy uses a simple newline-based approach).
//!   - Nested offense reindentation tracking (RuboCop tracks @reindented_lines
//!     across nested corrections; Murphy corrects each in isolation which may
//!     require multiple fixpoint passes).
//!   - Itblock nodes are not handled (Ruby 3.4 `it` parameter blocks).
//! ```
//!
//! ## Matched shapes
//!
//! Enumerator block bodies or loop bodies that end with a block-form or
//! modifier-form `if`/`unless` (no `else`), whose then-branch does not
//! contain `break` or `return`.
//!
//! ## Examples
//!
//! ```ruby
//! # bad (EnforcedStyle: skip_modifier_ifs — default)
//! [1, 2].each do |a|
//!   if a == 1
//!     puts a
//!   end
//! end
//!
//! # good
//! [1, 2].each do |a|
//!   next unless a == 1
//!   puts a
//! end
//!
//! # allowed (modifier form, skip_modifier_ifs)
//! [1, 2].each do |a|
//!   puts a if a == 1
//! end
//! ```



use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "Use `next` to skip iteration.";

/// Exit types whose presence in the if branch prevents the cop from flagging.

/// Enumerator methods that trigger the cop for blocks.
const ENUMERATOR_METHODS: &[&str] = &[
    "collect",
    "collect_concat",
    "detect",
    "downto",
    "each",
    "find",
    "find_all",
    "find_index",
    "inject",
    "loop",
    "map",
    "map!",
    "reduce",
    "reject",
    "reject!",
    "reverse_each",
    "select",
    "select!",
    "times",
    "upto",
];

/// Stateless unit struct.
#[derive(Default)]
pub struct Next;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "skip_modifier_ifs")]
    SkipModifierIfs,
    #[option(value = "always")]
    Always,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "skip_modifier_ifs",
        description = "When `skip_modifier_ifs`, modifier-form `if` at the end of an iteration is allowed. When `always`, all trailing conditionals are flagged."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(
        name = "MinBodyLength",
        default = 3,
        description = "Minimum number of lines of the `if` body to trigger this cop for block-form conditionals."
    )]
    pub min_body_length: i64,
    #[option(
        name = "AllowConsecutiveConditionals",
        default = false,
        description = "When `true`, allows consecutive `if` blocks at the end of an iteration."
    )]
    pub allow_consecutive_conditionals: bool,
}

#[cop(
    name = "Style/Next",
    description = "Use `next` to skip iteration instead of a condition at the end.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl Next {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        // Only flag blocks over enumerator methods (not lambdas/procs).
        if cx.is_lambda(node) {
            return;
        }
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if !is_enumerator_method(method) {
            return;
        }
        let body = match *cx.kind(node) {
            NodeKind::Block { body, .. } => body,
            _ => return,
        };
        check_body(body, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if !is_enumerator_method(method) {
            return;
        }
        let body = match *cx.kind(node) {
            NodeKind::Numblock { body, .. } => body,
            _ => return,
        };
        check_body(body, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let body = match *cx.kind(node) {
            NodeKind::While { body, .. } => body,
            _ => return,
        };
        check_body(body, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let body = match *cx.kind(node) {
            NodeKind::Until { body, .. } => body,
            _ => return,
        };
        check_body(body, cx);
    }

    #[on_node(kind = "for")]
    fn check_for(&self, node: NodeId, cx: &Cx<'_>) {
        let body = match *cx.kind(node) {
            NodeKind::For { body, .. } => body,
            _ => return,
        };
        check_body(body, cx);
    }
}

/// Returns `true` if the method name is an enumerator method.
fn is_enumerator_method(name: &str) -> bool {
    ENUMERATOR_METHODS.contains(&name) || name.starts_with("each_")
}

/// Checks whether the body ends with a flaggable condition and reports offense.
fn check_body(body: OptNodeId, cx: &Cx<'_>) {
    let Some(body_id) = body.get() else {
        return;
    };

    let Some(offending_if) = trailing_if_node(body_id, cx) else {
        return;
    };

    let opts = cx.options_or_default::<Options>();

    if !is_simple_if_without_break(offending_if, &opts, cx) {
        return;
    }

    if opts.allow_consecutive_conditionals && is_consecutive_conditional(offending_if, cx) {
        return;
    }

    // Offense range: from the start of the `if` node to the end of the condition.
    let offense_range = offense_range(offending_if, cx);
    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect.
    if cx.is_modifier_form(offending_if) {
        autocorrect_modifier(offending_if, cx);
    } else {
        autocorrect_block(offending_if, cx);
    }
}

/// Returns the trailing `if` node from the body, if one exists.
///
/// - If `body` is itself an `If` node, returns it.
/// - If `body` is a `Begin` with an `If` as its last child, returns that child.
fn trailing_if_node(body: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(body) {
        NodeKind::If { .. } => Some(body),
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            let last = *children.last()?;
            if matches!(cx.kind(last), NodeKind::If { .. }) {
                Some(last)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns `true` if `node` is a simple `if`/`unless` without an `else` branch,
/// no nested if-else children, not blocked by `allowed_modifier_if`, and
/// the if-branch body is not a break/return.
fn is_simple_if_without_break(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    // Must be an If node.
    let NodeKind::If { then_: if_branch, else_: else_branch, .. } = *cx.kind(node) else {
        return false;
    };

    // Must not be a ternary.
    if cx.is_ternary(node) {
        return false;
    }

    // Must not have an explicit else branch (check by token, not by AST field,
    // because for `unless`, the body is in else_ and there's no else token).
    if cx.is_else(node) {
        return false;
    }

    // Must not have child `if` nodes with else (nested if-else).
    if has_nested_if_with_else(node, cx) {
        return false;
    }

    // Check allowed_modifier_if?
    if is_allowed_modifier_if(node, opts, cx) {
        return false;
    }

    // The "actual body" (the branch that runs) is in then_ for `if` and else_
    // for `unless` (due to Murphy's translator swap for unless).
    let actual_body = if cx.is_unless(node) { else_branch } else { if_branch };
    if has_exit_body(actual_body, cx) {
        return false;
    }

    true
}

/// Returns `true` if any direct `if` children of this node have an `else`.
fn has_nested_if_with_else(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return false;
    };

    for child_opt in [then_, else_] {
        if let Some(child) = child_opt.get() {
            if matches!(cx.kind(child), NodeKind::If { .. }) && cx.is_else(child) {
                return true;
            }
        }
    }
    if matches!(cx.kind(cond), NodeKind::If { .. }) && cx.is_else(cond) {
        return true;
    }
    false
}

/// Returns `true` if the condition is "allowed modifier if" — i.e., should not
/// be flagged based on the enforcement style and min body length.
fn is_allowed_modifier_if(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if cx.is_modifier_form(node) {
        // Modifier form: allowed when style is skip_modifier_ifs.
        opts.enforced_style == EnforcedStyle::SkipModifierIfs
    } else {
        // Block form: allowed when the if body doesn't meet min_body_length.
        !min_body_length_met(node, opts, cx)
    }
}

/// Returns `true` if the actual body has at least `MinBodyLength` lines.
fn min_body_length_met(if_node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    let NodeKind::If { then_, else_, .. } = *cx.kind(if_node) else {
        return false;
    };
    // For `if`, body is in then_; for `unless`, body is in else_.
    let body_opt = if cx.is_unless(if_node) { else_ } else { then_ };
    let Some(branch) = body_opt.get() else {
        return false;
    };
    let body_src = cx.raw_source(cx.range(branch));
    let line_count = body_src.matches('\n').count() + 1;
    line_count >= opts.min_body_length as usize
}

/// Returns `true` if the if-branch is a break or return.
fn has_exit_body(if_branch: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(branch) = if_branch.get() else {
        return false;
    };
    matches!(cx.kind(branch), NodeKind::Break(_) | NodeKind::Return(_))
}

/// Returns `true` if the offending if node is preceded by another if node
/// as a sibling (consecutive conditionals pattern).
fn is_consecutive_conditional(if_node: NodeId, cx: &Cx<'_>) -> bool {
    // The if node must be inside a Begin. Find its index among siblings.
    let Some(parent) = cx.ancestors(if_node).next() else {
        return false;
    };
    let NodeKind::Begin(list) = cx.kind(parent) else {
        return false;
    };
    let siblings = cx.list(*list);
    let Some(idx) = siblings.iter().position(|&id| id == if_node) else {
        return false;
    };
    if idx == 0 {
        return false;
    }
    matches!(cx.kind(siblings[idx - 1]), NodeKind::If { .. })
}

/// Compute the offense range: from start of the if node to end of its condition.
fn offense_range(if_node: NodeId, cx: &Cx<'_>) -> Range {
    let NodeKind::If { cond, .. } = *cx.kind(if_node) else {
        return cx.range(if_node);
    };
    let if_start = cx.range(if_node).start;
    let cond_end = cx.range(cond).end;
    Range {
        start: if_start,
        end: cond_end,
    }
}

/// Autocorrect modifier form: `body if cond` → `next unless cond\nbody`
///
/// Input:  `puts a if a == 1`  (modifier form, body is `puts a`)
/// Output: `next unless a == 1\nputs a`
fn autocorrect_modifier(if_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(if_node) else {
        return;
    };
    // For `if`, body is in then_; for `unless`, body is in else_
    // (Murphy's translator swaps branches for unless).
    let body_opt = if cx.is_unless(if_node) { else_ } else { then_ };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    let inverse_kw = cx.if_inverse_keyword(if_node);
    if inverse_kw.is_empty() {
        return;
    }

    let cond_src = cx.raw_source(cx.range(cond));
    let body_src = cx.raw_source(cx.range(body_id));

    // Compute the indentation of the current node for the body line.
    let if_range = cx.range(if_node);
    let indent = compute_line_indent(if_range.start, cx.source().as_bytes());

    // Skip autocorrect for multi-line conditions (same safety reason as
    // autocorrect_block: multi-line cond_src breaks the statement boundary).
    if cond_src.contains('\n') {
        return;
    }

    let replacement = format!(
        "next {inverse_kw} {cond_src}\n{indent}{body_src}"
    );
    cx.emit_edit(if_range, &replacement);
}

/// Autocorrect block form: `if cond\n  body\nend` → `next unless cond\nbody`
///
/// Input:
/// ```text
///   if a == 1
///     puts a
///   end
/// ```
/// Output:
/// ```text
///   next unless a == 1
///   puts a
/// ```
fn autocorrect_block(if_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(if_node) else {
        return;
    };
    // For `if`, body is in then_; for `unless`, body is in else_.
    let body_opt = if cx.is_unless(if_node) { else_ } else { then_ };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    let inverse_kw = cx.if_inverse_keyword(if_node);
    if inverse_kw.is_empty() {
        return;
    }

    let cond_src = cx.raw_source(cx.range(cond)).to_owned();
    let if_range = cx.range(if_node);
    let source = cx.source().as_bytes();

    // Skip autocorrect for multi-line conditions: embedding a multi-line
    // condition directly into `next unless <cond>` breaks the statement
    // boundary (the second line of the condition becomes the body).
    if cond_src.contains('\n') {
        return;
    }

    // Compute the indent of the `if` keyword line.
    let if_indent = compute_line_indent(if_range.start, source);
    // The body is indented by one extra level (typically 2 spaces).
    // We'll detect actual body indent and strip back to if_indent.
    let body_range = cx.range(body_id);
    let body_src = cx.raw_source(body_range);

    // Compute how much to dedent: body indent - if indent.
    let body_indent = compute_line_indent(body_range.start, source);
    let dedent_chars = body_indent.len().saturating_sub(if_indent.len());

    // Dedent body lines.
    let dedented_body = dedent_lines(body_src, dedent_chars);

    let replacement = format!("next {inverse_kw} {cond_src}\n{if_indent}{}", dedented_body.trim_end_matches('\n'));
    cx.emit_edit(if_range, &replacement);
}

/// Returns the leading whitespace of the line containing `offset`.
fn compute_line_indent<'a>(offset: u32, source: &'a [u8]) -> &'a str {
    let offset = offset as usize;
    // Find the start of the line.
    let line_start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    // Count leading spaces/tabs.
    let indent_end = source[line_start..]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    // Safety: source is valid UTF-8 (Murphy guarantees this).
    std::str::from_utf8(&source[line_start..line_start + indent_end]).unwrap_or("")
}

/// Remove `n` leading whitespace characters from each line of `s`.
fn dedent_lines(s: &str, n: usize) -> String {
    if n == 0 {
        return s.to_owned();
    }
    let mut result = String::with_capacity(s.len());
    for line in s.split('\n') {
        let stripped = strip_leading_whitespace(line, n);
        result.push_str(stripped);
        result.push('\n');
    }
    // Remove last added '\n' if original didn't end with '\n'.
    if !s.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Strip up to `n` leading whitespace characters from a line.
fn strip_leading_whitespace(s: &str, n: usize) -> &str {
    let mut count = 0;
    let mut chars = s.char_indices();
    while count < n {
        match chars.next() {
            Some((_, ' ')) | Some((_, '\t')) => count += 1,
            Some((i, _)) => return &s[i..],
            None => return "",
        }
    }
    match chars.next() {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Next, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- basic detection: block form (MinBodyLength: 3 default) -----

    #[test]
    fn flags_block_form_if_at_end_with_three_line_body() {
        test::<Next>().expect_offense(indoc! {"
            [1, 2].each do |a|
              if a == 1
              ^^^^^^^^^ Use `next` to skip iteration.
                puts a
                puts a
                puts a
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_block_form_if_with_short_body() {
        // Default MinBodyLength is 3; a 1-line body is not flagged.
        test::<Next>().expect_no_offenses(indoc! {"
            [1, 2].each do |a|
              if a == 1
                puts a
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_block_form_if_with_two_line_body() {
        test::<Next>().expect_no_offenses(indoc! {"
            [1, 2].each do |a|
              if a == 1
                puts a
                puts a
              end
            end
        "});
    }

    // ----- MinBodyLength option -----

    #[test]
    fn flags_with_custom_min_body_length_one() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                  ^^^^^^^^^ Use `next` to skip iteration.
                    puts a
                  end
                end
            "});
    }

    // ----- modifier form -----

    #[test]
    fn does_not_flag_modifier_if_in_skip_modifier_ifs_mode() {
        // Default style is skip_modifier_ifs.
        test::<Next>().expect_no_offenses(indoc! {"
            [1, 2].each do |a|
              puts a if a == 1
            end
        "});
    }

    #[test]
    fn flags_modifier_if_in_always_mode() {
        test::<Next>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Always,
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                [1, 2].each do |a|
                  puts a if a == 1
                  ^^^^^^^^^^^^^^^^ Use `next` to skip iteration.
                end
            "});
    }

    // ----- while/until/for -----

    #[test]
    fn flags_while_loop() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                while true
                  if a == 1
                  ^^^^^^^^^ Use `next` to skip iteration.
                    puts a
                  end
                end
            "});
    }

    #[test]
    fn flags_until_loop() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                until done
                  if a == 1
                  ^^^^^^^^^ Use `next` to skip iteration.
                    puts a
                  end
                end
            "});
    }

    #[test]
    fn flags_for_loop() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                for a in [1, 2]
                  if a == 1
                  ^^^^^^^^^ Use `next` to skip iteration.
                    puts a
                  end
                end
            "});
    }

    // ----- negative cases -----

    #[test]
    fn does_not_flag_if_with_else() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                    puts a
                  else
                    puts :other
                  end
                end
            "});
    }

    #[test]
    fn does_not_flag_if_with_break_in_body() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                    break
                  end
                end
            "});
    }

    #[test]
    fn does_not_flag_if_with_return_in_body() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                    return
                  end
                end
            "});
    }

    #[test]
    fn does_not_flag_non_enumerator_block() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                foo do |a|
                  if a == 1
                    puts a
                  end
                end
            "});
    }

    #[test]
    fn does_not_flag_ternary() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                [1, 2].each do |a|
                  a == 1 ? puts(a) : nil
                end
            "});
    }

    // ----- AllowConsecutiveConditionals -----

    #[test]
    fn does_not_flag_consecutive_conditionals_when_allowed() {
        test::<Next>()
            .with_options(&Options {
                allow_consecutive_conditionals: true,
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                    puts a
                  end
                  if a == 2
                    puts a
                  end
                end
            "});
    }

    #[test]
    fn flags_consecutive_conditionals_when_not_allowed() {
        // Default is AllowConsecutiveConditionals: false — last if IS flagged.
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                [1, 2].each do |a|
                  if a == 1
                    puts a
                  end
                  if a == 2
                  ^^^^^^^^^ Use `next` to skip iteration.
                    puts a
                  end
                end
            "});
    }

    // ----- autocorrect: modifier form -----

    #[test]
    fn corrects_modifier_if_to_next_unless() {
        test::<Next>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Always,
                min_body_length: 1,
                ..Options::default()
            })
            .expect_correction(
                indoc! {"
                    [1, 2].each do |a|
                      puts a if a == 1
                      ^^^^^^^^^^^^^^^^ Use `next` to skip iteration.
                    end
                "},
                indoc! {"
                    [1, 2].each do |a|
                      next unless a == 1
                      puts a
                    end
                "},
            );
    }

    #[test]
    fn corrects_modifier_unless_to_next_if() {
        test::<Next>()
            .with_options(&Options {
                enforced_style: EnforcedStyle::Always,
                min_body_length: 1,
                ..Options::default()
            })
            .expect_correction(
                indoc! {"
                    [1, 2].each do |a|
                      puts a unless a == 1
                      ^^^^^^^^^^^^^^^^^^^^ Use `next` to skip iteration.
                    end
                "},
                indoc! {"
                    [1, 2].each do |a|
                      next if a == 1
                      puts a
                    end
                "},
            );
    }

    // ----- autocorrect: block form -----

    #[test]
    fn corrects_block_if_to_next_unless() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_correction(
                indoc! {"
                    [1, 2].each do |a|
                      if a == 1
                      ^^^^^^^^^ Use `next` to skip iteration.
                        puts a
                      end
                    end
                "},
                indoc! {"
                    [1, 2].each do |a|
                      next unless a == 1
                      puts a
                    end
                "},
            );
    }

    #[test]
    fn corrects_block_unless_to_next_if() {
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_correction(
                indoc! {"
                    [1, 2].each do |a|
                      unless a == 1
                      ^^^^^^^^^^^^^ Use `next` to skip iteration.
                        puts a
                      end
                    end
                "},
                indoc! {"
                    [1, 2].each do |a|
                      next if a == 1
                      puts a
                    end
                "},
            );
    }


    // ----- multi-line condition: offense reported but no autocorrect -----

    #[test]
    fn does_not_autocorrect_multiline_condition() {
        // Autocorrect is skipped for multi-line conditions because embedding a
        // continuation line into `next unless <cond>` would break the statement
        // boundary. The offense IS reported (manually verifiable), but no edit
        // is emitted.
        test::<Next>()
            .with_options(&Options {
                min_body_length: 1,
                ..Options::default()
            })
            .expect_no_corrections(indoc! {"
                [1, 2].each do |a|
                  if a == 1 &&
                     a == 2
                    puts a
                  end
                end
            "});
    }

    // ----- idempotency -----

    #[test]
    fn corrected_next_unless_is_idempotent() {
        test::<Next>().expect_no_offenses(indoc! {"
            [1, 2].each do |a|
              next unless a == 1
              puts a
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(Next);
