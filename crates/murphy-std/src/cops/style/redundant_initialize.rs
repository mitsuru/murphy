//! `Style/RedundantInitialize` — checks for `initialize` methods that are redundant.
//!
//! An initializer is redundant if:
//! - Its body is empty **and** it takes no arguments, or
//! - Its body consists solely of a `super` or `super(...)` call that forwards
//!   the exact same arguments as the method signature.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantInitialize
//! upstream_version_checked: 1.86.2
//! version_added: "1.27"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Enabled: pending (same as upstream) — `default_enabled = false`.
//!   Safe: false — removing an empty initializer may alter behavior if the
//!   superclass initializer raises an exception.
//!   AutoCorrect: contextual — removes the entire method definition including
//!   its surrounding whitespace lines.
//!
//!   AllowComments (default: true): when true, an initializer containing any
//!   comments is not flagged. Murphy checks for comments in the node range via
//!   `cx.comments_in_range`; the upstream sub-check `!comments_contain_disables?`
//!   (skip if the comment is a `rubocop:disable` inline annotation) is not
//!   implemented — that is a v1 limitation.
//!
//!   All plain `arg` parameters must be plain `Arg` nodes; `Optarg`, `Kwoptarg`,
//!   `Kwarg`, `Blockarg` in the arg list prevent the match (no offense).
//!   Rest-like parameters (`Restarg`, `Kwrestarg`, `ForwardArgs`) cause the
//!   entire method to be skipped by the `forwards?` guard.
//!
//!   Singleton method defs (`def self.initialize`) are skipped because they have
//!   a receiver and are not instance initializers.
//! ```
//!
//! ## Matched shapes
//!
//! - `def initialize; end` — empty body, no args → MSG_EMPTY
//! - `def initialize; super; end` — `zsuper`, no args → MSG
//! - `def initialize(a, b); super; end` — `zsuper`, all plain args → MSG
//! - `def initialize(a, b); super(a, b); end` — explicit super, same args → MSG
//!
//! ## Why this shape
//!
//! These initializers add no behavior and can be safely removed (with the caveat
//! that an empty initializer might intentionally suppress a superclass one that
//! raises — hence `safe: false`).
//!
//! ## Autocorrect
//!
//! Removes the entire method definition line(s) via
//! `cx.range_by_whole_lines(cx.range(node), true)`, which mirrors RuboCop's
//! `range_by_whole_lines(node.source_range, include_final_newline: true)`.
//! Leading comments outside the node range are preserved (not deleted), matching
//! RuboCop's behavior.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, NodeList, cop};

const MSG: &str = "Remove unnecessary `initialize` method.";
const MSG_EMPTY: &str = "Remove unnecessary empty `initialize` method.";

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowComments",
        default = true,
        description = "When `true`, initializers containing comments are not flagged."
    )]
    pub allow_comments: bool,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantInitialize;

#[cop(
    name = "Style/RedundantInitialize",
    description = "Checks for redundant `initialize` methods.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl RedundantInitialize {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { receiver, name, args, body } = *cx.kind(node) else {
            return;
        };

        // Only instance initializers — skip `def self.initialize` (receiver present).
        if receiver.get().is_some() {
            return;
        }

        // Only the `initialize` method.
        if cx.symbol_str(name) != "initialize" {
            return;
        }

        let NodeKind::Args(args_list) = *cx.kind(args) else {
            return;
        };

        // Skip if any rest-like parameter is present (changes arity contract).
        if has_forwarding_arg(args_list, cx) {
            return;
        }

        // Skip if AllowComments is true and the node contains comments.
        let opts = cx.options_or_default::<Options>();
        if opts.allow_comments && !cx.comments_in_range(cx.range(node)).is_empty() {
            return;
        }

        match body.get() {
            None => {
                // Empty body — only flag when no arguments (otherwise might mask ArgumentError).
                if cx.list(args_list).is_empty() {
                    emit(node, MSG_EMPTY, cx);
                }
            }
            Some(body_id) => {
                match *cx.kind(body_id) {
                    NodeKind::Begin(_) => {
                        // Multiple statements — not redundant.
                    }
                    // Bare `super` — redundant only if all params are plain Args.
                    NodeKind::Zsuper if all_plain_args(args_list, cx) => {
                        emit(node, MSG, cx);
                    }
                    // Explicit `super(a, b)` — redundant if all params are plain
                    // Args and the super call forwards exactly the same names.
                    NodeKind::Super(super_args)
                        if all_plain_args(args_list, cx)
                            && same_args(args_list, super_args, cx) =>
                    {
                        emit(node, MSG, cx);
                    }
                    NodeKind::Zsuper | NodeKind::Super(_) => {
                        // Guard failed — not redundant.
                    }
                    _ => {
                        // Any other body — not redundant.
                    }
                }
            }
        }
    }
}

/// Returns `true` if the args list contains any rest-like node that changes
/// the method's arity contract (`Restarg`, `Kwrestarg`, `ForwardArgs`).
fn has_forwarding_arg(args_list: NodeList, cx: &Cx<'_>) -> bool {
    cx.list(args_list).iter().any(|&child| {
        matches!(
            *cx.kind(child),
            NodeKind::Restarg(_) | NodeKind::Kwrestarg(_) | NodeKind::ForwardArgs
        )
    })
}

/// Returns `true` if every parameter in the args list is a plain positional
/// `Arg` node (no `Optarg`, `Kwarg`, `Kwoptarg`, `Blockarg`, etc.).
fn all_plain_args(args_list: NodeList, cx: &Cx<'_>) -> bool {
    cx.list(args_list)
        .iter()
        .all(|&child| matches!(*cx.kind(child), NodeKind::Arg(_)))
}

/// Returns `true` if the explicit `super(…)` args exactly match the def's
/// plain arg names in order and count.
///
/// Mirrors RuboCop's `same_args?`:
/// `args.map(&:name) == super_node.arguments.map { |a| a.children[0] }`
///
/// Each super arg must be an `Lvar` whose symbol equals the corresponding
/// def arg's symbol.
fn same_args(def_args: NodeList, super_args: NodeList, cx: &Cx<'_>) -> bool {
    let def_arg_ids = cx.list(def_args);
    let super_arg_ids = cx.list(super_args);
    if def_arg_ids.len() != super_arg_ids.len() {
        return false;
    }
    def_arg_ids
        .iter()
        .zip(super_arg_ids.iter())
        .all(|(&def_arg, &super_arg)| {
            let NodeKind::Arg(def_sym) = *cx.kind(def_arg) else {
                return false;
            };
            let NodeKind::Lvar(super_sym) = *cx.kind(super_arg) else {
                return false;
            };
            def_sym == super_sym
        })
}

/// Emit an offense on the whole node and register an autocorrect that removes
/// the entire method definition lines.
///
/// Uses `cx.range_by_whole_lines(cx.range(node), true)` (not
/// `range_with_comments_and_lines`) so leading comments outside the node range
/// are not deleted — matching RuboCop's `range_by_whole_lines(node.source_range, ...)`.
fn emit(node: NodeId, msg: &str, cx: &Cx<'_>) {
    cx.emit_offense(cx.range(node), msg, None);
    let removal_range = cx.range_by_whole_lines(cx.range(node), true);
    cx.emit_edit(removal_range, "");
}

#[cfg(test)]
mod tests {
    use super::{Options, RedundantInitialize};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- empty body, no args ---

    #[test]
    fn flags_empty_no_args() {
        test::<RedundantInitialize>().expect_offense(indoc! {"
            def initialize; end
            ^^^^^^^^^^^^^^^^^^^ Remove unnecessary empty `initialize` method.
        "});
    }

    #[test]
    fn corrects_empty_no_args() {
        test::<RedundantInitialize>().expect_correction(
            indoc! {"
                def initialize; end
                ^^^^^^^^^^^^^^^^^^^ Remove unnecessary empty `initialize` method.
            "},
            "",
        );
    }

    #[test]
    fn no_offense_empty_with_args() {
        // Empty body but has arguments — might mask ArgumentError.
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(a, b)
            end
        "});
    }

    #[test]
    fn no_offense_empty_with_underscore_arg() {
        // `def initialize(_)` — changes parameter requirements.
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(_)
            end
        "});
    }

    // --- bare super (zsuper) ---

    #[test]
    fn flags_zsuper_no_args() {
        test::<RedundantInitialize>().expect_offense(indoc! {"
            def initialize; super; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `initialize` method.
        "});
    }

    #[test]
    fn flags_zsuper_with_plain_args() {
        test::<RedundantInitialize>().expect_offense(indoc! {"
            def initialize(a, b); super; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `initialize` method.
        "});
    }

    #[test]
    fn corrects_zsuper_no_args() {
        test::<RedundantInitialize>().expect_correction(
            indoc! {"
                def initialize; super; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `initialize` method.
            "},
            "",
        );
    }

    // --- explicit super(a, b) ---

    #[test]
    fn flags_explicit_super_same_args() {
        test::<RedundantInitialize>().expect_offense(indoc! {"
            def initialize(a, b); super(a, b); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `initialize` method.
        "});
    }

    #[test]
    fn corrects_explicit_super_same_args() {
        test::<RedundantInitialize>().expect_correction(
            indoc! {"
                def initialize(a, b); super(a, b); end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `initialize` method.
            "},
            "",
        );
    }

    // --- good cases ---

    #[test]
    fn no_offense_body_with_work() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize
              do_something
            end
        "});
    }

    #[test]
    fn no_offense_body_with_work_and_super() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize
              do_something
              super
            end
        "});
    }

    #[test]
    fn no_offense_super_different_arg_count() {
        // super(a) for (a, b) — different number of args.
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(a, b)
              super(a)
            end
        "});
    }

    #[test]
    fn no_offense_default_value_arg() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(a, b = 5)
              super
            end
        "});
    }

    #[test]
    fn no_offense_keyword_default_value_arg() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(a, b: 5)
              super
            end
        "});
    }

    #[test]
    fn no_offense_restarg() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(*)
            end
        "});
    }

    #[test]
    fn no_offense_kwrestarg() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(**)
            end
        "});
    }

    #[test]
    fn no_offense_forward_args() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize(...)
            end
        "});
    }

    #[test]
    fn no_offense_singleton_method() {
        // `def self.initialize` is not an instance initializer.
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def self.initialize
            end
        "});
    }

    #[test]
    fn no_offense_non_initialize() {
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def setup
            end
        "});
    }

    // --- AllowComments option ---

    #[test]
    fn no_offense_with_comment_default_allow_comments() {
        // Default: AllowComments = true — comments protect the method.
        test::<RedundantInitialize>().expect_no_offenses(indoc! {"
            def initialize
              # Overriding to negate superclass initialize.
            end
        "});
    }

    #[test]
    fn flags_with_comment_when_allow_comments_false() {
        test::<RedundantInitialize>()
            .with_options(&Options { allow_comments: false })
            .expect_offense(indoc! {"
                def initialize; end
                ^^^^^^^^^^^^^^^^^^^ Remove unnecessary empty `initialize` method.
            "});
    }

    // --- multiline corrections ---
    // Multi-line offense ranges can't be expressed with caret annotations.
    // Use run_cop_with_edits to assert offense count + corrected output.

    fn apply_edits(mut src: String, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut sorted: Vec<_> = edits.iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        for edit in sorted {
            src.replace_range(edit.range.start as usize..edit.range.end as usize, &edit.replacement);
        }
        src
    }

    #[test]
    fn corrects_multiline_empty_initialize() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let src = indoc! {"
            class Foo
              def initialize
              end
            end
        "};
        let run = run_cop_with_edits::<RedundantInitialize>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Remove unnecessary empty `initialize` method.");
        let corrected = apply_edits(src.to_owned(), &run.edits);
        assert_eq!(corrected, "class Foo
end
");
    }

    #[test]
    fn corrects_multiline_super_initialize() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let src = indoc! {"
            class Foo
              def initialize(a, b)
                super(a, b)
              end
            end
        "};
        let run = run_cop_with_edits::<RedundantInitialize>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Remove unnecessary `initialize` method.");
        let corrected = apply_edits(src.to_owned(), &run.edits);
        assert_eq!(corrected, "class Foo
end
");
    }

    #[test]
    fn leading_comment_outside_node_is_preserved() {
        // RuboCop deletes only `node.source_range` lines; leading comments outside
        // the node are NOT removed. Verify our autocorrect matches this behavior.
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let src = indoc! {"
            class Foo
              # keep me
              def initialize
                super
              end
            end
        "};
        let run = run_cop_with_edits::<RedundantInitialize>(src);
        assert_eq!(run.offenses.len(), 1);
        let corrected = apply_edits(src.to_owned(), &run.edits);
        assert!(
            corrected.contains("# keep me"),
            "Leading comment was eaten! Got: {corrected:?}"
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantInitialize);
