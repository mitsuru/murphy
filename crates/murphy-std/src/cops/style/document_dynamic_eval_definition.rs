//! `Style/DocumentDynamicEvalDefinition` — requires comment documentation when
//! using eval-family methods with string interpolation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DocumentDynamicEvalDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags calls to `eval`, `class_eval`, `module_eval`, or `instance_eval`
//!   whose first argument is an interpolated string (dstr with `begin` children)
//!   when no inline comment documentation is present.
//!
//!   Inline comment check (`inline_comment_docs?`): every source line containing
//!   an interpolation (`#{...}`) must also contain a `# comment` (a `#` that is
//!   not a `#{`). This mirrors RuboCop's `COMMENT_REGEXP = /\s*#(?!{).*/`.
//!
//!   Offense is on the method selector (RuboCop's `node.loc.selector`).
//!
//!   Parity gaps vs RuboCop:
//!   - `comment_block_docs?`: for heredoc args, RuboCop also accepts a block of
//!     `#` comments either inside the heredoc or immediately preceding the call.
//!     Murphy does not implement this check — heredoc interpolations without
//!     inline comments will be flagged even if a block comment exists.
//!
//!   Enabled: `pending` (same as upstream). No autocorrect.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Add a comment block showing its appearance if interpolated.";

/// Stateless unit struct.
#[derive(Default)]
pub struct DocumentDynamicEvalDefinition;

#[cop(
    name = "Style/DocumentDynamicEvalDefinition",
    description = "When using `class_eval` (or other `eval`) with string interpolation, add a comment block showing its appearance if interpolated.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DocumentDynamicEvalDefinition {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver: _, method, args, .. } = *cx.kind(node) else {
            return;
        };

        // Only eval-family methods.
        let method_name = cx.symbol_str(method);
        if !matches!(
            method_name,
            "eval" | "class_eval" | "module_eval" | "instance_eval"
        ) {
            return;
        }

        // First argument must be a dstr with interpolation.
        let arg_list = cx.list(args);
        let Some(&first_arg) = arg_list.first() else {
            return;
        };

        let NodeKind::Dstr(dstr_list) = cx.kind(first_arg) else {
            return;
        };

        // Check if there are any `begin` (interpolation) children.
        let dstr_children = cx.list(*dstr_list);
        let has_interpolation = dstr_children
            .iter()
            .any(|&child| matches!(cx.kind(child), NodeKind::Begin(_)));
        if !has_interpolation {
            return;
        }

        // Check inline comment docs: every source line containing an interpolation
        // must have a trailing `#` comment.
        if inline_comment_docs(first_arg, cx) {
            return;
        }

        // Offense on the method selector (loc.name for Send).
        let selector = cx.selector(node);
        let offense_range = if selector == Range::ZERO {
            cx.range(node)
        } else {
            selector
        };
        cx.emit_offense(offense_range, MSG, None);
    }
}

/// Returns `true` if all source lines containing interpolations (`begin` nodes)
/// also contain a `# comment` that is not a `#{` (string interpolation opener).
///
/// Mirrors RuboCop's `inline_comment_docs?`:
/// ```ruby
/// node.each_child_node(:begin).all? do |begin_node|
///   source_line = processed_source.lines[begin_node.first_line - 1]
///   source_line.match?(COMMENT_REGEXP)  # /\s*#(?!{).*/
/// end
/// ```
fn inline_comment_docs(arg_node: NodeId, cx: &Cx<'_>) -> bool {
    let source = cx.source();
    let bytes = source.as_bytes();

    let NodeKind::Dstr(list) = cx.kind(arg_node) else {
        return false;
    };

    let dstr_children = cx.list(*list);
    for &child in dstr_children.iter() {
        if !matches!(cx.kind(child), NodeKind::Begin(_)) {
            continue;
        }

        // Find the source line containing this interpolation.
        let child_start = cx.range(child).start as usize;
        let line_start = bytes[..child_start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let line_end = bytes[child_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |p| child_start + p);
        let line = &bytes[line_start..line_end];

        // Check if the line contains `#` that is not immediately followed by `{`.
        if !line_has_comment(line) {
            return false;
        }
    }

    true
}

/// Returns `true` if `line` contains a `#` character that is not immediately
/// followed by `{` (i.e., a real comment, not a string interpolation opener).
fn line_has_comment(line: &[u8]) -> bool {
    let mut i = 0usize;
    while i < line.len() {
        if line[i] == b'#' {
            // Check it's not `#{`
            if i + 1 >= line.len() || line[i + 1] != b'{' {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::DocumentDynamicEvalDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_class_eval_dstr_without_comment() {
        test::<DocumentDynamicEvalDefinition>().expect_offense(indoc! {r#"
            class_eval("def #{unsafe_method}!(*params); end")
            ^^^^^^^^^^ Add a comment block showing its appearance if interpolated.
        "#});
    }

    #[test]
    fn accepts_class_eval_dstr_with_inline_comment() {
        test::<DocumentDynamicEvalDefinition>().expect_no_offenses(indoc! {r#"
            class_eval("def #{unsafe_method}!(*params); end # def capitalize!(*params); end")
        "#});
    }

    #[test]
    fn accepts_class_eval_plain_string() {
        // A plain string (no interpolation) should not be flagged.
        test::<DocumentDynamicEvalDefinition>()
            .expect_no_offenses("class_eval(\"def regular_method; end\")\n");
    }

    #[test]
    fn flags_eval_dstr_without_comment() {
        test::<DocumentDynamicEvalDefinition>().expect_offense(indoc! {r#"
            eval("def #{method_name}; end")
            ^^^^ Add a comment block showing its appearance if interpolated.
        "#});
    }

    #[test]
    fn flags_module_eval_dstr_without_comment() {
        test::<DocumentDynamicEvalDefinition>().expect_offense(indoc! {r#"
            module_eval("def #{method_name}; end")
            ^^^^^^^^^^^ Add a comment block showing its appearance if interpolated.
        "#});
    }

    #[test]
    fn flags_instance_eval_dstr_without_comment() {
        test::<DocumentDynamicEvalDefinition>().expect_offense(indoc! {r#"
            instance_eval("def #{method_name}; end")
            ^^^^^^^^^^^^^ Add a comment block showing its appearance if interpolated.
        "#});
    }

    #[test]
    fn accepts_non_eval_method() {
        test::<DocumentDynamicEvalDefinition>()
            .expect_no_offenses("send_message(\"def #{method_name}; end\")\n");
    }
}

murphy_plugin_api::submit_cop!(DocumentDynamicEvalDefinition);
