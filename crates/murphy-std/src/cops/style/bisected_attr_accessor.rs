//! `Style/BisectedAttrAccessor` — combine paired `attr_reader`/`attr_writer`
//! for the same method into a single `attr_accessor`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BisectedAttrAccessor
//! upstream_version_checked: 1.86.2
//! version_added: "0.87"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Checks class, module, and sclass bodies for attr_reader/attr_writer
//!   pairs (including the bare `attr` method as a reader) that can be
//!   combined into a single attr_accessor. Visibility grouping is honoured
//!   (a public reader + private writer are not flagged). Autocorrect emits
//!   coordinated edits across reader and writer macro nodes.
//!   Known v1 limitation: private/protected/public methods that wrap an
//!   attr call in the same argument (e.g. `private def foo`) are not handled
//!   -- those are represented differently in the AST (conservative: no false
//!   positives, just a missed pairing).
//! ```
//!
//! ## Matched shapes
//!
//! Direct children of a class/module/sclass body that are bare `Send` nodes
//! (nil receiver) whose method is one of:
//! - `attr_reader` or `attr` (counts as a reader)
//! - `attr_writer` (counts as a writer)
//!
//! When an attribute symbol appears in both a reader and a writer within the
//! same visibility group, both argument nodes are flagged.
//!
//! ## Why this shape
//!
//! Only direct-child sends are matched, mirroring RuboCop's `each_child_node(:send)`.
//! Attr macros inside conditionals or other nested structures are intentionally
//! excluded -- they may have dynamic semantics.
//!
//! Visibility is tracked by scanning for bare `private`/`protected`/`public`
//! sends with no arguments (cursor-style visibility changes).
//!
//! ## Autocorrect
//!
//! Given:
//!
//! ```ruby
//! attr_reader :a, :b   # bisected: [:a]
//! attr_writer :a
//! ```
//!
//! The reader macro, when partially bisected, becomes:
//!
//! ```ruby
//! attr_accessor :a
//! attr_reader :b
//! ```
//!
//! When fully bisected, the reader line is replaced with `attr_accessor :bisected`.
//! The writer line is removed entirely when fully bisected, or trimmed when partial.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Classification of an attr macro node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MacroKind {
    Reader, // attr_reader or attr
    Writer, // attr_writer
}

/// One attr macro call in the class body.
struct AttrMacro {
    /// The Send node id.
    node: NodeId,
    /// Whether this is a reader or writer.
    kind: MacroKind,
    /// The argument symbol nodes (one per attribute name).
    arg_nodes: Vec<NodeId>,
    /// The attribute names (symbol value, e.g. "bar" for :bar).
    names: Vec<String>,
}

impl AttrMacro {
    fn is_reader(&self) -> bool {
        self.kind == MacroKind::Reader
    }

    fn is_writer(&self) -> bool {
        self.kind == MacroKind::Writer
    }
}

/// One visibility group collected from the body.
struct VisibilityGroup {
    macros: Vec<AttrMacro>,
}

#[derive(Default)]
pub struct BisectedAttrAccessor;

#[cop(
    name = "Style/BisectedAttrAccessor",
    description = "Combine paired `attr_reader`/`attr_writer` into `attr_accessor`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl BisectedAttrAccessor {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body, cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sclass { body, .. } = *cx.kind(node) else {
            return;
        };
        check_body(body, cx);
    }
}

/// Extract direct children of a body node.
fn body_children(body: OptNodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(body_id) = body.get() else {
        return vec![];
    };
    match *cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![body_id],
    }
}

/// Returns true if this is a bare visibility setter with no arguments.
fn is_bare_visibility(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    if receiver.get().is_some() {
        return false;
    }
    let method_name = cx.symbol_str(method);
    if !matches!(method_name, "private" | "protected" | "public") {
        return false;
    }
    cx.list(args).is_empty()
}

/// Try to parse an attr macro from a node.
fn parse_attr_macro(node: NodeId, cx: &Cx<'_>) -> Option<AttrMacro> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    if receiver.get().is_some() {
        return None;
    }
    let method_name = cx.symbol_str(method);
    let macro_kind = match method_name {
        "attr_reader" | "attr" => MacroKind::Reader,
        "attr_writer" => MacroKind::Writer,
        _ => return None,
    };
    let arg_ids = cx.list(args);
    if arg_ids.is_empty() {
        return None;
    }
    let mut arg_nodes = Vec::with_capacity(arg_ids.len());
    let mut names = Vec::with_capacity(arg_ids.len());
    for &arg_id in arg_ids {
        let NodeKind::Sym(sym) = *cx.kind(arg_id) else {
            return None;
        };
        arg_nodes.push(arg_id);
        names.push(cx.symbol_str(sym).to_owned());
    }
    Some(AttrMacro {
        node,
        kind: macro_kind,
        arg_nodes,
        names,
    })
}

/// Collect visibility-grouped macros from a body.
fn collect_groups(body: OptNodeId, cx: &Cx<'_>) -> Vec<VisibilityGroup> {
    let children = body_children(body, cx);
    let mut groups: Vec<VisibilityGroup> = vec![VisibilityGroup { macros: vec![] }];
    for child in children {
        if is_bare_visibility(child, cx) {
            groups.push(VisibilityGroup { macros: vec![] });
        } else if let (Some(m), Some(g)) = (parse_attr_macro(child, cx), groups.last_mut()) {
            g.macros.push(m);
        }
    }
    groups
}

/// Process one body.
fn check_body(body: OptNodeId, cx: &Cx<'_>) {
    for group in collect_groups(body, cx) {
        check_group(&group.macros, cx);
    }
}

/// Find the intersection of reader names and writer names in a group.
fn find_bisected(macros: &[AttrMacro]) -> Vec<String> {
    use std::collections::HashSet;
    let reader_names: HashSet<&str> = macros
        .iter()
        .filter(|m| m.is_reader())
        .flat_map(|m| m.names.iter().map(|s| s.as_str()))
        .collect();
    let writer_names: HashSet<&str> = macros
        .iter()
        .filter(|m| m.is_writer())
        .flat_map(|m| m.names.iter().map(|s| s.as_str()))
        .collect();
    let mut bisected: Vec<String> = reader_names
        .intersection(&writer_names)
        .map(|&s| s.to_owned())
        .collect();
    bisected.sort();
    bisected
}

/// Emit offenses and edits for one visibility group.
fn check_group(macros: &[AttrMacro], cx: &Cx<'_>) {
    use std::collections::HashSet;
    let bisected = find_bisected(macros);
    if bisected.is_empty() {
        return;
    }
    let bisected_set: HashSet<&str> = bisected.iter().map(|s| s.as_str()).collect();

    for macro_entry in macros {
        let bisected_args: Vec<(NodeId, &str)> = macro_entry
            .arg_nodes
            .iter()
            .zip(macro_entry.names.iter())
            .filter(|(_, name)| bisected_set.contains(name.as_str()))
            .map(|(&id, name)| (id, name.as_str()))
            .collect();

        if bisected_args.is_empty() {
            continue;
        }

        for (arg_id, name) in &bisected_args {
            let msg = format!("Combine both accessors into `attr_accessor :{name}`.");
            cx.emit_offense(cx.range(*arg_id), &msg, None);
        }

        emit_autocorrect(macro_entry, &bisected_args, cx);
    }
}

/// The names of the remaining (non-bisected) attributes in a macro, in order.
fn rest_names<'a>(
    macro_entry: &'a AttrMacro,
    bisected_set: &std::collections::HashSet<&str>,
) -> Vec<&'a str> {
    macro_entry
        .names
        .iter()
        .filter(|name| !bisected_set.contains(name.as_str()))
        .map(|s| s.as_str())
        .collect()
}

/// Emit autocorrect edits for one macro entry.
fn emit_autocorrect(
    macro_entry: &AttrMacro,
    bisected_args: &[(NodeId, &str)],
    cx: &Cx<'_>,
) {
    let bisected_set: std::collections::HashSet<&str> =
        bisected_args.iter().map(|(_, n)| *n).collect();
    let bisected_names_joined = bisected_args
        .iter()
        .map(|(_, n)| format!(":{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let all_bisected = macro_entry.names.len() == bisected_args.len();
    let node_range = cx.range(macro_entry.node);
    let whole_line = cx.range_by_whole_lines(node_range, true);
    let indent = leading_indent(cx, node_range.start);

    if macro_entry.is_reader() {
        if all_bisected {
            let replacement = format!("{indent}attr_accessor {bisected_names_joined}\n");
            cx.emit_edit(whole_line, &replacement);
        } else {
            let remaining = rest_names(macro_entry, &bisected_set);
            let remaining_joined = remaining
                .iter()
                .map(|n| format!(":{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            // Insert attr_accessor line before the current line.
            let insert_point = Range {
                start: whole_line.start,
                end: whole_line.start,
            };
            cx.emit_edit(
                insert_point,
                &format!("{indent}attr_accessor {bisected_names_joined}\n"),
            );
            // Replace the node text with the trimmed reader.
            cx.emit_edit(node_range, &format!("attr_reader {remaining_joined}"));
        }
    } else {
        // Writer
        if all_bisected {
            cx.emit_edit(whole_line, "");
        } else {
            let remaining = rest_names(macro_entry, &bisected_set);
            let remaining_joined = remaining
                .iter()
                .map(|n| format!(":{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            cx.emit_edit(node_range, &format!("attr_writer {remaining_joined}"));
        }
    }
}

/// Extract leading whitespace at the start of the line containing `offset`.
fn leading_indent(cx: &Cx<'_>, offset: u32) -> String {
    let src = cx.source().as_bytes();
    let start = offset as usize;
    let line_start = src[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[line_start..start]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .map(|&b| b as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::BisectedAttrAccessor;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense: reader + writer ---

    #[test]
    fn flags_reader_and_writer_for_same_attr() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            class Foo
              attr_reader :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
              attr_writer :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
            end
        "#});
    }

    // --- `attr` counts as reader ---

    #[test]
    fn flags_attr_as_reader() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            class Foo
              attr :bar
                   ^^^^ Combine both accessors into `attr_accessor :bar`.
              attr_writer :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
            end
        "#});
    }

    // --- No offense: reader + writer in different visibility groups ---

    #[test]
    fn no_offense_different_visibility() {
        test::<BisectedAttrAccessor>().expect_no_offenses(indoc! {r#"
            class Foo
              attr_reader :bar
              private
              attr_writer :bar
            end
        "#});
    }

    // --- Both in same private visibility group: flag ---

    #[test]
    fn flags_reader_and_writer_in_same_private_group() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            class Foo
              private
              attr_reader :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
              attr_writer :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
            end
        "#});
    }

    // --- No offense: only reader, no writer ---

    #[test]
    fn no_offense_reader_only() {
        test::<BisectedAttrAccessor>().expect_no_offenses(indoc! {r#"
            class Foo
              attr_reader :bar
            end
        "#});
    }

    // --- No offense: only writer, no reader ---

    #[test]
    fn no_offense_writer_only() {
        test::<BisectedAttrAccessor>().expect_no_offenses(indoc! {r#"
            class Foo
              attr_writer :bar
            end
        "#});
    }

    // --- No offense: already attr_accessor ---

    #[test]
    fn no_offense_attr_accessor() {
        test::<BisectedAttrAccessor>().expect_no_offenses(indoc! {r#"
            class Foo
              attr_accessor :bar
            end
        "#});
    }

    // --- Flags only the shared attribute ---

    #[test]
    fn flags_only_shared_attr() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            class Foo
              attr_reader :bar, :baz
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
              attr_writer :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
            end
        "#});
    }

    // --- Module body ---

    #[test]
    fn flags_in_module() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            module Foo
              attr_reader :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
              attr_writer :bar
                          ^^^^ Combine both accessors into `attr_accessor :bar`.
            end
        "#});
    }

    // --- Sclass body ---

    #[test]
    fn flags_in_sclass() {
        test::<BisectedAttrAccessor>().expect_offense(indoc! {r#"
            class Foo
              class << self
                attr_reader :bar
                            ^^^^ Combine both accessors into `attr_accessor :bar`.
                attr_writer :bar
                            ^^^^ Combine both accessors into `attr_accessor :bar`.
              end
            end
        "#});
    }

    // --- Autocorrect: all bisected ---

    #[test]
    fn corrects_all_bisected() {
        test::<BisectedAttrAccessor>().expect_correction(
            indoc! {r#"
                class Foo
                  attr_reader :bar
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                  attr_writer :bar
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_accessor :bar
                end
            "#},
        );
    }

    // --- Autocorrect: partial bisection (reader has extra attrs) ---

    #[test]
    fn corrects_partial_bisection_reader_has_extra() {
        test::<BisectedAttrAccessor>().expect_correction(
            indoc! {r#"
                class Foo
                  attr_reader :bar, :baz
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                  attr_writer :bar
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_accessor :bar
                  attr_reader :baz
                end
            "#},
        );
    }

    // --- Autocorrect: partial bisection (writer has extra attrs) ---

    #[test]
    fn corrects_partial_bisection_writer_has_extra() {
        test::<BisectedAttrAccessor>().expect_correction(
            indoc! {r#"
                class Foo
                  attr_reader :bar
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                  attr_writer :bar, :baz
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_accessor :bar
                  attr_writer :baz
                end
            "#},
        );
    }

    // --- Multiple bisected attrs (fully bisected) ---

    #[test]
    fn corrects_multiple_bisected_all() {
        test::<BisectedAttrAccessor>().expect_correction(
            indoc! {r#"
                class Foo
                  attr_reader :bar, :baz
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                                    ^^^^ Combine both accessors into `attr_accessor :baz`.
                  attr_writer :bar, :baz
                              ^^^^ Combine both accessors into `attr_accessor :bar`.
                                    ^^^^ Combine both accessors into `attr_accessor :baz`.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_accessor :bar, :baz
                end
            "#},
        );
    }
}

murphy_plugin_api::submit_cop!(BisectedAttrAccessor);
