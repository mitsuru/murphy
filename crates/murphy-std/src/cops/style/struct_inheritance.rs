//! `Style/StructInheritance` — flags `class Foo < Struct.new(...)` inheritance.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StructInheritance
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is complete. Autocorrect is implemented for the common cases.
//!   The ::Struct.new and Struct.new forms are treated identically (Murphy's
//!   translator lowers ::Struct to (const :Struct nil), same as Struct).
//!   Unparenthesized Struct.new (e.g. Struct.new :x, :y) is supported.
//! ```
//!
//! ## Matched shapes
//!
//! `Class` nodes whose `superclass` is:
//! - `(send (const :Struct nil) :new ...)` — `Struct.new(:x, ...)`
//! - `(block (send (const :Struct nil) :new ...) ...)` — `Struct.new(:x) do...end`
//!
//! ## Autocorrect
//!
//! Transforms `class Foo < Struct.new(...)` to `Foo = Struct.new(...) do`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str =
    "Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.";

#[derive(Default)]
pub struct StructInheritance;

#[cop(
    name = "Style/StructInheritance",
    description = "Don't extend an instance initialized by `Struct.new`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl StructInheritance {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn is_struct_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return false;
    };
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    if cx.symbol_str(method) != "new" {
        return false;
    }
    matches!(
        *cx.kind(recv_id),
        NodeKind::Const { name, .. } if cx.symbol_str(name) == "Struct"
    )
}

fn is_struct_new(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Block { call, .. } => is_struct_send(call, cx),
        _ => is_struct_send(node, cx),
    }
}

/// Find the `<` operator token between two ranges.
fn find_lt_in_range(
    toks: &[murphy_plugin_api::SourceToken],
    source: &[u8],
    range: Range,
) -> Option<Range> {
    let idx = toks.partition_point(|t| t.range.start < range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"<"
        })
        .map(|t| t.range)
}

/// Find the last `end` keyword token within `range`.
fn find_end_in_range(
    toks: &[murphy_plugin_api::SourceToken],
    source: &[u8],
    range: Range,
) -> Option<Range> {
    let idx = toks.partition_point(|t| t.range.start < range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < range.end)
        .filter(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"end"
        })
        .last()
        .map(|t| t.range)
}

/// Walk backwards from `pos` in `source`, returning the position after any
/// run of ASCII spaces/tabs (not newlines) — used to trim whitespace before
/// a token on the same line.
fn trim_same_line_space_before(source: &[u8], pos: u32) -> u32 {
    let mut i = pos as usize;
    while i > 0 && (source[i - 1] == b' ' || source[i - 1] == b'\t') {
        i -= 1;
    }
    i as u32
}

fn check(class_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class {
        name: class_name_id,
        superclass,
        body,
    } = *cx.kind(class_node)
    else {
        return;
    };

    let Some(super_id) = superclass.get() else {
        return;
    };

    if !is_struct_new(super_id, cx) {
        return;
    }

    let offense_range = cx.range(super_id);
    cx.emit_offense(offense_range, MSG, None);

    let class_range = cx.range(class_node);
    let class_name_range = cx.range(class_name_id);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Edit 1: Remove `class ` prefix (from node start to class-name start).
    let class_keyword_range = Range {
        start: class_range.start,
        end: class_name_range.start,
    };
    cx.emit_edit(class_keyword_range, "");

    // Edit 2: Replace `<` with `=`.
    let op_search = Range {
        start: class_name_range.end,
        end: cx.range(super_id).start,
    };
    if let Some(op_range) = find_lt_in_range(toks, source, op_search) {
        cx.emit_edit(op_range, "=");
    }

    // Edit 3: Handle the body / tail.
    match *cx.kind(super_id) {
        NodeKind::Block { .. } => {
            // Superclass is a block: `Struct.new(:x) do end`
            // We need to:
            // 1. Remove the block's own closing `end` keyword (and preceding
            //    same-line whitespace), keeping the `do`.
            // 2. Remove the outer class's closing `\nend` (everything from
            //    after the block to the class end).
            //
            // Result: `Person = Struct.new(:x) do\nend`
            let block_range = cx.range(super_id);

            // Find and remove the block's own `end`.
            if let Some(end_tok) = find_end_in_range(toks, source, block_range) {
                let remove_start = trim_same_line_space_before(source, end_tok.start);
                cx.emit_edit(
                    Range {
                        start: remove_start,
                        end: end_tok.end,
                    },
                    "",
                );
            }

            // The class's outer `end` stays — it serves as the block's
            // closing `end` in the corrected output.
        }
        _ => {
            let has_body = body.get().is_some();
            if !has_body {
                // Empty body: remove everything after Struct.new(...) through
                // end of class. Result: `Name = Struct.new(...)`
                let super_end = cx.range(super_id).end;
                cx.emit_edit(
                    Range {
                        start: super_end,
                        end: class_range.end,
                    },
                    "",
                );
            } else {
                // Non-empty body: append ` do` after the struct.new call.
                // If unparenthesized, wrap args in parens first.
                let super_range = cx.range(super_id);
                let name_range = cx.loc(super_id).name;
                let has_parens = toks
                    .iter()
                    .skip(toks.partition_point(|t| t.range.start < name_range.end))
                    .take_while(|t| t.range.start < super_range.end)
                    .any(|t| t.kind == SourceTokenKind::LeftParen);

                if has_parens {
                    // Parenthesized: append ` do` after closing paren.
                    cx.emit_edit(
                        Range {
                            start: super_range.end,
                            end: super_range.end,
                        },
                        " do",
                    );
                } else {
                    // Unparenthesized: replace `<selector-end>...<expr-end>`
                    // with `(<args>) do`.
                    let NodeKind::Send { args, .. } = *cx.kind(super_id) else {
                        return;
                    };
                    let args_list = cx.list(args);
                    if args_list.is_empty() {
                        cx.emit_edit(
                            Range {
                                start: super_range.end,
                                end: super_range.end,
                            },
                            " do",
                        );
                    } else {
                        let args_src: Vec<&str> = args_list
                            .iter()
                            .map(|&a| cx.raw_source(cx.range(a)))
                            .collect();
                        let args_joined = args_src.join(", ");
                        cx.emit_edit(
                            Range {
                                start: name_range.end,
                                end: super_range.end,
                            },
                            &format!("({args_joined}) do"),
                        );
                    }
                }
                // The class's closing `end` is preserved as-is.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StructInheritance;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_struct_inheritance_with_body() {
        test::<StructInheritance>().expect_correction(
            indoc! {"
                class Person < Struct.new(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.
                  def foo; end
                end
            "},
            indoc! {"
                Person = Struct.new(:first_name, :last_name) do
                  def foo; end
                end
            "},
        );
    }

    #[test]
    fn flags_struct_inheritance_empty_body_multiline() {
        test::<StructInheritance>().expect_correction(
            indoc! {"
                class Person < Struct.new(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.
                end
            "},
            "Person = Struct.new(:first_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_struct_inheritance_empty_body_single_line() {
        test::<StructInheritance>().expect_correction(
            "class Person < Struct.new(:first_name, :last_name); end\n               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.\n",
            "Person = Struct.new(:first_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_struct_inheritance_with_do_end_block() {
        test::<StructInheritance>().expect_correction(
            indoc! {"
                class Person < Struct.new(:first_name, :last_name) do end
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.
                end
            "},
            indoc! {"
                Person = Struct.new(:first_name, :last_name) do
                end
            "},
        );
    }

    #[test]
    fn flags_struct_with_cbase_notation() {
        // ::Struct.new is treated identically to Struct.new in Murphy
        test::<StructInheritance>().expect_correction(
            indoc! {"
                class Person < ::Struct.new(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.
                  def foo; end
                end
            "},
            indoc! {"
                Person = ::Struct.new(:first_name, :last_name) do
                  def foo; end
                end
            "},
        );
    }

    #[test]
    fn accepts_plain_class() {
        test::<StructInheritance>().expect_no_offenses(indoc! {"
            class Person
            end
        "});
    }

    #[test]
    fn accepts_assignment_to_struct_new() {
        test::<StructInheritance>()
            .expect_no_offenses("Person = Struct.new(:first_name, :last_name)\n");
    }

    #[test]
    fn accepts_assignment_to_block_form() {
        test::<StructInheritance>().expect_no_offenses(indoc! {"
            Person = Struct.new(:first_name, :last_name) do
              def age; 42; end
            end
        "});
    }

    #[test]
    fn accepts_normal_class_inheritance() {
        test::<StructInheritance>().expect_no_offenses(indoc! {"
            class Person < Animal
            end
        "});
    }

    #[test]
    fn accepts_delegate_class() {
        test::<StructInheritance>().expect_no_offenses(indoc! {"
            class Person < DelegateClass(Animal)
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(StructInheritance);
