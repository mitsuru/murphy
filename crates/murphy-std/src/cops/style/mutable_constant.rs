//! `Style/MutableConstant` — flags mutable literal objects assigned to constants
//! and suggests adding `.freeze`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MutableConstant
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy handles the common `literals` mode: flags Array, Hash, Str, Dstr
//!   (interpolated string), and Xstr (backtick string) literals assigned to
//!   constants without `.freeze`. Regexp and RangeExpr literals are excluded
//!   because Murphy targets Ruby 3.0+ where those are immutable by default.
//!   Plain Str literals are excluded when `# frozen_string_literal: true` is
//!   set (mirrors RedundantFreeze reasoning).
//!   `strict` mode additionally flags non-literal mutable values (e.g. `Foo.new`
//!   or any other arbitrary method call/expression).
//!   Autocorrect is marked unsafe (matching RuboCop's SafeAutoCorrect: false),
//!   since adding `.freeze` changes mutation behavior from accepted to raising
//!   `FrozenError`.
//!   Parity gaps vs RuboCop:
//!   - Unbracketed array `CONST = 1, 2` is not wrapped to `[1, 2].freeze`;
//!     Murphy's AST represents this as a plain Array node. We add `.freeze`
//!     which produces `1, 2.freeze` — wrong. These are skipped for now.
//!   - Splat array `CONST = [*x]` is not converted to `.to_a.freeze`.
//!   - `# shareable_constant_value` magic comment is not respected.
//!   - `strict` mode does not cover all RuboCop patterns (e.g. `Struct.new`
//!     arguments aren't analyzed).
//! ```
//!
//! ## Matched shapes (literals mode)
//!
//! `Casgn` nodes whose value is a mutable literal that is not already frozen:
//! - `Array` (but only when it has a bracket opener `[` — unbracketed skipped)
//! - `Hash`
//! - `Str` (unless `# frozen_string_literal: true` is present)
//! - `Dstr` (interpolated string — never frozen by the pragma)
//! - `Xstr` (backtick string)
//!
//! ## Matched shapes (strict mode)
//!
//! Additionally flags any non-immutable value that is not already a `.freeze` call.
//!
//! ## Autocorrect (unsafe)
//!
//! Inserts `.freeze` at the end of the value node range.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Freeze mutable objects assigned to constants.";

#[derive(Default)]
pub struct MutableConstant;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Flag only mutable literals.
    #[default]
    #[option(value = "literals")]
    Literals,
    /// Flag all non-immutable constant assignments.
    #[option(value = "strict")]
    Strict,
}

#[derive(CopOptions)]
pub struct MutableConstantOptions {
    #[option(
        name = "EnforcedStyle",
        default = "literals",
        description = "Selects which assignments to flag: `literals` flags only mutable literals; `strict` flags all mutable constant assignments."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/MutableConstant",
    description = "Freeze mutable objects assigned to constants.",
    default_severity = "warning",
    default_enabled = true,
    options = MutableConstantOptions,
    safe_autocorrect = false,
)]
impl MutableConstant {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Casgn { value, .. } = cx.kind(node) else {
        return;
    };
    let Some(value) = value.get() else {
        // Deconstructed pattern assignment (e.g. `CONST = nil` in `Casgn` with no value) — skip.
        return;
    };

    // Skip if the value is already `.freeze`.
    if is_freeze_call(value, cx) {
        return;
    }

    let opts = cx.options_or_default::<MutableConstantOptions>();

    let should_flag = match opts.enforced_style {
        EnforcedStyle::Literals => is_flagged_mutable_literal(value, cx),
        EnforcedStyle::Strict => is_flagged_strict(value, cx),
    };

    if !should_flag {
        return;
    }

    // For unbracketed arrays (source doesn't start with `[`), skip autocorrect.
    // Inserting `.freeze` at end of the array source would bind to the last element,
    // producing invalid Ruby. A proper fix requires wrapping to `[...].freeze`.
    let is_unbracketed_array = is_unbracketed_array(value, cx);

    // For complex expressions (ternaries, compound operations, etc.), appending
    // `.freeze` may bind at wrong precedence. Only autocorrect when safe.
    let safe_to_autocorrect = !is_unbracketed_array && is_safe_freeze_target(value, cx);

    cx.emit_offense(cx.range(value), MSG, None);

    if safe_to_autocorrect {
        // Autocorrect: insert `.freeze` at end of value range.
        cx.emit_edit(
            Range { start: cx.range(value).end, end: cx.range(value).end },
            ".freeze",
        );
    }
}

/// Returns true when appending `.freeze` at the end of this node's source range
/// is safe — i.e., `.freeze` will bind to the whole expression rather than to
/// just the last sub-expression.
///
/// Safe shapes:
/// - Literals: Array `[...]`, Hash `{...}`, Str, Dstr, Xstr, Regexp
/// - Method calls with parentheses: `foo()`, `Foo.new(x)` (`.freeze` binds clearly)
/// - No-argument method calls: `foo`, `Foo.new` (no precedence hazard)
/// - Block calls: `Block`/`Numblock`/`Itblock`
/// - Constants: `Const`
///
/// Unsafe shapes (would need parentheses):
/// - Command-style calls with arguments and no parens: `foo 1, 2`
///   → `.freeze` binds to last arg, not the return value
/// - Ternary/If, And/Or, Assignment forms, Begin, etc.
fn is_safe_freeze_target(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        // Literals: brackets/braces/quotes ensure `.freeze` binds to whole literal.
        NodeKind::Array(_)
        | NodeKind::Hash(_)
        | NodeKind::Str(_)
        | NodeKind::Dstr(_)
        | NodeKind::Xstr(_)
        | NodeKind::Regexp { .. } => true,

        // Send/Csend: safe if there are no arguments OR if the call uses parentheses.
        // Command-style calls without parens (`foo 1, 2`) are NOT safe — `.freeze`
        // would bind to the last argument rather than the return value.
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            cx.call_arguments(node).is_empty() || cx.is_parenthesized(node)
        }

        // Block-form calls: `foo { ... }` or `foo do ... end` — `.freeze` binds to
        // the block's return value unambiguously.
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => true,

        // Constants: `Foo`, `::Bar` — no arguments, no precedence hazard.
        NodeKind::Const { .. } => true,

        // Everything else (ternary, compound expressions, etc.) is unsafe.
        _ => false,
    }
}

/// Returns true when `node` is a `.freeze` call with no arguments on any receiver.
fn is_freeze_call(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(node) == Some("freeze") && cx.call_arguments(node).is_empty()
}

/// Returns true for mutable literals that should be flagged in `literals` mode.
///
/// The flagged set is `cx.is_mutable_literal` **minus**:
/// - `Regexp` and `RangeExpr` (frozen since Ruby 3.0+, which Murphy targets)
/// - plain `Str` when `# frozen_string_literal: true` is present (already frozen)
///
/// `Dstr` and `Xstr` are included regardless of the pragma because dstr is NOT
/// frozen by `frozen_string_literal: true` in Ruby 3.0+ (mirrors RedundantFreeze).
fn is_flagged_mutable_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        // Regexp is frozen in Ruby 3.0+ — don't flag.
        NodeKind::Regexp { .. } => false,
        // RangeExpr is frozen in Ruby 3.0+ — don't flag.
        NodeKind::RangeExpr { .. } => false,
        // Plain string: skip if frozen_string_literal: true.
        NodeKind::Str(_) => {
            if let Some(comment) = cx.frozen_string_literal_comment() {
                comment.value_bool != 1
            } else {
                true
            }
        }
        // All other mutable literals (Array, Hash, Dstr, Xstr) are flagged.
        _ => cx.is_mutable_literal(node),
    }
}

/// Returns true for values that should be flagged in `strict` mode.
///
/// In strict mode, anything that is not demonstrably immutable is flagged.
/// Specifically, we flag anything that:
/// - Is a mutable literal (via `literals` mode logic), OR
/// - Is not an immutable literal (i.e., not a numeric, bool, nil, sym, etc.)
fn is_flagged_strict(node: NodeId, cx: &Cx<'_>) -> bool {
    // Already-frozen is excluded by the caller.
    // If it's immutable (numeric, bool, nil, sym, frozen regexp/range), accept.
    if cx.is_immutable_literal(node) {
        return false;
    }
    // Regexp and RangeExpr: frozen in 3.0+, don't flag.
    if matches!(cx.kind(node), NodeKind::Regexp { .. } | NodeKind::RangeExpr { .. }) {
        return false;
    }
    // Plain str with frozen_string_literal: true — skip.
    if let NodeKind::Str(_) = cx.kind(node) {
        if let Some(comment) = cx.frozen_string_literal_comment() {
            if comment.value_bool == 1 {
                return false;
            }
        }
    }
    // Everything else is mutable — flag it.
    true
}

/// Returns true when `node` is an `Array` literal without a leading `[` token.
/// E.g., `CONST = 1, 2` parses as `(casgn :CONST nil (array (int 1) (int 2)))` but
/// the source text is `1, 2` without brackets. Inserting `.freeze` at value end
/// would yield `1, 2.freeze` which binds to `2` only — wrong.
fn is_unbracketed_array(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Array(_)) {
        return false;
    }
    let src = cx.raw_source(cx.range(node));
    !src.starts_with('[')
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, MutableConstant, MutableConstantOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Array literals -----

    #[test]
    fn flags_array_literal() {
        test::<MutableConstant>().expect_offense(indoc! {"
            CONST = [1, 2, 3]
                    ^^^^^^^^^ Freeze mutable objects assigned to constants.
        "});
    }

    #[test]
    fn corrects_array_literal() {
        test::<MutableConstant>().expect_correction(
            indoc! {"
                CONST = [1, 2, 3]
                        ^^^^^^^^^ Freeze mutable objects assigned to constants.
            "},
            "CONST = [1, 2, 3].freeze\n",
        );
    }

    #[test]
    fn accepts_frozen_array() {
        test::<MutableConstant>().expect_no_offenses("CONST = [1, 2, 3].freeze\n");
    }

    // ----- Hash literals -----

    #[test]
    fn flags_hash_literal() {
        test::<MutableConstant>().expect_offense(indoc! {"
            CONST = { a: 1 }
                    ^^^^^^^^ Freeze mutable objects assigned to constants.
        "});
    }

    #[test]
    fn corrects_hash_literal() {
        test::<MutableConstant>().expect_correction(
            indoc! {"
                CONST = { a: 1 }
                        ^^^^^^^^ Freeze mutable objects assigned to constants.
            "},
            "CONST = { a: 1 }.freeze\n",
        );
    }

    #[test]
    fn accepts_frozen_hash() {
        test::<MutableConstant>().expect_no_offenses("CONST = { a: 1 }.freeze\n");
    }

    // ----- String literals -----

    #[test]
    fn flags_string_literal_without_frozen_pragma() {
        test::<MutableConstant>().expect_offense(indoc! {"
            CONST = 'hello'
                    ^^^^^^^ Freeze mutable objects assigned to constants.
        "});
    }

    #[test]
    fn corrects_string_literal() {
        test::<MutableConstant>().expect_correction(
            indoc! {"
                CONST = 'hello'
                        ^^^^^^^ Freeze mutable objects assigned to constants.
            "},
            "CONST = 'hello'.freeze\n",
        );
    }

    #[test]
    fn accepts_string_literal_with_frozen_pragma() {
        test::<MutableConstant>().expect_no_offenses(
            "# frozen_string_literal: true\nCONST = 'hello'\n",
        );
    }

    #[test]
    fn flags_string_literal_with_false_frozen_pragma() {
        test::<MutableConstant>().expect_offense(indoc! {"
            # frozen_string_literal: false
            CONST = 'hello'
                    ^^^^^^^ Freeze mutable objects assigned to constants.
        "});
    }

    #[test]
    fn accepts_frozen_string() {
        test::<MutableConstant>().expect_no_offenses("CONST = 'hello'.freeze\n");
    }

    // ----- Interpolated string (dstr) -----

    #[test]
    fn flags_interpolated_string_even_with_frozen_pragma() {
        // dstr is NOT frozen by frozen_string_literal: true in Ruby 3.0+
        test::<MutableConstant>().expect_offense(
            "# frozen_string_literal: true\nCONST = \"hello #{name}\"\n\
             \x20\x20\x20\x20\x20\x20\x20\x20^^^^^^^^^^^^^^^^ Freeze mutable objects assigned to constants.\n",
        );
    }

    // ----- Regexp and Range (frozen in Ruby 3.0+ — should NOT flag) -----

    #[test]
    fn accepts_regexp_literal() {
        test::<MutableConstant>().expect_no_offenses("CONST = /foo/\n");
    }

    #[test]
    fn accepts_range_literal() {
        test::<MutableConstant>().expect_no_offenses("CONST = 1..10\n");
    }

    // ----- Immutable literals (should NOT flag) -----

    #[test]
    fn accepts_integer_constant() {
        test::<MutableConstant>().expect_no_offenses("CONST = 42\n");
    }

    #[test]
    fn accepts_symbol_constant() {
        test::<MutableConstant>().expect_no_offenses("CONST = :foo\n");
    }

    #[test]
    fn accepts_true_constant() {
        test::<MutableConstant>().expect_no_offenses("CONST = true\n");
    }

    #[test]
    fn accepts_nil_constant() {
        test::<MutableConstant>().expect_no_offenses("CONST = nil\n");
    }

    // ----- Strict mode -----

    #[test]
    fn strict_mode_flags_method_call() {
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_offense(indoc! {"
                CONST = Foo.new
                        ^^^^^^^ Freeze mutable objects assigned to constants.
            "});
    }

    #[test]
    fn strict_mode_corrects_method_call() {
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_correction(
                indoc! {"
                    CONST = Foo.new
                            ^^^^^^^ Freeze mutable objects assigned to constants.
                "},
                "CONST = Foo.new.freeze\n",
            );
    }

    #[test]
    fn strict_mode_still_accepts_immutable_literals() {
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_no_offenses("CONST = 42\n");
    }

    #[test]
    fn strict_mode_still_accepts_frozen_value() {
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_no_offenses("CONST = [1, 2].freeze\n");
    }

    #[test]
    fn strict_mode_still_accepts_regexp() {
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_no_offenses("CONST = /foo/\n");
    }

    // ----- Command-style calls (strict mode, no autocorrect) -----

    #[test]
    fn strict_mode_flags_command_call_without_autocorrect() {
        // `foo 1, 2` — command form without parens; offense reported but not autocorrected.
        test::<MutableConstant>()
            .with_options(&MutableConstantOptions { enforced_style: EnforcedStyle::Strict })
            .expect_offense(indoc! {"
                CONST = foo 1, 2
                        ^^^^^^^^ Freeze mutable objects assigned to constants.
            "});
    }

    // ----- Empty array -----

    #[test]
    fn flags_empty_array() {
        test::<MutableConstant>().expect_offense(indoc! {"
            CONST = []
                    ^^ Freeze mutable objects assigned to constants.
        "});
    }

    #[test]
    fn corrects_empty_array() {
        test::<MutableConstant>().expect_correction(
            indoc! {"
                CONST = []
                        ^^ Freeze mutable objects assigned to constants.
            "},
            "CONST = [].freeze\n",
        );
    }
}

murphy_plugin_api::submit_cop!(MutableConstant);
