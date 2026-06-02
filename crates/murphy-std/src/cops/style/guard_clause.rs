//! `Style/GuardClause` — use a guard clause instead of wrapping code in a
//! conditional expression.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/GuardClause
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects the two canonical shapes:
//!   1. Methods/blocks (define_method/define_singleton_method) whose body ends
//!      with a plain `if` (no else) — suggests `return unless cond`.
//!   2. Top-level `if` nodes whose if-branch or else-branch is already a guard
//!      clause — suggests converting the whole if to a modifier guard.
//!
//!   Supported:
//!   - def / defs / define_method / define_singleton_method scope
//!   - MinBodyLength option (default: 1)
//!   - AllowConsecutiveConditionals option (default: false)
//!   - Offense on the `if`/`unless` keyword token
//!   - Autocorrect for simple (single-line condition, no and/or guard) cases
//!
//!   Gaps vs RuboCop:
//!   - Heredoc argument detection in autocorrect (skipped; offense still reported)
//!   - `foo || raise('exception')` and-or guard_clause autocorrect skipped
//!     (RuboCop's own NOTE says this is incomplete)
//!   - `assigned_lvar_used_in_if_branch?` guard (rare edge case)
//!   - itblock nodes (Ruby 3.4 `it` parameter blocks)
//!   - `then` keyword removal in inline `if cond then body end` forms
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! def test
//!   if something
//!     work
//!   end
//! end
//!
//! # good
//! def test
//!   return unless something
//!   work
//! end
//!
//! # bad
//! if something
//!   raise 'exception'
//! else
//!   ok
//! end
//!
//! # good
//! raise 'exception' if something
//! ok
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Use a guard clause (`%s`) instead of wrapping the code inside a conditional expression.";

/// Stateless unit struct.
#[derive(Default)]
pub struct GuardClause;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "MinBodyLength",
        default = 1,
        description = "Minimum number of lines in the `if` body to trigger this cop."
    )]
    pub min_body_length: i64,
    #[option(
        name = "AllowConsecutiveConditionals",
        default = false,
        description = "When `true`, allows consecutive `if` blocks without triggering the cop."
    )]
    pub allow_consecutive_conditionals: bool,
}

#[cop(
    name = "Style/GuardClause",
    description = "Use a guard clause instead of wrapping code inside a conditional expression.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl GuardClause {
    /// Checks `def` method bodies for ending `if` without else.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(body) = cx.def_body(node).get() else {
            return;
        };
        check_ending_body(body, cx);
    }

    /// Checks `defs` singleton method bodies for ending `if` without else.
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(body) = cx.def_body(node).get() else {
            return;
        };
        check_ending_body(body, cx);
    }

    /// Checks blocks on `define_method` / `define_singleton_method`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_define_method_block(node, cx) {
            return;
        }
        let body = match *cx.kind(node) {
            NodeKind::Block { body, .. } => body,
            _ => return,
        };
        let Some(body_id) = body.get() else {
            return;
        };
        check_ending_body(body_id, cx);
    }

    /// Checks blocks on `define_method` / `define_singleton_method` (numblock).
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_define_method_block(node, cx) {
            return;
        }
        let body = match *cx.kind(node) {
            NodeKind::Numblock { body, .. } => body,
            _ => return,
        };
        let Some(body_id) = body.get() else {
            return;
        };
        check_ending_body(body_id, cx);
    }

    /// Checks `if` nodes with an else branch where one branch is a guard clause.
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        if accepted_form_shape2(node, cx) {
            return;
        }

        let if_branch = cx.if_then_branch(node);
        let else_branch = cx.if_else_branch(node);

        // Determine which branch is the guard clause.
        let (guard_id, kw, branch_side) =
            if let Some(g) = if_branch.get().filter(|&b| cx.is_guard_clause(b)) {
                (g, cx.if_keyword(node).to_owned(), GuardSide::If)
            } else if let Some(g) = else_branch.get().filter(|&b| cx.is_guard_clause(b)) {
                (g, cx.if_inverse_keyword(node).to_owned(), GuardSide::Else)
            } else {
                return;
            };

        let is_and_or = matches!(
            cx.kind(guard_id),
            NodeKind::And { .. } | NodeKind::Or { .. }
        );

        let guard_src = cx.raw_source(cx.range(guard_id)).to_owned();
        let cond = match cx.if_condition(node).get() {
            Some(c) => c,
            None => return,
        };
        let cond_src = cx.raw_source(cx.range(cond));
        let example = format!("{guard_src} {kw} {cond_src}");

        let msg = MSG.replacen("%s", &example, 1);
        let keyword_loc = cx.if_keyword_loc(node);
        if keyword_loc == Range::ZERO {
            return;
        }
        cx.emit_offense(keyword_loc, &msg, None);

        // Autocorrect: skip for and/or guards and multiline conditions.
        if !is_and_or && cx.is_single_line(cond) {
            autocorrect_shape2(node, cond, &guard_src, &kw, branch_side, cx);
        }
    }
}

/// Which branch holds the guard clause.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GuardSide {
    If,
    Else,
}

/// Returns `true` if this is a define_method/define_singleton_method block.
fn is_define_method_block(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.method_name(node),
        Some("define_method" | "define_singleton_method")
    )
}

/// Check the body ending with an `if` node — Shape 1.
fn check_ending_body(body: NodeId, cx: &Cx<'_>) {
    match cx.kind(body) {
        NodeKind::If { .. } => {
            check_ending_if(body, cx);
        }
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            if let Some(&last) = children
                .last()
                .filter(|&&n| matches!(cx.kind(n), NodeKind::If { .. }))
            {
                check_ending_if(last, cx);
            }
        }
        _ => {}
    }
}

/// Check a single trailing `if` node in a def body — Shape 1.
fn check_ending_if(node: NodeId, cx: &Cx<'_>) {
    if accepted_form_shape1(node, cx) {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    if !min_body_length_met(node, &opts, cx) {
        return;
    }

    if opts.allow_consecutive_conditionals && consecutive_conditionals(node, cx) {
        return;
    }

    let keyword_loc = cx.if_keyword_loc(node);
    if keyword_loc == Range::ZERO {
        return;
    }

    let inv_kw = cx.if_inverse_keyword(node);
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return,
    };
    let cond_src = cx.raw_source(cx.range(cond));
    let example = format!("return {inv_kw} {cond_src}");
    let msg = MSG.replacen("%s", &example, 1);

    cx.emit_offense(keyword_loc, &msg, None);

    // Autocorrect for block-form with single-line condition.
    if !cx.is_modifier_form(node) && cx.is_single_line(cond) {
        autocorrect_shape1(node, cond, inv_kw, cx);
    }

    // Recurse into the if-branch body for nested ifs.
    if let Some(branch) = cx.if_then_branch(node).get() {
        check_ending_body(branch, cx);
    }
}

/// `accepted_form` for Shape 1 (ending if, no else).
fn accepted_form_shape1(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_modifier_form(node) || cx.is_ternary(node) || cx.is_elsif(node) {
        return true;
    }
    // Must not have an else branch.
    if cx.is_else(node) {
        return true;
    }
    // Condition must not be multiline.
    if cx
        .if_condition(node)
        .get()
        .is_some_and(|cond| cx.is_multiline(cond))
    {
        return true;
    }
    // Parent must not be an assignment.
    if cx
        .parent(node)
        .get()
        .is_some_and(|parent| cx.is_assignment(parent))
    {
        return true;
    }
    false
}

/// `accepted_form` for Shape 2 (if with else, guard in branch).
fn accepted_form_shape2(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_modifier_form(node) || cx.is_ternary(node) || cx.is_elsif(node) {
        return true;
    }
    // Must have an else branch.
    if !cx.is_else(node) {
        return true;
    }
    // Must not be an elsif chain in the else branch.
    if cx
        .if_else_branch(node)
        .get()
        .is_some_and(|else_id| cx.is_elsif(else_id))
    {
        return true;
    }
    // Condition must not be multiline.
    if cx
        .if_condition(node)
        .get()
        .is_some_and(|cond| cx.is_multiline(cond))
    {
        return true;
    }
    // Parent must not be an assignment.
    if cx
        .parent(node)
        .get()
        .is_some_and(|parent| cx.is_assignment(parent))
    {
        return true;
    }
    false
}

/// Returns `true` if the if body meets `MinBodyLength`.
fn min_body_length_met(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    // For `unless`, body is in else_; for `if`, body is in then_.
    let body_opt = if cx.is_unless(node) {
        cx.if_else_branch(node)
    } else {
        cx.if_then_branch(node)
    };
    let Some(branch) = body_opt.get() else {
        return false;
    };
    let body_src = cx.raw_source(cx.range(branch));
    let line_count = body_src.matches('\n').count() + 1;
    line_count >= opts.min_body_length as usize
}

/// Returns `true` if this if is immediately preceded by another if without else.
fn consecutive_conditionals(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let NodeKind::Begin(list) = cx.kind(parent) else {
        return false;
    };
    let siblings = cx.list(*list);
    let Some(idx) = siblings.iter().position(|&id| id == node) else {
        return false;
    };
    if idx == 0 {
        return false;
    }
    let prev = siblings[idx - 1];
    matches!(cx.kind(prev), NodeKind::If { .. }) && !cx.is_else(prev)
}

/// Autocorrect Shape 1: block `if cond\n  body\nend` → `return inv_kw cond\nbody`
fn autocorrect_shape1(node: NodeId, cond: NodeId, inv_kw: &str, cx: &Cx<'_>) {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();

    let body_opt = if cx.is_unless(node) {
        cx.if_else_branch(node)
    } else {
        cx.if_then_branch(node)
    };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    let cond_src = cx.raw_source(cx.range(cond));
    let body_range = cx.range(body_id);
    let body_src = cx.raw_source(body_range);
    let if_indent = line_indent(node_range.start, source);
    let body_indent = line_indent(body_range.start, source);
    let dedent = body_indent.len().saturating_sub(if_indent.len());
    let dedented_body = dedent_lines(body_src, dedent);

    let replacement = format!(
        "return {inv_kw} {cond_src}\n{if_indent}{}",
        dedented_body.trim_end_matches('\n')
    );
    cx.emit_edit(node_range, &replacement);
}

/// Autocorrect Shape 2: `if cond; guard; else; ok; end` → `guard kw cond\nok`
fn autocorrect_shape2(
    node: NodeId,
    cond: NodeId,
    guard_src: &str,
    kw: &str,
    branch_side: GuardSide,
    cx: &Cx<'_>,
) {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let cond_src = cx.raw_source(cx.range(cond));
    let if_indent = line_indent(node_range.start, source);

    let keep_branch_opt = match branch_side {
        GuardSide::If => cx.if_else_branch(node),
        GuardSide::Else => cx.if_then_branch(node),
    };

    if let Some(keep_id) = keep_branch_opt.get() {
        let keep_range = cx.range(keep_id);
        let keep_src = cx.raw_source(keep_range);
        let keep_indent = line_indent(keep_range.start, source);
        let dedent = keep_indent.len().saturating_sub(if_indent.len());
        let dedented_keep = dedent_lines(keep_src, dedent);
        let replacement = format!(
            "{guard_src} {kw} {cond_src}\n{if_indent}{}",
            dedented_keep.trim_end_matches('\n')
        );
        cx.emit_edit(node_range, &replacement);
    } else {
        let replacement = format!("{guard_src} {kw} {cond_src}");
        cx.emit_edit(node_range, &replacement);
    }
}

/// Returns the leading whitespace of the line containing `offset`.
fn line_indent(offset: u32, source: &[u8]) -> &str {
    let offset = offset as usize;
    let line_start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let indent_end = source[line_start..]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    std::str::from_utf8(&source[line_start..line_start + indent_end]).unwrap_or("")
}

/// Remove `n` leading whitespace characters from each line.
fn dedent_lines(s: &str, n: usize) -> String {
    if n == 0 {
        return s.to_owned();
    }
    let mut result = String::with_capacity(s.len());
    for line in s.split('\n') {
        let stripped = strip_leading_ws(line, n);
        result.push_str(stripped);
        result.push('\n');
    }
    if !s.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Strip up to `n` leading whitespace characters from a line.
fn strip_leading_ws(s: &str, n: usize) -> &str {
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
    use super::{GuardClause, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- Shape 1: ending if in def body ----

    #[test]
    fn flags_ending_if_in_def_body() {
        test::<GuardClause>().expect_offense(indoc! {"
            def test
              if something
              ^^ Use a guard clause (`return unless something`) instead of wrapping the code inside a conditional expression.
                work
              end
            end
        "});
    }

    #[test]
    fn flags_ending_unless_in_def_body() {
        test::<GuardClause>().expect_offense(indoc! {"
            def test
              unless something
              ^^^^^^ Use a guard clause (`return if something`) instead of wrapping the code inside a conditional expression.
                work
              end
            end
        "});
    }

    #[test]
    fn flags_ending_if_in_begin_body() {
        test::<GuardClause>().expect_offense(indoc! {"
            def test
              work
              if something
              ^^ Use a guard clause (`return unless something`) instead of wrapping the code inside a conditional expression.
                more_work
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_if_with_else() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
              if something
                work
              else
                other
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_modifier_if_in_def() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
              work if something
            end
        "});
    }

    #[test]
    fn does_not_flag_ternary_in_def() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
              something ? work : other
            end
        "});
    }

    #[test]
    fn does_not_flag_empty_def() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
            end
        "});
    }

    #[test]
    fn does_not_flag_short_body_when_min_body_length_2() {
        test::<GuardClause>()
            .with_options(&Options {
                min_body_length: 2,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                def test
                  if something
                    work
                  end
                end
            "});
    }

    #[test]
    fn flags_body_meeting_min_body_length_2() {
        test::<GuardClause>()
            .with_options(&Options {
                min_body_length: 2,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                def test
                  if something
                  ^^ Use a guard clause (`return unless something`) instead of wrapping the code inside a conditional expression.
                    work
                    more
                  end
                end
            "});
    }

    #[test]
    fn does_not_flag_consecutive_when_allowed() {
        test::<GuardClause>()
            .with_options(&Options {
                allow_consecutive_conditionals: true,
                ..Options::default()
            })
            .expect_no_offenses(indoc! {"
                def test
                  if foo
                    work
                  end
                  if bar
                    work
                  end
                end
            "});
    }

    #[test]
    fn flags_second_consecutive_when_not_allowed() {
        test::<GuardClause>()
            .with_options(&Options {
                allow_consecutive_conditionals: false,
                ..Options::default()
            })
            .expect_offense(indoc! {"
                def test
                  if foo
                    work
                  end
                  if bar
                  ^^ Use a guard clause (`return unless bar`) instead of wrapping the code inside a conditional expression.
                    work
                  end
                end
            "});
    }

    #[test]
    fn flags_define_method_block() {
        test::<GuardClause>().expect_offense(indoc! {"
            define_method(:test) do
              if something
              ^^ Use a guard clause (`return unless something`) instead of wrapping the code inside a conditional expression.
                work
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_regular_block() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            [1, 2].each do |a|
              if a == 1
                work
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_multiline_condition() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
              if foo &&
                 bar
                work
              end
            end
        "});
    }

    // ---- Shape 2: if-else where one branch is a guard clause ----

    #[test]
    fn flags_if_else_where_if_branch_is_raise() {
        test::<GuardClause>().expect_offense(indoc! {"
            if something
            ^^ Use a guard clause (`raise 'exception' if something`) instead of wrapping the code inside a conditional expression.
              raise 'exception'
            else
              ok
            end
        "});
    }

    #[test]
    fn flags_if_else_where_else_branch_is_raise() {
        test::<GuardClause>().expect_offense(indoc! {"
            if something
            ^^ Use a guard clause (`raise 'exception' unless something`) instead of wrapping the code inside a conditional expression.
              ok
            else
              raise 'exception'
            end
        "});
    }

    #[test]
    fn does_not_flag_if_without_else_standalone() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            if something
              work
            end
        "});
    }

    #[test]
    fn does_not_flag_if_else_where_neither_branch_is_guard() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            if something
              work
            else
              other_work
            end
        "});
    }

    // ---- Autocorrect: Shape 1 ----

    #[test]
    fn corrects_ending_if_in_def_to_return_unless() {
        test::<GuardClause>().expect_correction(
            indoc! {"
                def test
                  if something
                  ^^ Use a guard clause (`return unless something`) instead of wrapping the code inside a conditional expression.
                    work
                  end
                end
            "},
            indoc! {"
                def test
                  return unless something
                  work
                end
            "},
        );
    }

    #[test]
    fn corrects_ending_unless_in_def_to_return_if() {
        test::<GuardClause>().expect_correction(
            indoc! {"
                def test
                  unless something
                  ^^^^^^ Use a guard clause (`return if something`) instead of wrapping the code inside a conditional expression.
                    work
                  end
                end
            "},
            indoc! {"
                def test
                  return if something
                  work
                end
            "},
        );
    }

    // ---- Autocorrect: Shape 2 ----

    #[test]
    fn corrects_if_else_with_guard_if_branch() {
        test::<GuardClause>().expect_correction(
            indoc! {"
                if something
                ^^ Use a guard clause (`raise 'exception' if something`) instead of wrapping the code inside a conditional expression.
                  raise 'exception'
                else
                  ok
                end
            "},
            indoc! {"
                raise 'exception' if something
                ok
            "},
        );
    }

    // ---- Idempotency ----

    #[test]
    fn corrected_guard_clause_is_idempotent() {
        test::<GuardClause>().expect_no_offenses(indoc! {"
            def test
              return unless something
              work
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(GuardClause);
