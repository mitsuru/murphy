//! `Metrics/ModuleLength` — flag modules whose body exceeds `Max` code lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/ModuleLength
//! upstream_version_checked: 1.87.0
//! version_added: "0.31"
//! version_changed: "0.87"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.70, murphy-e7bz.71]
//! notes: >
//!   Mirrors RuboCop's `Metrics::ModuleLength` (`CodeLength` mixin +
//!   `Metrics::Utils::CodeLengthCalculator`), verified numerically against
//!   standalone rubocop 1.87.0 (`--only Metrics/ModuleLength`).
//!
//!   Two measured shapes, mirroring RuboCop's `on_module` and `on_casgn`:
//!
//!   1. `on_module` — every `module Foo … end`. RuboCop's `code_length` takes
//!      the *classlike* path (`classlike_code_length`), NOT `extract_body`:
//!      - `namespace_module?` — when the module's sole body is itself a
//!        `class`/`module`, the length is 0 (a pure namespace wrapper is never
//!        flagged), so `module A; module B; … end; end` measures only `B`.
//!      - base count = the lines strictly between the `module Foo` header line
//!        and the `end` line, minus every line covered by an inner
//!        `class`/`module` descendant, then dropping blank/comment lines (when
//!        `CountComments` is false). Inner-module/class lines are subtracted so
//!        each classlike node is measured independently.
//!      Offense range is the whole module node (RuboCop's non-LSP
//!      `node.source_range`).
//!
//!   2. `on_casgn` — `Foo = Module.new do … end` (RuboCop's `module_definition?`
//!      matcher `(casgn nil? _ (any_block (send (const {nil? cbase} :Module)
//!      :new) ...))`). Here RuboCop passes the *casgn* to `check_code_length`;
//!      `code_length(casgn)` is not classlike, so it uses `extract_body` → the
//!      `Module.new` block body → an ordinary body-line count (shared with
//!      `Metrics/MethodLength` via `body_code_length`). Offense range is the
//!      constant name (RuboCop's `casgn` location = `node.loc.name`).
//!
//!   `CountAsOne` (default `[]`) folds each top-level descendant of a named kind
//!   (`array`/`hash`/`heredoc`/`method_call`) to a single line, via RuboCop's
//!   `each_top_level_descendant` (stop at first match, never recurse into a
//!   match or an inner classlike node) + `length - descendant_length + 1`.
//!
//!   Fires when length > Max (default 100). Message:
//!   "Module has too many lines. [length/Max]".
//!
//!   Gap (murphy-e7bz.70): the `omit_length` unbraced-hash fold subtraction is
//!   not applied. This is the same shared `body_code_length` / fold-loop
//!   limitation `Metrics/MethodLength` documents — both the `on_module`
//!   (`classlike_code_length`) and `on_casgn` (`body_code_length`) paths omit
//!   RuboCop's `CodeLengthCalculator#omit_length`, which subtracts the 1-2
//!   "absent brace" lines when an unbraced trailing-hash kwargs argument is
//!   folded as the sole argument of a parenthesized call. Demonstrated (rubocop
//!   1.87.0, Max 2, `CountAsOne: ['hash']`):
//!   `module M; foo(\n a: 1,\n b: 2\n ); end` → rubocop no offense, Murphy
//!   `[3/2]`. (RuboCop's `each_top_level_descendant` also seeds the casgn fold
//!   walk at the casgn node rather than the block body; that divergence is
//!   benign here — the only extra candidate is the single-line `Module.new`
//!   send, which folds to a no-op.)
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
//! module M
//!   # ... 101 code lines ...
//! end
//!
//! # bad — Module.new assigned to a constant
//! Foo = Module.new do
//!   # ... 101 code lines ...
//! end
//! ```

use crate::cops::util::{
    FoldableType, body_code_length, classlike_code_length, parse_foldable_types,
};
use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ModuleLength;

/// Options for [`ModuleLength`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct ModuleLengthOptions {
    #[option(
        name = "Max",
        default = 100,
        description = "Maximum allowed module body length in code lines."
    )]
    pub max: i64,
    #[option(
        name = "CountComments",
        default = false,
        description = "Count full-line comments toward the module length."
    )]
    pub count_comments: bool,
    #[option(
        name = "CountAsOne",
        description = "Constructs (array, hash, heredoc, method_call) each counted as one line."
    )]
    pub count_as_one: Vec<String>,
}

#[cop(
    name = "Metrics/ModuleLength",
    description = "Avoid modules longer than 100 lines of code.",
    default_severity = "warning",
    default_enabled = true,
    options = ModuleLengthOptions,
)]
impl ModuleLength {
    /// RuboCop `on_module`: measure every `module … end` via the classlike
    /// code-length path.
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ModuleLengthOptions>();
        let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
        let length = classlike_code_length(node, opts.count_comments, &foldable_types, cx);
        emit(cx.range(node), length, opts.max, cx);
    }

    /// RuboCop `on_casgn`: `Foo = Module.new do … end` — measured via the
    /// `extract_body` (block-body) path, with the offense on the constant name.
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(block) = module_new_block(node, cx) else {
            return;
        };
        let Some(body) = cx.block_body(block).get() else {
            // Empty `Module.new` block → length 0, never an offense.
            return;
        };
        let opts = cx.options_or_default::<ModuleLengthOptions>();
        let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
        let length = body_code_length(body, opts.count_comments, &foldable_types, cx);
        emit(casgn_name_range(node, cx), length, opts.max, cx);
    }
}

/// Emit the offense when `length > max`. `range` is the cop-specific offense
/// location (whole module for `on_module`, constant name for `on_casgn`).
fn emit(range: Range, length: i64, max: i64, cx: &Cx<'_>) {
    if length <= max {
        return;
    }
    let message = format!("Module has too many lines. [{length}/{max}]");
    cx.emit_offense(range, &message, None);
}

/// RuboCop `module_definition?`: `(casgn nil? _ (any_block (send (const
/// {nil? cbase} :Module) :new) ...))` — a constant assignment with no scope
/// whose value is a block on `Module.new` / `::Module.new`. Returns the block
/// node when matched.
fn module_new_block(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Casgn { scope, value, .. } = *cx.kind(node) else {
        return None;
    };
    // `casgn nil?` — the constant must have no namespace scope.
    if scope.get().is_some() {
        return None;
    }
    let block = value.get()?;
    // `any_block` — Block/Numblock/Itblock; `cx.block_call` delegates each form.
    let call = cx.block_call(block).get()?;
    // `(send (const {nil? cbase} :Module) :new)`.
    if cx.method_name(call) != Some("new") {
        return None;
    }
    let recv = cx.call_receiver(call).get()?;
    if !cx.is_global_const(recv, "Module") {
        return None;
    }
    Some(block)
}

/// RuboCop's `casgn` offense location = `node.loc.name` (the bare constant). The
/// `module_definition?` matcher guarantees a nil scope, so the constant name
/// sits at the casgn node start; constant names are ASCII, so a byte-length span
/// is exact.
fn casgn_name_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let name_loc = cx.node(node).loc.name;
    // Prefer the recorded name loc when it is a non-empty sub-range of the node.
    let node_range = cx.range(node);
    if name_loc.start >= node_range.start
        && name_loc.end <= node_range.end
        && name_loc.end > name_loc.start
    {
        return name_loc;
    }
    // Fallback: compute from the constant name length at the node start.
    let NodeKind::Casgn { name, .. } = *cx.kind(node) else {
        return node_range;
    };
    let len = cx.symbol_str(name).len() as u32;
    Range {
        start: node_range.start,
        end: node_range.start + len,
    }
}

murphy_plugin_api::submit_cop!(ModuleLength);

#[cfg(test)]
mod tests {
    use super::{ModuleLength, ModuleLengthOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn opts(max: i64) -> ModuleLengthOptions {
        ModuleLengthOptions {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
        }
    }

    /// Run the cop and return offense messages. The `on_module` offense range is
    /// the whole (multiline) module node, which `expect_offense`'s caret form
    /// cannot express; these tests assert the message (and so the `[length/Max]`
    /// count). The casgn-form range is pinned separately via `expect_offense`.
    fn messages(opts: &ModuleLengthOptions, source: &str) -> Vec<String> {
        run_cop_with_options::<ModuleLength>(source, opts)
            .into_iter()
            .map(|o| o.message)
            .collect()
    }

    #[test]
    fn flags_module_over_max() {
        // 5 body code lines > Max 3 (verified == rubocop 1.87.0: [5/3]).
        let src = indoc! {"
            module M
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Module has too many lines. [5/3]".to_string()]
        );
    }

    #[test]
    fn accepts_module_at_max() {
        // Exactly 3 code lines → not > 3.
        test::<ModuleLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            module M
              a = 1
              b = 2
              c = 3
            end
        "});
    }

    #[test]
    fn accepts_empty_module() {
        test::<ModuleLength>()
            .with_options(&opts(0))
            .expect_no_offenses("module M; end\n");
    }

    #[test]
    fn blank_lines_not_counted() {
        // a = 1, b = 2 with interspersed blank lines → 2 code lines.
        let src = indoc! {"
            module M
              a = 1


              b = 2

            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Module has too many lines. [2/1]".to_string()]
        );
        test::<ModuleLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_not_counted_by_default() {
        // Interior comments excluded by default → 2 code lines.
        let src = indoc! {"
            module M
              a = 1
              # comment 1
              # comment 2
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Module has too many lines. [2/1]".to_string()]
        );
        test::<ModuleLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_counted_when_enabled() {
        // CountComments: true → a, #c1, #c2, b = 4 code lines (verified ==
        // rubocop 1.87.0: [4/3]).
        let cc = ModuleLengthOptions {
            max: 3,
            count_comments: true,
            count_as_one: Vec::new(),
        };
        let src = indoc! {"
            module M
              a = 1
              # comment 1
              # comment 2
              b = 2
            end
        "};
        assert_eq!(
            messages(&cc, src),
            vec!["Module has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn namespace_module_not_measured() {
        // Outer's body is a single inner module → Outer measures 0
        // (`namespace_module?`); only Inner is measured. Inner has 5 lines > 3.
        // (verified == rubocop 1.87.0: only Inner fires [5/3].)
        let src = indoc! {"
            module Outer
              module Inner
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
            vec!["Module has too many lines. [5/3]".to_string()]
        );
    }

    #[test]
    fn inner_module_lines_subtracted() {
        // Outer: a,b plus a 3-line inner module → Outer counts a,b = 2 ≤ 3 and
        // Inner counts x,y,z = 3 ≤ 3, so NEITHER fires (verified == rubocop
        // 1.87.0: no offenses).
        test::<ModuleLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            module Outer
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
    fn outer_fires_with_inner_subtracted() {
        // Outer: a,b,c,d = 4 own code lines plus a 1-line inner module. Inner
        // lines subtracted → Outer = 4 > 3; Inner = 1 ≤ 3 → only Outer fires
        // (verified == rubocop 1.87.0: [4/3]).
        let src = indoc! {"
            module Outer
              a = 1
              b = 2
              c = 3
              d = 4
              module Inner
                x = 1
              end
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Module has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn count_as_one_array() {
        // CountAsOne: ['array'] folds the 4-line array to 1 → A(folded)=1 + b=1
        // = 2 ≤ 3, no offense (verified == rubocop 1.87.0).
        let with_fold = ModuleLengthOptions {
            max: 3,
            count_comments: false,
            count_as_one: vec!["array".to_string()],
        };
        test::<ModuleLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            module M
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
            module M
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
            vec!["Module has too many lines. [6/3]".to_string()]
        );
    }

    #[test]
    fn count_as_one_heredoc() {
        // Folds the heredoc to 1 → length 1 ≤ 1, no offense (verified ==
        // rubocop 1.87.0: no Metrics/ModuleLength offense).
        let with_fold = ModuleLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["heredoc".to_string()],
        };
        test::<ModuleLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            module M
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
        // Murphy: [3/2]. This test pins the current (divergent) behavior; flip
        // it to `expect_no_offenses` when murphy-e7bz.70 lands.
        let with_fold = ModuleLengthOptions {
            max: 2,
            count_comments: false,
            count_as_one: vec!["hash".to_string()],
        };
        let src = indoc! {"
            module M
              foo(
                a: 1,
                b: 2
              )
            end
        "};
        assert_eq!(
            messages(&with_fold, src),
            vec!["Module has too many lines. [3/2]".to_string()]
        );
    }

    #[test]
    fn casgn_module_new_form_fires() {
        // `Foo = Module.new do … end` — offense on the constant name `Foo`
        // (verified == rubocop 1.87.0: 3-caret highlight, [4/3]).
        test::<ModuleLength>().with_options(&opts(3)).expect_offense(indoc! {"
            Foo = Module.new do
            ^^^ Module has too many lines. [4/3]
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "});
    }

    #[test]
    fn casgn_module_new_at_max_accepted() {
        test::<ModuleLength>().with_options(&opts(3)).expect_no_offenses(indoc! {"
            Foo = Module.new do
              a = 1
              b = 2
              c = 3
            end
        "});
    }

    #[test]
    fn casgn_class_new_not_matched() {
        // `Class.new` is ClassLength's territory, not ModuleLength's.
        test::<ModuleLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            Foo = Class.new do
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn casgn_scoped_module_new_not_matched() {
        // `module_definition?` requires a nil scope (`casgn nil?`).
        test::<ModuleLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            Bar::Foo = Module.new do
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn cbase_module_new_matched() {
        // `(const cbase :Module)` — `::Module.new` is matched.
        let src = indoc! {"
            Foo = ::Module.new do
              a = 1
              b = 2
              c = 3
              d = 4
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Module has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn plain_class_not_measured() {
        // A `class` node is ClassLength's job; ModuleLength has no on_class.
        test::<ModuleLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            class Foo
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn default_max_100_does_not_fire_on_small_module() {
        // Default Max 100 with a tiny module → no offense.
        test::<ModuleLength>().expect_no_offenses(indoc! {"
            module M
              a = 1
            end
        "});
    }
}
