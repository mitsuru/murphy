//! `Style/DataInheritance` — flags `class Foo < Data.define(...)` inheritance.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DataInheritance
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detection is complete. Autocorrect is implemented for the common cases.
//!   The ::Data.define and Data.define forms are treated identically (Murphy's
//!   translator lowers ::Data to (const :Data nil), same as Data).
//!   minimum_target_ruby_version 3.2 is not enforced by Murphy (no target-ruby
//!   gating in v1); noted as acceptable parity gap.
//!   SafeAutoCorrect: false is noted — the autocorrect changes the inheritance
//!   tree (ancestors chain), which may affect downstream code.
//! ```
//!
//! ## Matched shapes
//!
//! `Class` nodes whose `superclass` is:
//! - `(send (const :Data nil) :define ...)` — `Data.define(:x, ...)`
//! - `(block (send (const :Data nil) :define ...) ...)` — `Data.define(:x) do...end`
//!
//! ## Autocorrect
//!
//! Transforms `class Foo < Data.define(...)` to `Foo = Data.define(...) do`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RangeSide, SpaceRangeOptions, SourceTokenKind, cop};

const MSG: &str = "Don't extend an instance initialized by `Data.define`. \
                   Use a block to customize the class.";

#[derive(Default)]
pub struct DataInheritance;

#[cop(
    name = "Style/DataInheritance",
    description = "Don't extend an instance initialized by `Data.define`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DataInheritance {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn is_data_send(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("define") {
        return false;
    }
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_global_const(recv_id, "Data")
}

fn is_data_define(node: NodeId, cx: &Cx<'_>) -> bool {
    if let Some(call) = cx.block_call(node).get() {
        is_data_send(call, cx)
    } else {
        is_data_send(node, cx)
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

    if !is_data_define(super_id, cx) {
        return;
    }

    let offense_range = cx.range(super_id);
    cx.emit_offense(offense_range, MSG, None);

    let class_range = cx.range(class_node);
    let class_name_range = cx.range(class_name_id);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Edit 1: Remove `class ` keyword (from node start to just after keyword+space).
    let class_kw_range = cx.loc(class_node).keyword();
    let kw_with_space = cx.range_with_surrounding_space(
        class_kw_range,
        SpaceRangeOptions {
            side: RangeSide::Right,
            newlines: false,
            whitespace: false,
            continuations: false,
        },
    );
    let class_keyword_range = Range {
        start: class_range.start,
        end: kw_with_space.end,
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
            // Superclass is a block: `Data.define(:x) do end`
            // Find and remove the block's own `end`.
            let block_range = cx.range(super_id);
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
            // The class's outer `end` stays — it serves as the block's closing `end`.
        }
        _ => {
            let has_body = body.get().is_some();
            if !has_body {
                // Empty body. Check if the region between Data.define and the
                // class's `end` contains comments — if so, preserve them.
                let super_end = cx.range(super_id).end;
                let after_define = Range {
                    start: super_end,
                    end: class_range.end,
                };
                let has_comments = !cx.comments_in_range(after_define).is_empty();

                if has_comments {
                    // Has comments: append ` do` to wrap in a block.
                    let super_range = cx.range(super_id);
                    cx.emit_edit(
                        Range {
                            start: super_range.end,
                            end: super_range.end,
                        },
                        " do",
                    );
                } else {
                    // No comments: remove everything after Data.define(...)
                    // through end of class. Result: `Name = Data.define(...)`
                    cx.emit_edit(
                        Range {
                            start: super_end,
                            end: class_range.end,
                        },
                        "",
                    );
                }
            } else {
                // Non-empty body: append ` do` after the data.define call.
                let super_range = cx.range(super_id);
                cx.emit_edit(
                    Range {
                        start: super_range.end,
                        end: super_range.end,
                    },
                    " do",
                );
                // The class's closing `end` is preserved as-is.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DataInheritance;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_data_inheritance_with_body() {
        test::<DataInheritance>().expect_correction(
            indoc! {"
                class Person < Data.define(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Data.define`. Use a block to customize the class.
                  def age; 42; end
                end
            "},
            indoc! {"
                Person = Data.define(:first_name, :last_name) do
                  def age; 42; end
                end
            "},
        );
    }

    #[test]
    fn flags_data_inheritance_empty_body_multiline() {
        test::<DataInheritance>().expect_correction(
            indoc! {"
                class Person < Data.define(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Data.define`. Use a block to customize the class.
                end
            "},
            "Person = Data.define(:first_name, :last_name)\n",
        );
    }

    #[test]
    fn flags_data_inheritance_with_do_end_block() {
        test::<DataInheritance>().expect_correction(
            indoc! {"
                class Person < Data.define(:first_name, :last_name) do end
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Data.define`. Use a block to customize the class.
                end
            "},
            indoc! {"
                Person = Data.define(:first_name, :last_name) do
                end
            "},
        );
    }

    #[test]
    fn flags_data_with_cbase_notation() {
        test::<DataInheritance>().expect_correction(
            indoc! {"
                class Person < ::Data.define(:first_name, :last_name)
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Data.define`. Use a block to customize the class.
                  def age; 42; end
                end
            "},
            indoc! {"
                Person = ::Data.define(:first_name, :last_name) do
                  def age; 42; end
                end
            "},
        );
    }

    #[test]
    fn accepts_plain_class() {
        test::<DataInheritance>().expect_no_offenses(indoc! {"
            class Person
            end
        "});
    }

    #[test]
    fn accepts_assignment_to_data_define() {
        test::<DataInheritance>()
            .expect_no_offenses("Person = Data.define(:first_name, :last_name)\n");
    }

    #[test]
    fn accepts_assignment_to_block_form() {
        test::<DataInheritance>().expect_no_offenses(indoc! {"
            Person = Data.define(:first_name, :last_name) do
              def age; 42; end
            end
        "});
    }

    #[test]
    fn accepts_normal_class_inheritance() {
        test::<DataInheritance>().expect_no_offenses(indoc! {"
            class Person < Animal
            end
        "});
    }

    #[test]
    fn flags_data_with_comment_in_empty_body() {
        test::<DataInheritance>().expect_correction(
            indoc! {"
                class Person < Data.define(:name)
                               ^^^^^^^^^^^^^^^^^^ Don't extend an instance initialized by `Data.define`. Use a block to customize the class.
                  # important note
                end
            "},
            indoc! {"
                Person = Data.define(:name) do
                  # important note
                end
            "},
        );
    }

    #[test]
    fn accepts_namespaced_data_define() {
        // MyNamespace::Data.define is not the built-in Data — must not flag.
        test::<DataInheritance>().expect_no_offenses(indoc! {"
            class Foo < MyNamespace::Data.define(:x)
            end
        "});
    }

    #[test]
    fn accepts_struct_new() {
        // Struct.new is handled by StructInheritance, not DataInheritance.
        test::<DataInheritance>().expect_no_offenses(indoc! {"
            class Person < Struct.new(:name)
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(DataInheritance);
