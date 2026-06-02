//! `Style/AccessorGrouping` — checks for grouping of accessors in `class`,
//! `module`, and `sclass` bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AccessorGrouping
//! upstream_version_checked: 1.86.2
//! version_added: "0.87"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: grouped (default) — flags consecutive `attr_reader`,
//!   `attr_writer`, `attr_accessor`, or `attr` calls with the same method name
//!   and same visibility that could be merged into one statement.
//!   Autocorrects by merging all groupable siblings into the first occurrence
//!   and deleting the rest (including preceding whitespace/newline).
//!
//!   EnforcedStyle: separated — flags any accessor call with more than one
//!   argument; autocorrects by splitting into one-argument-per-call.
//!
//!   Known v1 limitations vs RuboCop:
//!   - `skip_for_grouping?` (constant-after-accessor guard) is not
//!     implemented; grouping may be suggested across a constant assignment.
//!   - `range_with_trailing_argument_comment` (comment preservation on
//!     separated correction) is not implemented.
//!   - RBS::Inline annotation (`#:` comment) guard for `groupable_accessor?`
//!     is not implemented.
//!   - Sorbet `sig { ... }` block unwrapping in `groupable_accessor?` is not
//!     implemented; a Sorbet block before an accessor will allow grouping that
//!     RuboCop would forbid.
//! ```
//!
//! ## Matched shapes
//!
//! `Class`, `Module`, and `Sclass` nodes whose direct-child `Send` nodes are
//! accessor macros (`attr_reader`, `attr_writer`, `attr_accessor`, `attr`)
//! with a nil receiver and at least one argument.
//!
//! ## Autocorrect
//!
//! Grouped style: replace the first groupable sibling with a merged
//! `attr_reader :a, :b, :c` form; delete subsequent siblings along with any
//! preceding whitespace. Separated style: expand each multi-arg accessor into
//! N single-arg statements.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const ACCESSOR_METHODS: &[&str] = &["attr_reader", "attr_writer", "attr_accessor", "attr"];
const ACCESS_MODIFIERS: &[&str] = &["public", "protected", "private", "module_function"];

const GROUPED_MSG: &str = "Group together all `%s` attributes.";
const SEPARATED_MSG: &str = "Use one attribute per `%s`.";

/// Enforced grouping style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// All same-method accessors in the same body are merged into one statement.
    #[default]
    #[option(value = "grouped")]
    Grouped,
    /// Each accessor goes in its own statement with exactly one argument.
    #[option(value = "separated")]
    Separated,
}

/// Cop options for [`AccessorGrouping`].
#[derive(CopOptions)]
pub struct AccessorGroupingOptions {
    #[option(
        name = "EnforcedStyle",
        default = "grouped",
        description = "When `grouped` same-method accessors are merged. When `separated` each accessor has one argument."
    )]
    pub enforced_style: EnforcedStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct AccessorGrouping;

#[cop(
    name = "Style/AccessorGrouping",
    description = "Checks for grouping of accessors in `class` and `module` bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = AccessorGroupingOptions,
)]
impl AccessorGrouping {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body.get(), cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body.get(), cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sclass { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body.get(), cx);
    }
}

fn check_body(body_opt: Option<NodeId>, cx: &Cx<'_>) {
    let Some(body_id) = body_opt else {
        return;
    };
    let opts = cx.options_or_default::<AccessorGroupingOptions>();
    let accessor_sends = collect_accessor_sends(body_id, cx);
    if accessor_sends.is_empty() {
        return;
    }

    match opts.enforced_style {
        EnforcedStyle::Separated => check_separated(&accessor_sends, cx),
        EnforcedStyle::Grouped => check_grouped(&accessor_sends, cx),
    }
}

// ── Separated style ───────────────────────────────────────────────────────────

fn check_separated(sends: &[NodeId], cx: &Cx<'_>) {
    for &send_id in sends {
        let args = cx.call_arguments(send_id);
        if args.len() > 1 {
            let method_name = cx.method_name(send_id).unwrap_or("");
            let msg = SEPARATED_MSG.replacen("%s", method_name, 1);
            cx.emit_offense(cx.range(send_id), &msg, None);
            let replacement = separate_accessors(send_id, args, cx);
            cx.emit_edit(cx.range(send_id), &replacement);
        }
    }
}

/// Splits a multi-arg accessor into N single-arg statements.
/// `attr_reader :a, :b, :c` -> `attr_reader :a\nattr_reader :b\nattr_reader :c`
/// Lines 2+ are indented to the node's column.
fn separate_accessors(send_id: NodeId, args: &[NodeId], cx: &Cx<'_>) -> String {
    let method_name = cx.method_name(send_id).unwrap_or("");
    let source = cx.source().as_bytes();
    let node_start = cx.range(send_id).start as usize;
    let indent_start = source[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let indent_bytes = &source[indent_start..node_start];
    let indent = std::str::from_utf8(indent_bytes).unwrap_or("");

    let mut lines = Vec::with_capacity(args.len());
    for &arg in args {
        let arg_src = cx.raw_source(cx.range(arg));
        lines.push(format!("{method_name} {arg_src}"));
    }
    lines.join(&format!("\n{indent}"))
}

// ── Grouped style ─────────────────────────────────────────────────────────────

fn check_grouped(sends: &[NodeId], cx: &Cx<'_>) {
    let mut seen_groups: Vec<(String, &'static str)> = Vec::new(); // (method_name, visibility)

    for &send_id in sends {
        let method_name = cx.method_name(send_id).unwrap_or("").to_string();
        let visibility = accessor_visibility(send_id, cx);

        if seen_groups.iter().any(|(m, v)| m == &method_name && *v == visibility) {
            continue;
        }
        seen_groups.push((method_name.clone(), visibility));

        // Collect all groupable siblings (including this one).
        let siblings: Vec<NodeId> = sends
            .iter()
            .copied()
            .filter(|&s| {
                cx.method_name(s).unwrap_or("") == method_name.as_str()
                    && accessor_visibility(s, cx) == visibility
                    && groupable_accessor(s, cx)
                    && !has_previous_line_comment(s, cx)
            })
            .collect();

        if siblings.len() <= 1 {
            continue;
        }

        // Flag each sibling.
        for &sibling in &siblings {
            let msg = GROUPED_MSG.replacen("%s", &method_name, 1);
            cx.emit_offense(cx.range(sibling), &msg, None);
        }

        // Autocorrect: first gets merged form; rest are deleted.
        let first = siblings[0];
        let grouped = group_accessors(&siblings, cx);
        cx.emit_edit(cx.range(first), &grouped);

        let source = cx.source().as_bytes();
        for &sibling in siblings.iter().skip(1) {
            let remove_range = range_to_remove(source, &siblings, sibling, cx);
            cx.emit_edit(remove_range, "");
        }
    }
}

/// Returns the visibility label for `send_id` by scanning left siblings in
/// the body for the nearest bare `private`/`protected`/`public`/`module_function` send.
fn accessor_visibility(send_id: NodeId, cx: &Cx<'_>) -> &'static str {
    let Some(parent) = cx.parent(send_id).get() else {
        return "public";
    };
    let all_siblings: Vec<NodeId> = match cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(*list).to_vec(),
        _ => return "public",
    };

    let target_pos = match all_siblings.iter().position(|&s| s == send_id) {
        Some(p) => p,
        None => return "public",
    };

    // Walk backwards to find last bare visibility modifier.
    for &sibling in all_siblings[..target_pos].iter().rev() {
        let NodeKind::Send { method, args, receiver } = cx.kind(sibling) else {
            continue;
        };
        let name = cx.symbol_str(*method);
        if receiver.get().is_none()
            && cx.list(*args).is_empty()
            && ACCESS_MODIFIERS.contains(&name)
        {
            return match name {
                "private" => "private",
                "protected" => "protected",
                _ => "public",
            };
        }
    }
    "public"
}

/// Returns `true` if `node` is a groupable accessor — i.e., if the previous
/// sibling is either another accessor, an access modifier, or is absent (or
/// there's a blank line between them). Mirrors RuboCop's `groupable_accessor?`.
///
/// Not implemented: Sorbet `sig { ... }` block unwrapping and RBS::Inline
/// annotation (`#:` comment) guard.
fn groupable_accessor(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return true;
    };
    let all_siblings: Vec<NodeId> = match cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(*list).to_vec(),
        _ => return true, // single-statement body
    };
    let target_pos = match all_siblings.iter().position(|&s| s == node) {
        Some(p) => p,
        None => return true,
    };
    if target_pos == 0 {
        return true; // no left sibling
    }

    let prev = all_siblings[target_pos - 1];

    // If previous is a send, check what kind it is.
    if let NodeKind::Send { method, args, receiver } = cx.kind(prev) {
        if receiver.get().is_none() {
            let name = cx.symbol_str(*method);
            // Is it a bare access modifier?
            if ACCESS_MODIFIERS.contains(&name) && cx.list(*args).is_empty() {
                return true;
            }
            // Is it an attribute accessor?
            if ACCESSOR_METHODS.contains(&name) && !cx.list(*args).is_empty() {
                return true;
            }
        }
        // Not an accessor or modifier — check if there's a blank line between prev and node.
        let prev_end = cx.range(prev).end as usize;
        let node_start = cx.range(node).start as usize;
        let between = &cx.source().as_bytes()[prev_end..node_start];
        let newlines = between.iter().filter(|&&b| b == b'\n').count();
        return newlines > 1;
    }

    // Previous is not a send (e.g. a def, constant, block) — groupable.
    true
}

/// Returns `true` if there is an own-line comment on the line immediately
/// above `node`. Mirrors RuboCop's `previous_line_comment?`.
fn has_previous_line_comment(node: NodeId, cx: &Cx<'_>) -> bool {
    let range_no_comments = cx.range(node);
    let range_with_comments = cx.range_with_comments(node);
    range_with_comments.start < range_no_comments.start
}

/// Collects all direct-child `Send` nodes in `body_id` that are accessor calls.
fn collect_accessor_sends(body_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let stmts: &[NodeId] = match cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(*list),
        _ => {
            return if is_accessor_send(body_id, cx) {
                vec![body_id]
            } else {
                vec![]
            };
        }
    };
    stmts.iter().copied().filter(|&id| is_accessor_send(id, cx)).collect()
}

/// Returns `true` if `node` is an accessor send: nil receiver, accessor method
/// name, at least one argument.
fn is_accessor_send(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.call_receiver(node).get().is_some() {
        return false;
    }
    let Some(method_name) = cx.method_name(node) else {
        return false;
    };
    if !ACCESSOR_METHODS.contains(&method_name) {
        return false;
    }
    !cx.call_arguments(node).is_empty()
}

/// Builds the grouped form: `attr_reader :a, :b, :c` from all siblings.
/// Collects arguments preserving order (first sibling first), deduplicating.
fn group_accessors(siblings: &[NodeId], cx: &Cx<'_>) -> String {
    let method_name = cx.method_name(siblings[0]).unwrap_or("");
    let mut arg_srcs: Vec<&str> = Vec::new();
    for &s in siblings {
        for &arg_id in cx.call_arguments(s) {
            let src = cx.raw_source(cx.range(arg_id));
            if !arg_srcs.contains(&src) {
                arg_srcs.push(src);
            }
        }
    }
    format!("{} {}", method_name, arg_srcs.join(", "))
}

/// Computes the range to remove for a subsequent (non-first) sibling in the
/// grouped autocorrect — extends to include the preceding whitespace/newline.
fn range_to_remove(source: &[u8], siblings: &[NodeId], node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let prev = siblings
        .windows(2)
        .find_map(|w| if w[1] == node { Some(w[0]) } else { None });

    let Some(prev_id) = prev else {
        return node_range;
    };
    let prev_end = cx.range(prev_id).end as usize;
    let node_start = node_range.start as usize;

    let between = &source[prev_end..node_start];
    if !between.iter().any(|&b| !b.is_ascii_whitespace()) {
        Range {
            start: prev_end as u32,
            end: node_range.end,
        }
    } else {
        node_range
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessorGrouping, AccessorGroupingOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn separated_opts() -> AccessorGroupingOptions {
        AccessorGroupingOptions { enforced_style: EnforcedStyle::Separated }
    }

    fn grouped_opts() -> AccessorGroupingOptions {
        AccessorGroupingOptions { enforced_style: EnforcedStyle::Grouped }
    }

    // --- Grouped (default) ---

    #[test]
    fn grouped_flags_consecutive_attr_readers() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                class Foo
                  attr_reader :bar
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                  attr_reader :baz
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                end
            "});
    }

    #[test]
    fn grouped_accepts_single_attr_reader() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                end
            "});
    }

    #[test]
    fn grouped_accepts_already_grouped() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar, :baz
                end
            "});
    }

    #[test]
    fn grouped_corrects_two_readers_into_one() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_correction(
                indoc! {"
                    class Foo
                      attr_reader :bar
                      ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                      attr_reader :baz
                      ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                    end
                "},
                indoc! {"
                    class Foo
                      attr_reader :bar, :baz
                    end
                "},
            );
    }

    #[test]
    fn grouped_corrects_three_readers_into_one() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_correction(
                indoc! {"
                    class Foo
                      attr_reader :bar
                      ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                      attr_reader :bax
                      ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                      attr_reader :baz
                      ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                    end
                "},
                indoc! {"
                    class Foo
                      attr_reader :bar, :bax, :baz
                    end
                "},
            );
    }

    #[test]
    fn grouped_flags_consecutive_attr_accessors() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                class Foo
                  attr_accessor :bar
                  ^^^^^^^^^^^^^^^^^^ Group together all `attr_accessor` attributes.
                  attr_accessor :baz
                  ^^^^^^^^^^^^^^^^^^ Group together all `attr_accessor` attributes.
                end
            "});
    }

    #[test]
    fn grouped_does_not_group_different_methods() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                  attr_writer :baz
                end
            "});
    }

    #[test]
    fn grouped_flags_blank_line_between_accessors() {
        // A blank line alone does NOT prevent grouping in RuboCop's grouped style.
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                class Foo
                  attr_reader :bar
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.

                  attr_reader :baz
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                end
            "});
    }

    #[test]
    fn grouped_allows_comment_before_accessor() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                  # comment for baz
                  attr_reader :baz
                end
            "});
    }

    #[test]
    fn grouped_flags_in_module() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                module Foo
                  attr_reader :bar
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                  attr_reader :baz
                  ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                end
            "});
    }

    #[test]
    fn grouped_flags_in_sclass() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                class Foo
                  class << self
                    attr_reader :bar
                    ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                    attr_reader :baz
                    ^^^^^^^^^^^^^^^^ Group together all `attr_reader` attributes.
                  end
                end
            "});
    }

    #[test]
    fn grouped_separates_different_visibility() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                  private
                  attr_reader :baz
                end
            "});
    }

    #[test]
    fn grouped_accepts_accessor_after_non_accessor_send() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                  may_be_intended_annotation :baz
                  attr_reader :baz
                end
            "});
    }

    #[test]
    fn grouped_accepts_empty_class() {
        test::<AccessorGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses("class Foo\nend\n");
    }

    // --- Separated style ---

    #[test]
    fn separated_flags_multi_arg_attr_reader() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_offense(indoc! {"
                class Foo
                  attr_reader :bar, :baz
                  ^^^^^^^^^^^^^^^^^^^^^^ Use one attribute per `attr_reader`.
                end
            "});
    }

    #[test]
    fn separated_accepts_single_arg_accessors() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                  attr_reader :baz
                end
            "});
    }

    #[test]
    fn separated_corrects_two_args_to_two_statements() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_correction(
                indoc! {"
                    class Foo
                      attr_reader :bar, :baz
                      ^^^^^^^^^^^^^^^^^^^^^^ Use one attribute per `attr_reader`.
                    end
                "},
                indoc! {"
                    class Foo
                      attr_reader :bar
                      attr_reader :baz
                    end
                "},
            );
    }

    #[test]
    fn separated_corrects_three_args() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_correction(
                indoc! {"
                    class Foo
                      attr_reader :bar, :bax, :baz
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use one attribute per `attr_reader`.
                    end
                "},
                indoc! {"
                    class Foo
                      attr_reader :bar
                      attr_reader :bax
                      attr_reader :baz
                    end
                "},
            );
    }

    #[test]
    fn separated_flags_in_module() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_offense(indoc! {"
                module Foo
                  attr_reader :bar, :baz
                  ^^^^^^^^^^^^^^^^^^^^^^ Use one attribute per `attr_reader`.
                end
            "});
    }

    #[test]
    fn separated_accepts_empty_class() {
        test::<AccessorGrouping>()
            .with_options(&separated_opts())
            .expect_no_offenses("class Foo\nend\n");
    }

    // --- Config round-trip ---

    #[test]
    fn config_round_trip() {
        use murphy_plugin_api::CopOptions;
        let opts = AccessorGroupingOptions::from_config_json(br#"{"EnforcedStyle": "grouped"}"#)
            .expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::Grouped);
        let opts2 =
            AccessorGroupingOptions::from_config_json(br#"{"EnforcedStyle": "separated"}"#)
                .expect("valid");
        assert_eq!(opts2.enforced_style, EnforcedStyle::Separated);
    }
}

murphy_plugin_api::submit_cop!(AccessorGrouping);
