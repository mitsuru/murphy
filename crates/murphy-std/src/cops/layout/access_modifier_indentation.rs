//! `Layout/AccessModifierIndentation` — checks the indentation of bare
//! access-modifier macros (`public`/`protected`/`private`/`module_function`)
//! inside class/module/sclass/block bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/AccessModifierIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `on_class`/`on_module`/`on_sclass`/`on_block` plus `check_body`,
//!   `check_modifier`, `expected_indent_offset`, and `message`. Fires only when
//!   the body is a multi-statement `begin` (RuboCop's `node.body&.begin_type?`),
//!   then inspects each *direct child* `send` that is a bare access modifier,
//!   skipping any modifier on the same line as the opening keyword.
//!
//!   The offset is measured against the body's terminator (`node.loc.end` —
//!   the `end` keyword or `}` brace) exactly like RuboCop's
//!   `column_offset_between(modifier.source_range, end_range)`:
//!   `modifier_column - terminator_column`. The expected offset is `0` for
//!   `EnforcedStyle: outdent` and the configured indentation width (default 2)
//!   for `EnforcedStyle: indent` (the default).
//!
//!   Both `EnforcedStyle` values and the `IndentationWidth` option are
//!   modelled. The message matches RuboCop verbatim:
//!   `"Indent access modifiers like \`private\`."` /
//!   `"Outdent access modifiers like \`private\`."`.
//!
//!   Columns use `.chars().count()` from the line start so multi-byte source
//!   aligns by visible column (RuboCop measures byte columns via
//!   `column_offset_between`; this differs only for non-ASCII indentation,
//!   a known minor gap shared with the other Layout cops).
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop shifts the modifier to the
//!   expected column via `AlignmentCorrector`; the detect-only port ships
//!   without it, matching the alignment-cop precedent.
//! ```
//!
//! ## Matched shapes
//!
//! `class`/`module`/`sclass`/`block` nodes whose body is a multi-statement
//! `begin` containing a bare access modifier indented other than the expected
//! offset relative to the body's terminator.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct AccessModifierIndentation;

/// Options for [`AccessModifierIndentation`]. `EnforcedStyle` matches RuboCop
/// verbatim; the default is `indent`. `IndentationWidth` overrides the
/// indentation width used by the `indent` style (default 2, mirroring
/// `Layout/IndentationWidth`).
#[derive(CopOptions)]
pub struct AccessModifierIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "indent",
        description = "Whether to `indent` access modifiers one level in, or `outdent` them to the body keyword."
    )]
    pub enforced_style: AccessModifierIndentationStyle,
    #[option(
        name = "IndentationWidth",
        default = 0,
        description = "Indentation width for the `indent` style (0 = use the default of 2)."
    )]
    pub indentation_width: i64,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum AccessModifierIndentationStyle {
    /// Indent modifiers one level past the body keyword.
    #[option(value = "indent")]
    Indent,
    /// Align modifiers with the body keyword (zero offset from `end`).
    #[option(value = "outdent")]
    Outdent,
}

#[cop(
    name = "Layout/AccessModifierIndentation",
    description = "Check indentation of private/protected visibility modifiers.",
    default_severity = "warning",
    default_enabled = true,
    options = AccessModifierIndentationOptions,
)]
impl AccessModifierIndentation {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }
}

fn check_body(node: NodeId, cx: &Cx<'_>) {
    // `return unless node.body&.begin_type?`.
    let Some(body) = body_of(node, cx) else {
        return;
    };
    let NodeKind::Begin(list) = cx.kind(body) else {
        return;
    };

    // `end_range = node.loc.end`. For class/module/sclass and `do…end` blocks
    // this is the `end` keyword; for `{}` blocks it is the closing brace.
    let Some(terminator) = terminator_range(node, cx) else {
        return;
    };
    let src = cx.source();
    let terminator_column = column_of(terminator.start, src);

    let opts = cx.options_or_default::<AccessModifierIndentationOptions>();
    let expected = expected_indent_offset(&opts);

    let node_line = line_of(cx.range(node).start, src);

    for &child in cx.list(*list) {
        if !cx.is_bare_access_modifier(child) {
            continue;
        }
        let modifier_start = cx.range(child).start;
        // `next if same_line?(node, modifier)`.
        if line_of(modifier_start, src) == node_line {
            continue;
        }

        // `column_offset_between(modifier, end_range)` = modifier col − end col.
        let offset = column_of(modifier_start, src) as isize - terminator_column as isize;
        if offset == expected as isize {
            continue;
        }

        let modifier = cx.raw_source(cx.selector(child));
        let msg = message(opts.enforced_style, modifier);
        cx.emit_offense(cx.range(child), &msg, None);
    }
}

/// The body node of a `class`/`module`/`sclass`/`block`, if present.
fn body_of(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Class { body, .. }
        | NodeKind::Module { body, .. }
        | NodeKind::Sclass { body, .. }
        | NodeKind::Block { body, .. } => body.get(),
        _ => None,
    }
}

/// RuboCop's `node.loc.end`: the `end` keyword range, or the closing `}` for a
/// brace block. Returns `None` if neither can be located.
fn terminator_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let end_kw = cx.loc(node).end_keyword();
    if end_kw != Range::ZERO {
        return Some(end_kw);
    }
    // Brace block: the terminator is the final `}` byte of the node range.
    let r = cx.range(node);
    if cx.source().as_bytes().get(r.end as usize - 1) == Some(&b'}') {
        return Some(Range {
            start: r.end - 1,
            end: r.end,
        });
    }
    None
}

/// `expected_indent_offset`: 0 for `outdent`, the configured width for `indent`.
fn expected_indent_offset(opts: &AccessModifierIndentationOptions) -> usize {
    match opts.enforced_style {
        AccessModifierIndentationStyle::Outdent => 0,
        AccessModifierIndentationStyle::Indent => {
            if opts.indentation_width > 0 {
                opts.indentation_width as usize
            } else {
                2
            }
        }
    }
}

fn message(style: AccessModifierIndentationStyle, modifier: &str) -> String {
    let verb = match style {
        AccessModifierIndentationStyle::Indent => "Indent",
        AccessModifierIndentationStyle::Outdent => "Outdent",
    };
    format!("{verb} access modifiers like `{modifier}`.")
}

/// Visible column (0-based, char count) of a byte offset within its line.
fn column_of(offset: u32, src: &str) -> usize {
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// 1-based line number of `offset`.
fn line_of(offset: u32, src: &str) -> usize {
    src[..offset as usize].bytes().filter(|&b| b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::{AccessModifierIndentation, AccessModifierIndentationOptions, AccessModifierIndentationStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn outdent() -> AccessModifierIndentationOptions {
        AccessModifierIndentationOptions {
            enforced_style: AccessModifierIndentationStyle::Outdent,
            indentation_width: 0,
        }
    }

    // indent (default) ----------------------------------------------------

    #[test]
    fn accepts_indented_modifier() {
        test::<AccessModifierIndentation>().expect_no_offenses(indoc! {"
            class Foo
              def a; end

              private

              def b; end
            end
        "});
    }

    #[test]
    fn flags_outdented_modifier() {
        test::<AccessModifierIndentation>().expect_offense(indoc! {"
            class Foo
              def a; end

            private
            ^^^^^^^ Indent access modifiers like `private`.

              def b; end
            end
        "});
    }

    #[test]
    fn flags_over_indented_modifier() {
        test::<AccessModifierIndentation>().expect_offense(indoc! {"
            class Foo
              def a; end

                private
                ^^^^^^^ Indent access modifiers like `private`.

              def b; end
            end
        "});
    }

    #[test]
    fn checks_modules() {
        test::<AccessModifierIndentation>().expect_offense(indoc! {"
            module Foo
              def a; end

            protected
            ^^^^^^^^^ Indent access modifiers like `protected`.

              def b; end
            end
        "});
    }

    #[test]
    fn checks_singleton_class() {
        test::<AccessModifierIndentation>().expect_offense(indoc! {"
            class Foo
              class << self
                def a; end

              private
              ^^^^^^^ Indent access modifiers like `private`.

                def b; end
              end
            end
        "});
    }

    #[test]
    fn checks_blocks() {
        test::<AccessModifierIndentation>().expect_offense(indoc! {"
            Struct.new do
              def a; end

            private
            ^^^^^^^ Indent access modifiers like `private`.

              def b; end
            end
        "});
    }

    #[test]
    fn ignores_single_statement_body() {
        // A lone modifier (body is not a `begin`) never fires.
        test::<AccessModifierIndentation>().expect_no_offenses(indoc! {"
            class Foo
              private
            end
        "});
    }

    #[test]
    fn ignores_modifier_with_argument() {
        test::<AccessModifierIndentation>().expect_no_offenses(indoc! {"
            class Foo
              def a; end

            private :a

              def b; end
            end
        "});
    }

    // outdent -------------------------------------------------------------

    #[test]
    fn outdent_accepts_outdented_modifier() {
        test::<AccessModifierIndentation>()
            .with_options(&outdent())
            .expect_no_offenses(indoc! {"
                class Foo
                  def a; end

                private

                  def b; end
                end
            "});
    }

    #[test]
    fn outdent_flags_indented_modifier() {
        test::<AccessModifierIndentation>()
            .with_options(&outdent())
            .expect_offense(indoc! {"
                class Foo
                  def a; end

                  private
                  ^^^^^^^ Outdent access modifiers like `private`.

                  def b; end
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(AccessModifierIndentation);
