//! `Metrics/ClassLength` — flag classes whose body exceeds `Max` code lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/ClassLength
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "0.87"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.70, murphy-e7bz.71]
//! notes: >
//!   Mirrors RuboCop's `Metrics::ClassLength` (`CodeLength` mixin +
//!   `Metrics::Utils::CodeLengthCalculator`), verified numerically against
//!   standalone rubocop 1.87.0 (`--only Metrics/ClassLength`).
//!
//!   Three measured shapes, mirroring RuboCop's `on_class`, `on_sclass` and
//!   `on_casgn`:
//!
//!   1. `on_class` — every `class Foo … end`. RuboCop's `code_length` takes the
//!      *classlike* path (`classlike_code_length`, shared with
//!      `Metrics/ModuleLength`), NOT `extract_body`:
//!      - `namespace_module?` — when the class's sole body is itself a
//!        `class`/`module`, the length is 0 (a pure namespace wrapper is never
//!        flagged).
//!      - base count = the lines strictly between the `class Foo` header line and
//!        the `end` line, minus every line covered by an inner `class`/`module`
//!        descendant (`:module`/`:class` only — `sclass` is NOT subtracted, so a
//!        `class << self` block's lines DO count toward the enclosing class),
//!        then dropping blank/comment lines (when `CountComments` is false).
//!      Offense range is the whole class node (RuboCop's non-LSP
//!      `node.source_range`).
//!
//!   2. `on_sclass` — `class << expr … end`. Skipped when nested inside any
//!      `class` ancestor (RuboCop's `return if node.each_ancestor(:class).any?`)
//!      — note this checks `:class` only, so a singleton class inside only a
//!      `module` still fires. `sclass` is NOT classlike (`CLASSLIKE_TYPES` is
//!      `[:class, :module]`), so `code_length(sclass)` uses `extract_body` → the
//!      sclass body → an ordinary body-line count (shared `body_code_length`).
//!      Offense range is the whole sclass node (`node.source_range`).
//!
//!   3. `on_casgn` — `Foo = Class.new do … end` / `Foo = Struct.new(…) do … end`
//!      (RuboCop's `class_definition?` matcher's block arm: `(any_block (send
//!      #global_const?({:Struct :Class}) :new ...) _ $_)`). RuboCop computes
//!      `block_node = node.expression` and passes the *block* (not the casgn) to
//!      `check_code_length`. `code_length(block)` is not classlike, so it uses
//!      `extract_body` → the block body → an ordinary body-line count, and
//!      `location(block)` is the block's `source_range` (NOT the constant name —
//!      this is the deliberate divergence from `Metrics/ModuleLength`, whose
//!      `on_casgn` passes the casgn node and so highlights `loc.name`). The
//!      offense range therefore spans `Class.new … end` / `Struct.new(…) … end`,
//!      starting at the `Class`/`Struct` receiver. Because the match is on the
//!      block, there is NO casgn-scope constraint (unlike ModuleLength's
//!      `module_definition?` `casgn nil?`): a scoped target like
//!      `Bar::Foo = Class.new do … end` still fires (verified == rubocop 1.87.0).
//!
//!   `CountAsOne` (default `[]`) folds each top-level descendant of a named kind
//!   (`array`/`hash`/`heredoc`/`method_call`) to a single line, via RuboCop's
//!   `each_top_level_descendant` (stop at first match, never recurse into a match
//!   or an inner classlike node) + `length - descendant_length + 1`.
//!
//!   Fires when length > Max (default 100). Message:
//!   "Class has too many lines. [length/Max]".
//!
//!   Gap (murphy-e7bz.70): the `omit_length` unbraced-hash fold subtraction is
//!   not applied. This is the same shared `body_code_length` /
//!   `classlike_code_length` limitation `Metrics/ModuleLength` and
//!   `Metrics/MethodLength` document — RuboCop's `CodeLengthCalculator#omit_length`
//!   subtracts the 1-2 "absent brace" lines when an unbraced trailing-hash kwargs
//!   argument is folded as the sole argument of a parenthesized call. Demonstrated
//!   (rubocop 1.87.0, Max 2, `CountAsOne: ['hash']`):
//!   `class C; foo(\n a: 1,\n b: 2\n ); end` → rubocop no offense, Murphy `[3/2]`.
//!
//!   Scope simplification mirrored from `Metrics/ModuleLength` (NOT a bug to fix
//!   here): the casgn arm ignores RuboCop's `find_expression_within_parent`
//!   (masgn / chained-assignment) fallback. (RuboCop's `class_definition?` imposes
//!   no casgn-scope constraint, so scoped constant targets ARE handled — see (3).)
//!
//!   Gap (murphy-e7bz.71): with `CountAsOne: ['heredoc']`, the shared
//!   `heredoc_end_line_of_opener` FIFO logic mispairs *nested interpolated*
//!   heredocs, so the folded heredoc body extent is wrong. Default config (no
//!   `CountAsOne`) is unaffected and matches rubocop.
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 100) — body spans 101 code lines
//! class C
//!   # ... 101 code lines ...
//! end
//!
//! # bad — singleton class (not nested in a class) over the limit
//! class << obj
//!   # ... 101 code lines ...
//! end
//!
//! # bad — Class.new / Struct.new assigned to a constant
//! Foo = Class.new do
//!   # ... 101 code lines ...
//! end
//! ```

use crate::cops::util::{
    FoldableType, body_code_length, classlike_code_length, parse_foldable_types,
};
use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ClassLength;

/// Options for [`ClassLength`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct ClassLengthOptions {
    #[option(
        name = "Max",
        default = 100,
        description = "Maximum allowed class body length in code lines."
    )]
    pub max: i64,
    #[option(
        name = "CountComments",
        default = false,
        description = "Count full-line comments toward the class length."
    )]
    pub count_comments: bool,
    #[option(
        name = "CountAsOne",
        description = "Constructs (array, hash, heredoc, method_call) each counted as one line."
    )]
    pub count_as_one: Vec<String>,
}

#[cop(
    name = "Metrics/ClassLength",
    description = "Avoid classes longer than 100 lines of code.",
    default_severity = "warning",
    default_enabled = true,
    options = ClassLengthOptions,
)]
impl ClassLength {
    /// RuboCop `on_class`: measure every `class … end` via the classlike
    /// code-length path. Offense range is the whole class node.
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ClassLengthOptions>();
        let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
        let length = classlike_code_length(node, opts.count_comments, &foldable_types, cx);
        emit(cx.range(node), length, opts.max, cx);
    }

    /// RuboCop `on_sclass`: `class << expr … end`. Skip when nested inside any
    /// `class` ancestor (note: `:class` only, not `module`). `sclass` is not
    /// classlike, so it is measured via the body (`extract_body`) path. Offense
    /// range is the whole sclass node.
    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `return if node.each_ancestor(:class).any?`.
        if cx
            .ancestors(node)
            .any(|a| matches!(*cx.kind(a), NodeKind::Class { .. }))
        {
            return;
        }
        let NodeKind::Sclass { body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body) = body.get() else {
            // Empty singleton class → length 0, never an offense.
            return;
        };
        let opts = cx.options_or_default::<ClassLengthOptions>();
        let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
        let length = body_code_length(body, opts.count_comments, &foldable_types, cx);
        emit(cx.range(node), length, opts.max, cx);
    }

    /// RuboCop `on_casgn`: `Foo = Class.new do … end` / `Foo = Struct.new(…) do …
    /// end` — measured via the block-body (`extract_body`) path. The offense
    /// range is the whole *block* node (RuboCop passes the block, not the casgn,
    /// so `location` is `node.source_range`, NOT the constant name).
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(block) = class_definition_block(node, cx) else {
            return;
        };
        let Some(body) = cx.block_body(block).get() else {
            // Empty `Class.new`/`Struct.new` block → length 0, never an offense.
            return;
        };
        let opts = cx.options_or_default::<ClassLengthOptions>();
        let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
        let length = body_code_length(body, opts.count_comments, &foldable_types, cx);
        emit(cx.range(block), length, opts.max, cx);
    }
}

/// Emit the offense when `length > max`. `range` is the cop-specific offense
/// location (whole class/sclass node for `on_class`/`on_sclass`, the whole block
/// node for `on_casgn`).
fn emit(range: Range, length: i64, max: i64, cx: &Cx<'_>) {
    if length <= max {
        return;
    }
    let message = format!("Class has too many lines. [{length}/{max}]");
    cx.emit_offense(range, &message, None);
}

/// RuboCop's `on_casgn` path: `block_node = node.expression`, then
/// `block_node.class_definition?` whose block arm is `(any_block (send
/// #global_const?({:Struct :Class}) :new ...) _ $_)` — the casgn's value is a
/// block on `Class.new` / `::Class.new` / `Struct.new` / `::Struct.new`. Returns
/// the block node when matched.
///
/// Unlike `Metrics/ModuleLength` (whose `module_definition?` matcher embeds a
/// `casgn nil?` constraint), `class_definition?` is matched on the *block*, so it
/// imposes NO constraint on the casgn's constant scope: a scoped target like
/// `Bar::Foo = Class.new do … end` still fires (verified == rubocop 1.87.0).
///
/// (RuboCop's `find_expression_within_parent` masgn/chained-assignment fallback
/// is intentionally not modelled — that pre-existing simplification is shared
/// with `Metrics/ModuleLength`.)
fn class_definition_block(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
        return None;
    };
    let block = value.get()?;
    // `any_block` — Block/Numblock/Itblock; `cx.block_call` delegates each form.
    let call = cx.block_call(block).get()?;
    // `(send #global_const?({:Struct :Class}) :new ...)`.
    if cx.method_name(call) != Some("new") {
        return None;
    }
    let recv = cx.call_receiver(call).get()?;
    if !cx.is_global_const(recv, "Class") && !cx.is_global_const(recv, "Struct") {
        return None;
    }
    Some(block)
}

murphy_plugin_api::submit_cop!(ClassLength);

#[cfg(test)]
mod tests {
    use super::{ClassLength, ClassLengthOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn opts(max: i64) -> ClassLengthOptions {
        ClassLengthOptions {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
        }
    }

    /// Run the cop and return offense messages. The `on_class`/`on_sclass`
    /// offense range is the whole (multiline) node, which `expect_offense`'s
    /// caret form cannot express; these tests assert the message (and so the
    /// `[length/Max]` count).
    fn messages(opts: &ClassLengthOptions, source: &str) -> Vec<String> {
        run_cop_with_options::<ClassLength>(source, opts)
            .into_iter()
            .map(|o| o.message)
            .collect()
    }

    #[test]
    fn flags_class_over_max() {
        // 5 body code lines > Max 3 (verified == rubocop 1.87.0: [5/3]).
        let src = indoc! {"
            class Foo
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [5/3]".to_string()]
        );
    }

    #[test]
    fn accepts_class_at_max() {
        // Exactly 3 code lines → not > 3.
        test::<ClassLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            class Foo
              a = 1
              b = 2
              c = 3
            end
        "});
    }

    #[test]
    fn accepts_empty_class() {
        test::<ClassLength>()
            .with_options(&opts(0))
            .expect_no_offenses("class Foo; end\n");
    }

    #[test]
    fn blank_lines_not_counted() {
        // a = 1, b = 2 with interspersed blank lines → 2 code lines.
        let src = indoc! {"
            class Foo
              a = 1


              b = 2

            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Class has too many lines. [2/1]".to_string()]
        );
        test::<ClassLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_not_counted_by_default() {
        // Interior comments excluded by default → 2 code lines.
        let src = indoc! {"
            class Foo
              a = 1
              # comment 1
              # comment 2
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Class has too many lines. [2/1]".to_string()]
        );
        test::<ClassLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_counted_when_enabled() {
        // CountComments: true → a, #c1, #c2, b = 4 code lines (verified ==
        // rubocop 1.87.0: [4/3]).
        let cc = ClassLengthOptions {
            max: 3,
            count_comments: true,
            count_as_one: Vec::new(),
        };
        let src = indoc! {"
            class Foo
              a = 1
              # comment 1
              # comment 2
              b = 2
            end
        "};
        assert_eq!(
            messages(&cc, src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn namespace_class_not_measured() {
        // Outer's body is a single inner class → Outer measures 0
        // (`namespace_module?`); only Inner is measured. Inner has 5 lines > 3.
        // (verified == rubocop 1.87.0: only Inner fires [5/3].)
        let src = indoc! {"
            class Outer
              class Inner
                a = 1
                b = 2
                c = 3
                d = 4
                e = 5
              end
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [5/3]".to_string()]
        );
    }

    #[test]
    fn inner_class_lines_subtracted() {
        // Outer: a,b plus a 3-line inner class → Outer counts a,b = 2 ≤ 3 and
        // Inner counts x,y,z = 3 ≤ 3, so NEITHER fires (verified == rubocop
        // 1.87.0: no offenses).
        test::<ClassLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            class Outer
              a = 1
              class Inner
                x = 1
                y = 2
                z = 3
              end
              b = 2
            end
        "});
    }

    #[test]
    fn module_namespace_inner_class_only_measures_class() {
        // A `module` body is subtracted from a class's count too (RuboCop passes
        // both `:module` and `:class` to `line_numbers_of_inner_nodes`). Outer
        // class: a,b = 2 own code lines + 3-line inner module subtracted → 2 ≤ 3,
        // so the class does not fire (verified == rubocop 1.87.0: no
        // Metrics/ClassLength offense — ModuleLength would fire on the module,
        // but that is a different cop).
        test::<ClassLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            class Outer
              a = 1
              module Inner
                x = 1
                y = 2
                z = 3
              end
              b = 2
            end
        "});
    }

    #[test]
    fn sclass_lines_count_toward_enclosing_class() {
        // `sclass` is NOT subtracted as an inner classlike node, so the singleton
        // class body lines DO count toward the enclosing class. The inner sclass
        // is itself skipped (nested in a `class`). Outer Foo measures the 3 def
        // lines + the `class << self`/`end` lines that are non-blank → 5 > 3
        // (verified == rubocop 1.87.0: only Foo fires [5/3]).
        let src = indoc! {"
            class Foo
              class << self
                def a; end
                def b; end
                def c; end
              end
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [5/3]".to_string()]
        );
    }

    #[test]
    fn top_level_sclass_fires() {
        // A top-level `class << obj` (no class ancestor) is measured via the body
        // path: a,b,c,d = 4 > 3 (verified == rubocop 1.87.0: [4/3]).
        let src = indoc! {"
            class << obj
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn sclass_inside_module_only_fires() {
        // `on_sclass` skips only when an ancestor is a `class`; an sclass inside
        // only a `module` still fires. Body a,b,c,d = 4 > 3 (verified == rubocop
        // 1.87.0: [4/3]).
        let src = indoc! {"
            module M
              class << self
                def a; end
                def b; end
                def c; end
                def d; end
              end
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn accepts_empty_sclass() {
        test::<ClassLength>()
            .with_options(&opts(0))
            .expect_no_offenses("class << obj; end\n");
    }

    #[test]
    fn count_as_one_array() {
        // CountAsOne: ['array'] folds the 4-line array to 1 → A(folded)=1 + b=1
        // = 2 ≤ 3, no offense (verified == rubocop 1.87.0).
        let with_fold = ClassLengthOptions {
            max: 3,
            count_comments: false,
            count_as_one: vec!["array".to_string()],
        };
        test::<ClassLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            class Foo
              A = [
                1,
                2,
                3
              ]
              b = 2
            end
        "});
    }

    #[test]
    fn count_as_one_array_disabled_fires() {
        // Without folding the array spans 5 lines + b → 6 > 3 (verified ==
        // rubocop 1.87.0: [6/3]).
        let src = indoc! {"
            class Foo
              A = [
                1,
                2,
                3
              ]
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [6/3]".to_string()]
        );
    }

    #[test]
    fn count_as_one_heredoc() {
        // Folds the heredoc to 1 → length 1 ≤ 1, no offense (verified ==
        // rubocop 1.87.0: no Metrics/ClassLength offense).
        let with_fold = ClassLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["heredoc".to_string()],
        };
        test::<ClassLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            class Foo
              MSG = <<~TEXT
                line one
                line two
              TEXT
            end
        "});
    }

    #[test]
    fn omit_length_fold_gap_overcounts() {
        // GAP (murphy-e7bz.70): RuboCop's `omit_length` subtracts the 1-2
        // "absent brace" lines when an unbraced trailing-hash kwargs argument is
        // folded as the sole arg of a parenthesized call. Murphy does not, so it
        // over-counts. rubocop 1.87.0 (Max 2, CountAsOne ['hash']): no offense;
        // Murphy: [3/2]. This test pins the current (divergent) behavior; flip it
        // to `expect_no_offenses` when murphy-e7bz.70 lands.
        let with_fold = ClassLengthOptions {
            max: 2,
            count_comments: false,
            count_as_one: vec!["hash".to_string()],
        };
        let src = indoc! {"
            class Foo
              foo(
                a: 1,
                b: 2
              )
            end
        "};
        assert_eq!(
            messages(&with_fold, src),
            vec!["Class has too many lines. [3/2]".to_string()]
        );
    }

    #[test]
    fn casgn_class_new_form_fires() {
        // `Foo = Class.new do … end` — offense on the whole block node
        // (`Class.new do … end`), NOT the constant name (verified == rubocop
        // 1.87.0: start_column 7, [4/3]). The block range is multiline (starts at
        // `Class`, ends at `end`), so the caret form cannot express it; the
        // start-column is pinned separately in `casgn_offense_range_is_block`.
        let src = indoc! {"
            Foo = Class.new do
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn casgn_offense_range_is_block() {
        // Pin the casgn offense *range*: RuboCop highlights the whole block node
        // (`Class.new do … end`), starting at the `Class` receiver (byte 6 =
        // column 7), NOT the constant name `Foo`. This is the deliberate
        // divergence from ModuleLength.
        let src = indoc! {"
            Foo = Class.new do
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        let offenses = run_cop_with_options::<ClassLength>(src, &opts(3));
        assert_eq!(offenses.len(), 1, "expected exactly one offense");
        let range = offenses[0].range;
        // `Class` begins at byte offset 6 (`Foo = ` is 6 bytes).
        assert_eq!(range.start, 6, "offense should start at `Class`, not `Foo`");
        // The range ends at the closing `end` (last byte of the block node).
        let end_str = &src[range.start as usize..range.end as usize];
        assert!(
            end_str.starts_with("Class.new do") && end_str.ends_with("end"),
            "offense range should span the whole block: got {end_str:?}"
        );
    }

    #[test]
    fn casgn_struct_new_form_fires() {
        // `Foo = Struct.new(:a, :b) do … end` — block body x,y,z,w = 4 > 3
        // (verified == rubocop 1.87.0: [4/3]).
        let src = indoc! {"
            Foo = Struct.new(:a, :b) do
              x = 1
              y = 2
              z = 3
              w = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn casgn_class_new_at_max_accepted() {
        test::<ClassLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            Foo = Class.new do
              a = 1
              b = 2
              c = 3
            end
        "});
    }

    #[test]
    fn casgn_struct_new_without_block_not_matched() {
        // `class_definition?` requires `any_block`; a bare `Struct.new(...)` with
        // no block is not matched (verified == rubocop 1.87.0: no offense).
        test::<ClassLength>().with_options(&opts(0)).expect_no_offenses(
            "Foo = Struct.new(:a, :b, :c, :d, :e)\n",
        );
    }

    #[test]
    fn casgn_module_new_not_matched() {
        // `Module.new` is ModuleLength's territory, not ClassLength's.
        test::<ClassLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            Foo = Module.new do
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn casgn_scoped_class_new_fires() {
        // Divergence from ModuleLength: `class_definition?` is matched on the
        // block and imposes NO casgn-scope constraint, so a scoped target
        // `Bar::Foo = Class.new do … end` still fires. Block body a,b,c,d = 4 > 3
        // (verified == rubocop 1.87.0: [4/3], offense at the `Class` receiver).
        let src = indoc! {"
            Bar::Foo = Class.new do
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn cbase_class_new_matched() {
        // `(const cbase :Class)` — `::Class.new` is matched.
        let src = indoc! {"
            Foo = ::Class.new do
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Class has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn plain_module_not_measured() {
        // A `module` node is ModuleLength's job; ClassLength has no on_module.
        test::<ClassLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            module Foo
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn default_max_100_does_not_fire_on_small_class() {
        // Default Max 100 with a tiny class → no offense.
        test::<ClassLength>().expect_no_offenses(indoc! {"
            class Foo
              a = 1
            end
        "});
    }
}
