//! `Layout/ArgumentAlignment` — the arguments of a multi-line method call must
//! be aligned.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ArgumentAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `on_send`/`on_csend` plus the `Alignment#check_alignment` /
//!   `each_bad_alignment` spine, mirroring `Layout/ArrayAlignment`. Fires when a
//!   call has multiple arguments (`args.size >= 2`, or a single braceless hash
//!   first argument with two or more pairs), excluding `[]=` calls. Each
//!   flattened item that *begins its own line* must sit at the base column.
//!
//!   Argument flattening matches RuboCop verbatim:
//!     - with_first_argument (default): if the first argument is a braceless
//!       hash, the items are its pairs; otherwise the items are all arguments.
//!     - with_fixed_indentation: items are `args[0..-2]` plus the last
//!       argument's pairs when it is a braceless hash, else the last argument.
//!
//!   Base column:
//!     - with_first_argument: the first item's display column.
//!     - with_fixed_indentation (or no first item): the indentation of the
//!       call's method-name line (the opening `(` line for `l.(…)`-style calls
//!       with no selector) plus the configured indentation width (default 2).
//!
//!   Columns use `.chars().count()` from the line start (RuboCop's
//!   `display_column`, modulo full Unicode east-asian-width handling — a known
//!   minor gap shared with the other alignment cops).
//!
//!   Single-surface ABI blocker (intentionally NOT bypassed):
//!   RuboCop's `autocorrect_incompatible_with_other_cops?` reads
//!   `Layout/HashAlignment`'s `EnforcedHashRocketStyle`/`EnforcedColonStyle`
//!   to suppress autocorrect when a separator-alignment style is configured.
//!   Murphy's per-cop `CopOptions` surface cannot see a sibling cop's config,
//!   so this cop assumes the default (no separator alignment) and always runs —
//!   the common case. Because the detect-only port emits no autocorrect, the
//!   suppression has no observable effect on which offenses fire.
//!
//!   Autocorrect: not implemented (v1 gap), matching the
//!   `Layout/ArrayAlignment` / `Layout/ParameterAlignment` precedent.
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes with multiple arguments (other than `[]=`) where a later
//! flattened item begins its own line at a column other than the base column.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const ALIGN_PARAMS_MSG: &str =
    "Align the arguments of a method call if they span more than one line.";
const FIXED_INDENT_MSG: &str = "Use one level of indentation for arguments \
    following the first line of a multi-line method call.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ArgumentAlignment;

/// Options for [`ArgumentAlignment`]. `EnforcedStyle` matches RuboCop verbatim;
/// the default is `with_first_argument`. `IndentationWidth` overrides the
/// indentation width used by `with_fixed_indentation` (default 2, mirroring
/// `Layout/IndentationWidth`).
#[derive(CopOptions)]
pub struct ArgumentAlignmentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "with_first_argument",
        description = "How to align arguments following the first line of a multi-line method call."
    )]
    pub enforced_style: ArgumentAlignmentStyle,
    // `Option<i64>` (not `i64`) so the bundled default `IndentationWidth: ~`
    // (which merges to JSON `null`) decodes to `None` instead of erroring the
    // whole option struct and silently discarding the user's `EnforcedStyle`.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width for `with_fixed_indentation` (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentAlignmentStyle {
    /// Align with the first argument's column.
    #[option(value = "with_first_argument")]
    WithFirstArgument,
    /// Indent one level past the call's method-name line.
    #[option(value = "with_fixed_indentation")]
    WithFixedIndentation,
}

#[cop(
    name = "Layout/ArgumentAlignment",
    description = "Align the arguments of a multi-line method call.",
    default_severity = "warning",
    default_enabled = true,
    options = ArgumentAlignmentOptions,
)]
impl ArgumentAlignment {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Visible column (0-based, char count) of a byte offset within its line.
fn display_column(offset: u32, src: &str) -> usize {
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// Returns true when `offset` is the first non-whitespace byte on its line.
fn begins_its_line(offset: u32, src: &str) -> bool {
    let bytes = src.as_bytes();
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    bytes[line_start..offset as usize]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    // `return if !multiple_arguments?(node) || (node.call_type? && node.method?(:[]=))`.
    if !multiple_arguments(args, cx) {
        return;
    }
    if cx.method_name(node) == Some("[]=") {
        return;
    }

    let opts = cx.options_or_default::<ArgumentAlignmentOptions>();
    let fixed = opts.enforced_style == ArgumentAlignmentStyle::WithFixedIndentation;
    let src = cx.source();

    // RuboCop's `flattened_arguments`.
    let items = flattened_arguments(args, fixed, cx);

    // Base column: first item's display column (with_first_argument), or the
    // method-name line's indentation + width (with_fixed_indentation / no item).
    let base_column = if fixed || items.is_empty() {
        let anchor = anchor_line_offset(node, cx);
        first_non_ws_column(anchor, src) + indentation_width(&opts)
    } else {
        display_column(cx.range(items[0]).start, src)
    };

    let msg = if fixed {
        FIXED_INDENT_MSG
    } else {
        ALIGN_PARAMS_MSG
    };

    for &item in &items {
        let start = cx.range(item).start;
        if !begins_its_line(start, src) {
            continue;
        }
        if display_column(start, src) != base_column {
            cx.emit_offense(offending_range(item, cx), msg, None);
        }
    }
}

/// RuboCop's `multiple_arguments?`: two or more arguments, or a single hash
/// first argument with two or more pairs. The hash may be braced or braceless —
/// RuboCop checks `first_argument.hash_type?` here; the braceless distinction is
/// applied later in `flattened_arguments`.
fn multiple_arguments(args: &[NodeId], cx: &Cx<'_>) -> bool {
    if args.len() >= 2 {
        return true;
    }
    let Some(&first) = args.first() else {
        return false;
    };
    matches!(cx.kind(first), NodeKind::Hash(_)) && cx.hash_pairs(first).len() >= 2
}

/// RuboCop's `flattened_arguments`. Returns the items checked for alignment.
fn flattened_arguments(args: &[NodeId], fixed: bool, cx: &Cx<'_>) -> Vec<NodeId> {
    if fixed {
        // `arguments_with_last_arg_pairs`: args[0..-2] + last_arg (pairs if
        // braceless hash).
        let Some((&last, head)) = args.split_last() else {
            return Vec::new();
        };
        let mut items = head.to_vec();
        if is_braceless_hash(last, cx) {
            items.extend(cx.hash_pairs(last));
        } else {
            items.push(last);
        }
        items
    } else {
        // `arguments_or_first_arg_pairs`: first_arg.pairs if braceless hash,
        // else all arguments.
        let Some(&first) = args.first() else {
            return Vec::new();
        };
        if is_braceless_hash(first, cx) {
            cx.hash_pairs(first)
        } else {
            args.to_vec()
        }
    }
}

/// True when `node` is a hash literal written without surrounding braces
/// (RuboCop's `hash_type? && !braces?`).
fn is_braceless_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Hash(_)) {
        return false;
    }
    !cx.raw_source(cx.range(node)).starts_with('{')
}

/// Configured indentation width for `with_fixed_indentation` (null/non-positive
/// → default 2).
fn indentation_width(opts: &ArgumentAlignmentOptions) -> usize {
    opts.indentation_width.filter(|&w| w > 0).map_or(2, |w| w as usize)
}

/// RuboCop's `target_method_lineno`: the selector line, or the opening `(` line
/// for `l.(…)`-style calls with no selector. Returns a byte offset on that line.
fn anchor_line_offset(node: NodeId, cx: &Cx<'_>) -> u32 {
    let selector = cx.selector(node);
    if selector != Range::ZERO {
        return selector.start;
    }
    let begin = cx.loc(node).begin();
    if begin != Range::ZERO {
        return begin.start;
    }
    cx.range(node).start
}

/// Column of the first non-whitespace char on the line containing `offset`.
fn first_non_ws_column(offset: u32, src: &str) -> usize {
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    let line_end = src[line_start..]
        .find('\n')
        .map_or(src.len(), |p| line_start + p);
    src[line_start..line_end]
        .chars()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0)
}

/// Highlight the offending item, trimmed to its first line.
fn offending_range(item: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(item);
    let src = cx.source().as_bytes();
    let line_end = src[r.start as usize..r.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(r.end, |pos| r.start + pos as u32);
    Range {
        start: r.start,
        end: line_end,
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgumentAlignment, ArgumentAlignmentOptions, ArgumentAlignmentStyle};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn fixed() -> ArgumentAlignmentOptions {
        ArgumentAlignmentOptions {
            enforced_style: ArgumentAlignmentStyle::WithFixedIndentation,
            indentation_width: None,
        }
    }

    /// Regression (Codex #384): bundled default `IndentationWidth: ~` → JSON
    /// `null`. It must decode (as `Option<i64>`) rather than erroring the struct
    /// and discarding the user's `EnforcedStyle`.
    #[test]
    fn null_indentation_width_preserves_other_keys() {
        let opts = <ArgumentAlignmentOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"with_fixed_indentation","IndentationWidth":null}"#,
        )
        .expect("null IndentationWidth must decode, not discard the struct");
        assert!(opts.enforced_style == ArgumentAlignmentStyle::WithFixedIndentation);
    }

    /// Parity pin (Codex #384): RuboCop's `multiple_arguments?` is satisfied by a
    /// single hash first argument with >=2 pairs regardless of braces, so a lone
    /// braced hash under `with_fixed_indentation` is still checked against the
    /// call's fixed indent. Here the `{` sits at column 4 but the fixed base is
    /// column 2 (`foo(` indent 0 + one level), so it is flagged.
    #[test]
    fn fixed_checks_single_braced_hash_argument() {
        let offenses = run_cop_with_options::<ArgumentAlignment>(
            "foo(\n    { a: 1,\n      b: 2 }\n)\n",
            &fixed(),
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert!(
            offenses[0].message.contains("one level of indentation"),
            "got {offenses:?}"
        );
    }

    // with_first_argument (default) ---------------------------------------

    #[test]
    fn accepts_aligned_with_first_argument() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo :bar,
                :baz,
                key: value
        "});
    }

    #[test]
    fn accepts_open_paren_on_own_line() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo(
              :bar,
              :baz,
              key: value
            )
        "});
    }

    #[test]
    fn flags_misaligned_argument() {
        test::<ArgumentAlignment>().expect_offense(indoc! {"
            foo :bar,
              :baz
              ^^^^ Align the arguments of a method call if they span more than one line.
        "});
    }

    #[test]
    fn flags_misaligned_inside_parens() {
        test::<ArgumentAlignment>().expect_offense(indoc! {"
            foo(
              :bar,
                :baz
                ^^^^ Align the arguments of a method call if they span more than one line.
            )
        "});
    }

    #[test]
    fn accepts_single_argument() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo(
              :bar
            )
        "});
    }

    #[test]
    fn accepts_single_line_call() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo(:bar, :baz, key: value)
        "});
    }

    #[test]
    fn ignores_element_assignment() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            obj[:foo,
              :bar] = value
        "});
    }

    // braceless hash flattening -------------------------------------------

    #[test]
    fn flags_misaligned_braceless_hash_pairs() {
        test::<ArgumentAlignment>().expect_offense(indoc! {"
            foo a: 1,
              b: 2
              ^^^^ Align the arguments of a method call if they span more than one line.
        "});
    }

    #[test]
    fn accepts_aligned_braceless_hash_pairs() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo a: 1,
                b: 2
        "});
    }

    // with_first_argument: a *trailing* braceless hash is NOT flattened (only a
    // braceless-hash *first* argument is). The items are `[x, hash]`; the hash
    // node begins at `a: 1`. So `b: 2`, being inside the hash node rather than a
    // top-level item, is not checked even though it is "misaligned".
    #[test]
    fn first_argument_does_not_flatten_trailing_hash() {
        test::<ArgumentAlignment>().expect_no_offenses(indoc! {"
            foo x,
                a: 1,
              b: 2
        "});
    }

    // The flagged item here is the hash node itself (begins at `a: 1`), which
    // sits one column left of the base set by `x`.
    #[test]
    fn first_argument_flags_misaligned_trailing_hash_node() {
        test::<ArgumentAlignment>().expect_offense(indoc! {"
            foo x,
              a: 1,
              ^^^^^ Align the arguments of a method call if they span more than one line.
              b: 2
        "});
    }

    // with_fixed_indentation ----------------------------------------------

    #[test]
    fn fixed_accepts_one_level_indentation() {
        test::<ArgumentAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                foo :bar,
                  :baz,
                  key: value
            "});
    }

    #[test]
    fn fixed_flags_aligned_with_first_argument() {
        test::<ArgumentAlignment>()
            .with_options(&fixed())
            .expect_offense(indoc! {"
                foo :bar,
                    :baz
                    ^^^^ Use one level of indentation for arguments following the first line of a multi-line method call.
            "});
    }

    // with_fixed_indentation flattens the *trailing* braceless hash to its
    // pairs (`arguments_with_last_arg_pairs`), unlike with_first_argument. Each
    // pair must sit at the fixed indentation (method-name column + 2).
    #[test]
    fn fixed_accepts_flattened_trailing_hash_pairs() {
        test::<ArgumentAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                foo :bar,
                  a: 1,
                  b: 2
            "});
    }

    #[test]
    fn fixed_flags_misaligned_trailing_hash_pair() {
        test::<ArgumentAlignment>()
            .with_options(&fixed())
            .expect_offense(indoc! {"
                foo :bar,
                  a: 1,
                    b: 2
                    ^^^^ Use one level of indentation for arguments following the first line of a multi-line method call.
            "});
    }
}

murphy_plugin_api::submit_cop!(ArgumentAlignment);
