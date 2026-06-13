//! `Layout/EmptyLinesAroundAttributeAccessor` — keep a blank line after an
//! attribute accessor (`attr_reader`/`attr_writer`/`attr_accessor`/`attr`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundAttributeAccessor
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_send` for `RESTRICT_ON_SEND = %i[attr_reader
//!   attr_writer attr_accessor attr]`. An accessor (a bare, receiverless call
//!   to one of those methods with at least one argument) must be followed by a
//!   blank line unless the next statement is itself an attribute accessor, an
//!   allowed method (`AllowedMethods`, default `alias_method`/`public`/
//!   `protected`/`private`), or — when `AllowAliasSyntax` is true (default) —
//!   an `alias` statement. When the accessor is the last statement of an `if`
//!   body (`next_line_node` returns nil because the parent is an `if`), no
//!   offense is raised. The next-line-empty guard also accepts a `rubocop:`/
//!   `murphy:` enable directive comment immediately following the accessor.
//!   Autocorrect inserts a `\n` after the accessor's line (RuboCop's
//!   `corrector.insert_after(range_by_whole_lines, "\n")`); when an enable
//!   directive comment follows, the `\n` is inserted after that comment line
//!   to preserve directive semantics.
//!   Message: "Add an empty line after attribute accessor."
//!   Gaps (documented, not bypassed):
//!     - `next_line_node` uses Murphy's `right_sibling`, which mirrors
//!       parser-gem's `right_sibling`; the `node.parent.if_type?` guard is
//!       ported by checking the accessor's parent kind.
//!     - Murphy's translator leaves keyword `alias new old` as
//!       `NodeKind::Unknown` (subject-side `alias` translation is parser-only),
//!       so `allow_alias?` additionally recognises an `Unknown` next-sibling
//!       whose source begins with the `alias` keyword. `alias_method` (a send)
//!       is handled via `AllowedMethods` and is unaffected.
//! ```

use crate::cops::util::first_line_range;
use murphy_plugin_api::{
    CommentDirectiveKind, CopOptions, Cx, NodeId, NodeKind, Range, cop,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundAttributeAccessor;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowAliasSyntax",
        default = true,
        description = "Allow `alias` to follow an accessor without a blank line."
    )]
    pub allow_alias_syntax: bool,

    #[option(
        name = "AllowedMethods",
        default = ["alias_method", "public", "protected", "private"],
        description = "Method calls that may follow an accessor without a blank line."
    )]
    pub allowed_methods: Vec<String>,
}

const MSG: &str = "Add an empty line after attribute accessor.";

#[cop(
    name = "Layout/EmptyLinesAroundAttributeAccessor",
    description = "Keep blank lines around attribute accessors.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl EmptyLinesAroundAttributeAccessor {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `return unless node.attribute_accessor?`
        if !is_attribute_accessor(node, cx) {
            return;
        }

        let opts = cx.options_or_default::<Options>();
        let source = cx.source().as_bytes();

        // RuboCop's `node.last_line` is the accessor's final source line. The
        // "next line" is the physical line after it.
        let accessor_end = cx.range(node).end as usize;
        let Some(next_line_start) = line_after(source, accessor_end.saturating_sub(1)) else {
            // Accessor is on the file's last line — no following statement.
            return;
        };

        // `return if next_line_empty_or_enable_directive_comment?(node.last_line)`
        if next_line_is_blank(source, next_line_start) {
            return;
        }
        // An enable-directive comment on the next line, itself followed by a
        // blank line, is accepted.
        if let Some(after_directive) = enable_directive_next_line(cx, next_line_start)
            && next_line_is_blank(source, after_directive)
        {
            return;
        }

        // `next_line_node = next_line_node(node)`:
        // `return if node.parent.if_type?` then `node.right_sibling`.
        if cx
            .parent(node)
            .get()
            .is_some_and(|p| matches!(*cx.kind(p), NodeKind::If { .. }))
        {
            return;
        }
        let Some(next_node) = cx.right_sibling(node).get() else {
            return;
        };

        // `return unless require_empty_line?(next_line_node)`
        if !require_empty_line(next_node, &opts, cx) {
            return;
        }

        cx.emit_offense(first_line_range(node, cx), MSG, None);

        // Autocorrect: insert `\n` after the accessor's line. If an enable
        // directive comment follows, insert after that comment's line instead.
        let insert_at = match enable_directive_next_line(cx, next_line_start) {
            Some(after) => after,
            None => next_line_start,
        };
        cx.emit_edit(
            Range {
                start: insert_at as u32,
                end: insert_at as u32,
            },
            "\n",
        );
    }
}

/// `node.attribute_accessor?` — a receiverless call to `attr_reader`,
/// `attr_writer`, `attr_accessor`, or `attr` with at least one argument.
fn is_attribute_accessor(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.call_receiver(node).get().is_some() {
        return false;
    }
    let Some(name) = cx.method_name(node) else {
        return false;
    };
    matches!(
        name,
        "attr_reader" | "attr_writer" | "attr_accessor" | "attr"
    ) && !cx.call_arguments(node).is_empty()
}

/// `require_empty_line?(node)` — true when the following node is not an
/// allowed alias, another accessor, or an allowed method.
fn require_empty_line(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    !allow_alias(node, opts, cx) && !attribute_or_allowed_method(node, opts, cx)
}

/// `allow_alias?(node)` — `AllowAliasSyntax` is on and the node is an `alias`.
fn allow_alias(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if !opts.allow_alias_syntax {
        return false;
    }
    // RuboCop matches `node.alias_type?`. Murphy's translator lowers `alias`
    // syntax to `NodeKind::Alias` for some shapes but leaves the keyword form
    // as `NodeKind::Unknown` (subject-side `alias` translation is parser-only),
    // so also detect it by the leading `alias` keyword in the node's source.
    if matches!(*cx.kind(node), NodeKind::Alias { .. }) {
        return true;
    }
    matches!(*cx.kind(node), NodeKind::Unknown)
        && cx
            .raw_source(cx.range(node))
            .trim_start()
            .strip_prefix("alias")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// `attribute_or_allowed_method?(node)` — the node is a send that is itself an
/// accessor or an allowed method.
fn attribute_or_allowed_method(node: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    if is_attribute_accessor(node, cx) {
        return true;
    }
    cx.method_name(node)
        .is_some_and(|m| opts.allowed_methods.iter().any(|allowed| allowed == m))
}

/// The start offset of the physical line *after* the line containing `offset`,
/// or `None` if `offset` is on the last line of the source.
fn line_after(source: &[u8], offset: usize) -> Option<usize> {
    source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|pos| offset + pos + 1)
        .filter(|&start| start <= source.len())
}

/// Whether the line starting at `line_start` is blank, or `line_start` is
/// at/past EOF (`processed_source[line].nil?`).
///
/// RuboCop's `next_line_empty?` tests `processed_source[line].blank?`, which is
/// ActiveSupport's whitespace-aware `String#blank?` (`/\A[[:space:]]*\z/`) — a
/// whitespace-only line counts as blank here. This differs from the body cops'
/// literal `&:empty?` predicate, and is faithful to the upstream cop.
fn next_line_is_blank(source: &[u8], line_start: usize) -> bool {
    if line_start >= source.len() {
        return true;
    }
    let line_end = source[line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| line_start + pos);
    source[line_start..line_end]
        .iter()
        // Ruby's `blank?` (`/\A[[:space:]]*\z/`) treats POSIX space as blank:
        // space, `\t`, `\n`, vertical tab `\x0B`, form feed `\x0C`, `\r`.
        // Rust's `is_ascii_whitespace` omits the vertical tab, so match the
        // POSIX set explicitly for parity.
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r'))
}

/// If the line starting at `line_start` is a `rubocop:`/`murphy:` *enable*
/// directive comment, return the start of the line after it; else `None`.
fn enable_directive_next_line(cx: &Cx<'_>, line_start: usize) -> Option<usize> {
    let source = cx.source().as_bytes();
    let line_end = source[line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| line_start + pos);
    // Fast-path: a directive is a comment, so the line must contain `#`.
    // This skips the `comment_directives()` parse+alloc for the common case
    // (the line after an accessor is almost never a comment).
    if !source[line_start..line_end].contains(&b'#') {
        return None;
    }
    let on_line = |r: Range| {
        (r.start as usize) >= line_start && (r.start as usize) < line_end
    };
    let is_enable = cx.comment_directives().into_iter().any(|d| {
        d.kind == CommentDirectiveKind::Enable && on_line(d.comment_range)
    });
    if !is_enable {
        return None;
    }
    if line_end < source.len() {
        Some(line_end + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundAttributeAccessor;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn accepts_accessor_followed_by_blank_line() {
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :bar\n\n  def baz; end\nend\n");
    }

    #[test]
    fn flags_accessor_immediately_followed_by_method() {
        let src = "class Foo\n  attr_reader :bar\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAttributeAccessor>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Add an empty line after attribute accessor.");
    }

    #[test]
    fn accepts_consecutive_accessors() {
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :a\n  attr_writer :b\nend\n");
    }

    #[test]
    fn accepts_accessor_followed_by_allowed_method_private() {
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :a\n  private\nend\n");
    }

    #[test]
    fn accepts_accessor_followed_by_alias() {
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :a\n  alias b a\nend\n");
    }

    #[test]
    fn accepts_accessor_as_last_statement() {
        // No following sibling — nothing to require a blank line before.
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :a\nend\n");
    }

    #[test]
    fn ignores_non_accessor_send() {
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  foo :a\n  def baz; end\nend\n");
    }

    #[test]
    fn ignores_attr_with_no_arguments() {
        // `attr_reader` with no args is not an accessor declaration.
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader\n  def baz; end\nend\n");
    }

    #[test]
    fn corrects_by_inserting_blank_line() {
        let src = "class Foo\n  attr_reader :bar\n  def baz; end\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundAttributeAccessor>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "\n");
        // Insert at the start of the `  def baz; end` line (after the
        // accessor's line `\n`). "class Foo\n" = 0..10, "  attr_reader :bar\n"
        // = 10..29, so insertion point is byte 29.
        assert_eq!(edit.range.start, 29);
        assert_eq!(edit.range.end, 29);
    }

    #[test]
    fn accepts_accessor_followed_by_whitespace_only_line() {
        // RuboCop's `next_line_empty?` uses ActiveSupport `blank?`, so a line
        // of only spaces counts as the required blank line — no offense.
        test::<EmptyLinesAroundAttributeAccessor>()
            .expect_no_offenses("class Foo\n  attr_reader :bar\n  \n  def baz; end\nend\n");
    }

    #[test]
    fn accepts_accessor_followed_by_enable_directive_then_blank() {
        // `next_line_empty_or_enable_directive_comment?`: an enable directive
        // comment directly after the accessor, itself followed by a blank
        // line, satisfies the blank-line requirement.
        let src = "class Foo\n  attr_reader :bar\n  # rubocop:enable Layout/LineLength\n\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAttributeAccessor>(src);
        assert_eq!(offenses.len(), 0, "expected no offense, got {offenses:?}");
    }

    #[test]
    fn flags_accessor_followed_by_enable_directive_then_code() {
        // An enable directive comment NOT followed by a blank line does not
        // satisfy the requirement: the offense fires and the `\n` is inserted
        // after the directive comment line.
        let src = "class Foo\n  attr_reader :bar\n  # rubocop:enable Layout/LineLength\n  def baz; end\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundAttributeAccessor>(src);
        assert_eq!(result.offenses.len(), 1, "{:?}", result.offenses);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "\n");
        // Insert at the start of the `  def baz; end` line (after the directive
        // comment's `\n`), not after the accessor's line.
        let directive_line = "  # rubocop:enable Layout/LineLength\n";
        let insert_at = "class Foo\n  attr_reader :bar\n".len() + directive_line.len();
        assert_eq!(edit.range.start as usize, insert_at);
    }

    #[test]
    fn flags_attr_keyword() {
        let src = "class Foo\n  attr :bar\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAttributeAccessor>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundAttributeAccessor);
