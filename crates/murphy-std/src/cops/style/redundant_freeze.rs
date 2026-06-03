//! `Style/RedundantFreeze` — flags `Object#freeze` calls on immutable objects.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantFreeze
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered patterns:
//!     - Freezing immutable literals: int, float, sym, true, false, nil, rational, complex.
//!     - Freezing a plain `str` when `# frozen_string_literal: true` is present.
//!       `dstr` (interpolated strings) are intentionally excluded — they are not
//!       frozen by the pragma in Ruby 3.0+, so the freeze is not redundant.
//!     - Freezing `regexp` or `range` literals (frozen since Ruby 3.0; Murphy
//!       targets Ruby 3.0+ so no version gate is applied).
//!     - Freezing the result of `count`/`length`/`size` method calls
//!       (including block-wrapped forms via Block/Numblock/Itblock).
//!       Note: this is a heuristic — user-defined `count`/`length`/`size` methods
//!       could return mutable objects. This matches RuboCop's own approach, which
//!       applies the same heuristic without a version guard.
//!   Safety:
//!     - Autocorrect is unsafe for the `count`/`length`/`size` patterns: a
//!       user-defined method with one of those names could return a mutable object,
//!       so deleting `.freeze` could change observable behavior.
//!   Gaps:
//!     - Parenthesized arithmetic/comparison patterns:
//!       `(1 + 2).freeze`, `(a == b).freeze`, `(int op int).freeze`.
//!       RuboCop uses `begin` node (parenthesized expression wrapper) for these,
//!       but Murphy's arena AST emits `NodeKind::Unknown` for parenthesized
//!       subexpressions, making the inner operation inaccessible. This would
//!       require a Murphy AST enhancement to support.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! CONST = 1.freeze
//! CONST = :foo.freeze
//! CONST = true.freeze
//! CONST = /regex/.freeze
//! CONST = arr.count.freeze
//!
//! # good
//! CONST = 1
//! CONST = "mutable".freeze   # str without frozen_string_literal pragma
//! CONST = [1, 2, 3].freeze   # array is mutable
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantFreeze;

const MSG: &str = "Do not freeze immutable objects, as freezing them has no effect.";

#[cop(
    name = "Style/RedundantFreeze",
    description = "Checks for uses of `Object#freeze` on immutable objects.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl RedundantFreeze {
    #[on_node(kind = "send", methods = ["freeze"])]
    fn check_freeze(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return;
    };

    // `freeze` takes no arguments.
    if !cx.list(args).is_empty() {
        return;
    }

    let receiver = match cx.call_receiver(node).get() {
        Some(r) => r,
        None => return,
    };

    if !is_redundant_freeze_receiver(receiver, cx) {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect: delete the dot + "freeze" (including any trailing empty parens).
    // Using cx.range(node).end covers `1.freeze()` → deletes `.freeze()`, not just `.freeze`.
    if let Some(dot) = cx.call_operator_loc(node) {
        let delete_range = Range {
            start: dot.start,
            end: cx.range(node).end,
        };
        cx.emit_edit(delete_range, "");
    }
}

/// Returns true when calling `.freeze` on `receiver` is redundant.
fn is_redundant_freeze_receiver(receiver: NodeId, cx: &Cx<'_>) -> bool {
    // 1. Plain immutable literal (int, float, sym, true, false, nil, rational, complex).
    if cx.is_immutable_literal(receiver) {
        return true;
    }

    // 2. Regexp literal — frozen since Ruby 3.0, which Murphy targets.
    if matches!(cx.kind(receiver), NodeKind::Regexp { .. }) {
        return true;
    }

    // 3. Range literal — frozen since Ruby 3.0.
    if matches!(cx.kind(receiver), NodeKind::RangeExpr { .. }) {
        return true;
    }

    // 4. Plain `str` with `# frozen_string_literal: true` pragma.
    //    `dstr` is intentionally excluded (not frozen by the pragma in Ruby 3.0+).
    if matches!(cx.kind(receiver), NodeKind::Str(_))
        && let Some(comment) = cx.frozen_string_literal_comment()
            && comment.value_bool == 1 {
                return true;
            }

    // 5. Operations that produce immutable values.
    operation_produces_immutable(receiver, cx)
}

/// Returns true when the node is an operation likely to produce
/// an immutable (numeric or boolean) result.
///
/// Patterns covered:
///   - `recv.count/length/size(...)` — heuristic: these methods return Integer
///     on all standard Ruby collections. User-defined overrides could return
///     mutable objects, which is why the autocorrect is marked unsafe.
///   - Block-wrapped `count/length/size` call
///
/// Note: Parenthesized arithmetic/comparison patterns (`(1+2).freeze`, etc.)
/// are not handled because Murphy's arena AST emits `NodeKind::Unknown` for
/// parenthesized subexpressions, so the inner operation cannot be inspected.
fn operation_produces_immutable(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { method, .. } => {
            let method_name = cx.symbol_str(method);
            // count/length/size heuristic: standard collections return Integer;
            // user-defined overrides could differ (autocorrect is unsafe)
            matches!(method_name, "count" | "length" | "size")
        }
        // Block-wrapped count/length/size: `arr.count { ... }.freeze`
        NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } => {
            if let Some(name) = cx.method_name(call) {
                matches!(name, "count" | "length" | "size")
            } else {
                false
            }
        }
        NodeKind::Itblock { send, .. } => {
            if let Some(name) = cx.method_name(send) {
                matches!(name, "count" | "length" | "size")
            } else {
                false
            }
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantFreeze;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Immutable literals ---

    #[test]
    fn flags_integer_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = 1.freeze
                    ^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_integer_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = 1.freeze
                        ^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = 1\n",
        );
    }

    #[test]
    fn flags_float_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = 1.5.freeze
                    ^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_float_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = 1.5.freeze
                        ^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = 1.5\n",
        );
    }

    #[test]
    fn flags_symbol_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = :foo.freeze
                    ^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_symbol_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = :foo.freeze
                        ^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = :foo\n",
        );
    }

    #[test]
    fn flags_true_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = true.freeze
                    ^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_true_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = true.freeze
                        ^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = true\n",
        );
    }

    #[test]
    fn flags_false_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = false.freeze
                    ^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_false_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = false.freeze
                        ^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = false\n",
        );
    }

    #[test]
    fn flags_nil_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = nil.freeze
                    ^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_nil_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = nil.freeze
                        ^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = nil\n",
        );
    }

    // --- Regexp and Range (Ruby 3.0+) ---

    #[test]
    fn flags_regexp_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = /foo/.freeze
                    ^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_regexp_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = /foo/.freeze
                        ^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = /foo/\n",
        );
    }

    // --- String with frozen_string_literal: true ---

    #[test]
    fn flags_string_freeze_with_frozen_pragma() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            # frozen_string_literal: true
            CONST = 'hello'.freeze
                    ^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_string_freeze_with_frozen_pragma() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                # frozen_string_literal: true
                CONST = 'hello'.freeze
                        ^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "# frozen_string_literal: true\nCONST = 'hello'\n",
        );
    }

    #[test]
    fn accepts_string_freeze_without_frozen_pragma() {
        test::<RedundantFreeze>().expect_no_offenses("CONST = 'hello'.freeze\n");
    }

    #[test]
    fn accepts_interpolated_string_freeze_with_frozen_pragma() {
        // dstr (interpolated strings) are not frozen by the pragma in Ruby 3.0+
        test::<RedundantFreeze>().expect_no_offenses(
            "# frozen_string_literal: true\nCONST = \"hello #{name}\".freeze\n",
        );
    }

    // --- count/length/size operations ---

    #[test]
    fn flags_count_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = arr.count.freeze
                    ^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_count_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = arr.count.freeze
                        ^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = arr.count\n",
        );
    }

    #[test]
    fn flags_length_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = arr.length.freeze
                    ^^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_length_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = arr.length.freeze
                        ^^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = arr.length\n",
        );
    }

    #[test]
    fn flags_size_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = arr.size.freeze
                    ^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_size_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = arr.size.freeze
                        ^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = arr.size\n",
        );
    }

    #[test]
    fn flags_count_block_freeze() {
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = arr.count { |x| x > 1 }.freeze
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    #[test]
    fn corrects_count_block_freeze() {
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = arr.count { |x| x > 1 }.freeze
                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = arr.count { |x| x > 1 }\n",
        );
    }

    // --- count/length/size heuristic is intentional ---

    #[test]
    fn flags_count_on_arbitrary_receiver() {
        // This is intentional: same heuristic as RuboCop.
        // count/length/size on user-defined objects *could* return mutable values,
        // but we flag them to match RuboCop's behavior. Autocorrect is unsafe.
        test::<RedundantFreeze>().expect_offense(indoc! {"
            CONST = foo.count.freeze
                    ^^^^^^^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
        "});
    }

    // --- Negative cases (no offense) ---

    #[test]
    fn accepts_mutable_array_freeze() {
        test::<RedundantFreeze>().expect_no_offenses("CONST = [1, 2, 3].freeze\n");
    }

    #[test]
    fn accepts_mutable_hash_freeze() {
        test::<RedundantFreeze>().expect_no_offenses("CONST = { a: 1 }.freeze\n");
    }

    #[test]
    fn accepts_method_result_freeze() {
        test::<RedundantFreeze>().expect_no_offenses("CONST = foo.freeze\n");
    }

    #[test]
    fn accepts_no_receiver_freeze() {
        // bare `freeze` with no receiver
        test::<RedundantFreeze>().expect_no_offenses("freeze\n");
    }

    #[test]
    fn corrects_integer_freeze_with_empty_parens() {
        // `1.freeze()` should autocorrect to `1` (not `1()`)
        test::<RedundantFreeze>().expect_correction(
            indoc! {"
                CONST = 1.freeze()
                        ^^^^^^^^^^ Do not freeze immutable objects, as freezing them has no effect.
            "},
            "CONST = 1\n",
        );
    }

    #[test]
    fn accepts_string_freeze_with_false_pragma() {
        test::<RedundantFreeze>().expect_no_offenses(
            "# frozen_string_literal: false\nCONST = 'hello'.freeze\n",
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantFreeze);
