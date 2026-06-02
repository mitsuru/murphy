//! `Style/MixinGrouping` — checks for grouping of mixins in `class` and
//! `module` bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MixinGrouping
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: separated (default) — flags `include`/`extend`/`prepend`
//!   with multiple arguments (e.g. `include A, B`); autocorrects by splitting
//!   into separate statements, arguments reversed (matching RuboCop's
//!   `separate_mixins` which reverses arguments and emits them last-first).
//!
//!   EnforcedStyle: grouped — flags multiple consecutive statements with the
//!   same method name in the same body; autocorrects by merging into one
//!   statement on the first occurrence (also reversed), and deleting subsequent
//!   occurrences (swallowing the preceding whitespace/newline to avoid blank lines).
//!
//!   Only direct-body `Send` nodes with nil receiver and non-empty args are
//!   inspected. Methods on the body are `include`, `extend`, `prepend`.
//!
//!   Body extraction: if the body is a `Begin` node, iterate its children;
//!   otherwise treat the single body node as a one-element list.
//!
//!   No `sclass` support (matches upstream which also only covers `class`/`module`).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MIXIN_METHODS: &[&str] = &["include", "extend", "prepend"];

/// Enforced grouping style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Each mixin goes in its own statement: `include A` then `include B`.
    #[default]
    #[option(value = "separated")]
    Separated,
    /// All mixins of the same method go in one statement: `include A, B`.
    #[option(value = "grouped")]
    Grouped,
}

/// Cop options for [`MixinGrouping`].
#[derive(CopOptions)]
pub struct MixinGroupingOptions {
    #[option(
        name = "EnforcedStyle",
        default = "separated",
        description = "When `separated` each mixin is its own statement. When `grouped` same-method mixins are merged."
    )]
    pub enforced_style: EnforcedStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct MixinGrouping;

#[cop(
    name = "Style/MixinGrouping",
    description = "Checks for grouping of mixins in `class` and `module` bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = MixinGroupingOptions,
)]
impl MixinGrouping {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check_body(node, cx);
    }
}

fn check_body(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MixinGroupingOptions>();

    // Extract the body OptNodeId depending on node kind.
    let body_opt = match cx.kind(node) {
        NodeKind::Class { body, .. } => *body,
        NodeKind::Module { body, .. } => *body,
        _ => return,
    };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    // Collect direct-child Send nodes that are mixin calls.
    let mixin_sends = collect_mixin_sends(body_id, cx);
    if mixin_sends.is_empty() {
        return;
    }

    match opts.enforced_style {
        EnforcedStyle::Separated => {
            for &send_id in &mixin_sends {
                let args = mixin_send_args(send_id, cx);
                if args.len() > 1 {
                    // Offense: multi-arg mixin call, should be split.
                    let method_name = mixin_send_method(send_id, cx);
                    let msg = format!("Put `{}` mixins in separate statements.", method_name);
                    cx.emit_offense(cx.range(send_id), &msg, None);

                    // Autocorrect: split into separate statements (args reversed).
                    let replacement = separate_mixins(send_id, &args, cx);
                    cx.emit_edit(cx.range(send_id), &replacement);
                }
            }
        }
        EnforcedStyle::Grouped => {
            // For each unique mixin method, find all sibling sends with that method.
            let mut seen_methods: Vec<String> = Vec::new();
            for &send_id in &mixin_sends {
                let method_name = mixin_send_method(send_id, cx).to_owned();
                if seen_methods.contains(&method_name) {
                    continue;
                }
                seen_methods.push(method_name.clone());

                let siblings: Vec<NodeId> = mixin_sends
                    .iter()
                    .copied()
                    .filter(|&s| mixin_send_method(s, cx) == method_name)
                    .collect();

                if siblings.len() <= 1 {
                    continue;
                }

                // Flag each sibling and emit correction.
                for &sibling in &siblings {
                    let msg =
                        format!("Put `{}` mixins in a single statement.", method_name);
                    cx.emit_offense(cx.range(sibling), &msg, None);
                }

                // Autocorrect:
                // - First sibling: replace with grouped form (all args reversed).
                // - Subsequent siblings: delete (swallow preceding whitespace/newline).
                let first = siblings[0];
                let grouped = group_mixins(&siblings, cx);
                cx.emit_edit(cx.range(first), &grouped);

                let source = cx.source().as_bytes();
                for &sibling in siblings.iter().skip(1) {
                    let remove_range = range_to_remove_for_subsequent_mixin(source, &siblings, sibling, cx);
                    cx.emit_edit(remove_range, "");
                }
            }
        }
    }
}

/// Returns all direct-child Send nodes in `body_id` that are mixin calls
/// (nil receiver, method in MIXIN_METHODS, non-empty args).
fn collect_mixin_sends(body_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let stmts: &[NodeId] = match cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(*list),
        _ => {
            // Single-statement body: treat as one-element slice.
            return if is_mixin_send(body_id, cx) { vec![body_id] } else { vec![] };
        }
    };
    stmts.iter().copied().filter(|&id| is_mixin_send(id, cx)).collect()
}

/// Returns `true` if `node` is a nil-receiver Send with a mixin method and
/// at least one argument.
fn is_mixin_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return false;
    };
    if receiver.get().is_some() {
        return false;
    }
    let method_name = cx.symbol_str(method);
    if !MIXIN_METHODS.contains(&method_name) {
        return false;
    }
    !cx.list(args).is_empty()
}

/// Returns the method name of a Send node (panics if not a Send — callers
/// ensure the node is a mixin send).
fn mixin_send_method<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    let NodeKind::Send { method, .. } = *cx.kind(node) else {
        unreachable!("mixin_send_method called on non-Send node");
    };
    cx.symbol_str(method)
}

/// Returns the args slice for a mixin Send node.
fn mixin_send_args<'a>(node: NodeId, cx: &'a Cx<'a>) -> &'a [NodeId] {
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return &[];
    };
    cx.list(args)
}

/// Builds the `separated` autocorrect for a multi-arg mixin send.
///
/// Mirrors RuboCop's `separate_mixins`: reverses arguments and emits one
/// statement per argument, lines 2+ prefixed with the node's column indent.
fn separate_mixins(send_id: NodeId, args: &[NodeId], cx: &Cx<'_>) -> String {
    let method_name = mixin_send_method(send_id, cx);
    let source = cx.source().as_bytes();

    // Compute the column indent (bytes from last newline to node start).
    let node_start = cx.range(send_id).start as usize;
    let indent_start = source[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let indent_bytes = &source[indent_start..node_start];
    let indent = std::str::from_utf8(indent_bytes).unwrap_or("");

    // Reversed args: last arg becomes the first statement (matching RuboCop).
    let mut lines = Vec::with_capacity(args.len());
    for &arg in args.iter().rev() {
        let arg_src = cx.raw_source(cx.range(arg));
        lines.push(format!("{method_name} {arg_src}"));
    }

    lines.join(&format!("\n{indent}"))
}

/// Builds the `grouped` autocorrect for the first sibling of a set.
///
/// Mirrors RuboCop's `group_mixins`: collects args from all siblings in
/// **reverse** order (last sibling's args first), then joins with `, `.
fn group_mixins(siblings: &[NodeId], cx: &Cx<'_>) -> String {
    let method_name = mixin_send_method(siblings[0], cx);
    // Collect all args from all siblings reversed.
    let all_args: Vec<&str> = siblings
        .iter()
        .rev()
        .flat_map(|&s| mixin_send_args(s, cx))
        .map(|&arg_id| cx.raw_source(cx.range(arg_id)))
        .collect();
    format!("{} {}", method_name, all_args.join(", "))
}

/// Computes the range to remove for a subsequent (non-first) sibling in the
/// `grouped` autocorrect.
///
/// Mirrors RuboCop's `range_to_remove_for_subsequent_mixin`: tries to extend
/// the range to include preceding whitespace/newline (so no blank line remains).
fn range_to_remove_for_subsequent_mixin(
    source: &[u8],
    siblings: &[NodeId],
    node: NodeId,
    cx: &Cx<'_>,
) -> Range {
    let node_range = cx.range(node);
    // Find the previous sibling.
    let prev = siblings
        .windows(2)
        .find_map(|w| if w[1] == node { Some(w[0]) } else { None });

    let Some(prev_id) = prev else {
        return node_range;
    };
    let prev_end = cx.range(prev_id).end as usize;
    let node_start = node_range.start as usize;

    // The "between" region is source[prev_end..node_start].
    let between = &source[prev_end..node_start];

    // If there's only whitespace between them, extend the removal to include it.
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
    use super::{EnforcedStyle, MixinGrouping, MixinGroupingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn grouped_opts() -> MixinGroupingOptions {
        MixinGroupingOptions { enforced_style: EnforcedStyle::Grouped }
    }

    // ---- separated (default) ----

    #[test]
    fn flags_multi_arg_include_separated() {
        test::<MixinGrouping>().expect_offense(indoc! {"
            class Foo
              include Bar, Qox
              ^^^^^^^^^^^^^^^^ Put `include` mixins in separate statements.
            end
        "});
    }

    #[test]
    fn flags_multi_arg_extend_separated() {
        test::<MixinGrouping>().expect_offense(indoc! {"
            class Foo
              extend Bar, Qox
              ^^^^^^^^^^^^^^^ Put `extend` mixins in separate statements.
            end
        "});
    }

    #[test]
    fn accepts_single_arg_includes_separated() {
        test::<MixinGrouping>().expect_no_offenses(indoc! {"
            class Foo
              include Qox
              include Bar
            end
        "});
    }

    #[test]
    fn accepts_empty_class() {
        test::<MixinGrouping>().expect_no_offenses("class Foo\nend\n");
    }

    #[test]
    fn corrects_multi_arg_include_to_separate() {
        test::<MixinGrouping>().expect_correction(
            indoc! {"
                class Foo
                  include Bar, Qox
                  ^^^^^^^^^^^^^^^^ Put `include` mixins in separate statements.
                end
            "},
            indoc! {"
                class Foo
                  include Qox
                  include Bar
                end
            "},
        );
    }

    #[test]
    fn corrects_multi_arg_extend_to_separate() {
        test::<MixinGrouping>().expect_correction(
            indoc! {"
                class Foo
                  extend Bar, Qox
                  ^^^^^^^^^^^^^^^ Put `extend` mixins in separate statements.
                end
            "},
            indoc! {"
                class Foo
                  extend Qox
                  extend Bar
                end
            "},
        );
    }

    #[test]
    fn flags_multi_arg_in_module_separated() {
        test::<MixinGrouping>().expect_offense(indoc! {"
            module Foo
              include Bar, Qox
              ^^^^^^^^^^^^^^^^ Put `include` mixins in separate statements.
            end
        "});
    }

    // ---- grouped style ----

    #[test]
    fn flags_separate_extends_grouped() {
        test::<MixinGrouping>()
            .with_options(&grouped_opts())
            .expect_offense(indoc! {"
                class Foo
                  extend Bar
                  ^^^^^^^^^^ Put `extend` mixins in a single statement.
                  extend Qox
                  ^^^^^^^^^^ Put `extend` mixins in a single statement.
                end
            "});
    }

    #[test]
    fn accepts_grouped_extend_in_grouped_style() {
        test::<MixinGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  extend Qox, Bar
                end
            "});
    }

    #[test]
    fn corrects_separate_extends_to_grouped() {
        test::<MixinGrouping>()
            .with_options(&grouped_opts())
            .expect_correction(
                indoc! {"
                    class Foo
                      extend Bar
                      ^^^^^^^^^^ Put `extend` mixins in a single statement.
                      extend Qox
                      ^^^^^^^^^^ Put `extend` mixins in a single statement.
                    end
                "},
                indoc! {"
                    class Foo
                      extend Qox, Bar
                    end
                "},
            );
    }

    #[test]
    fn accepts_single_extend_in_grouped_style() {
        test::<MixinGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  extend Bar
                end
            "});
    }

    #[test]
    fn accepts_different_methods_in_grouped_style() {
        // extend and include are different methods — no grouping offense.
        test::<MixinGrouping>()
            .with_options(&grouped_opts())
            .expect_no_offenses(indoc! {"
                class Foo
                  extend Bar
                  include Qox
                end
            "});
    }

    #[test]
    fn config_round_trip() {
        use murphy_plugin_api::CopOptions;
        let opts =
            MixinGroupingOptions::from_config_json(br#"{"EnforcedStyle": "grouped"}"#).expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::Grouped);
        let opts2 =
            MixinGroupingOptions::from_config_json(br#"{"EnforcedStyle": "separated"}"#)
                .expect("valid");
        assert_eq!(opts2.enforced_style, EnforcedStyle::Separated);
    }
}

murphy_plugin_api::submit_cop!(MixinGrouping);
