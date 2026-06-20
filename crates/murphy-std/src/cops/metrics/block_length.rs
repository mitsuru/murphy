//! `Metrics/BlockLength` — flag `{}`/`do…end` blocks whose body exceeds `Max`
//! code lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/BlockLength
//! upstream_version_checked: 1.87.0
//! version_added: "0.44"
//! version_changed: "1.5"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.70]
//! notes: >
//!   Mirrors RuboCop's `CodeLength` mixin + `Metrics::Utils::CodeLengthCalculator`
//!   driven from `Metrics/BlockLength#on_block`, verified numerically against
//!   standalone rubocop 1.87.0 (`--only Metrics/BlockLength`) for plain
//!   code-line counts, `CountComments`, blank-line exclusion, the `refine`
//!   default skip, `AllowedMethods`/`AllowedPatterns` skips, the receiver.method
//!   `AllowedMethods` form, the `class_constructor?` skip, and the
//!   array/hash/heredoc/method_call `CountAsOne` folds.
//!
//!   Measured scopes (RuboCop `on_block`/`on_numblock`/`on_itblock`): every
//!   block (`{ }` / `do…end`, numblock `{ _1 }`, itblock `{ it }`) whose body is
//!   non-empty. Empty bodies always pass (`extract_body` → nil → length 0).
//!
//!   Skips (RuboCop `on_block`, in order):
//!   - `allowed_method?(node.method_name)` — the block call's method name is in
//!     `AllowedMethods` (default `[refine]`).
//!   - `matches_allowed_pattern?(node.method_name)` — the method name matches
//!     an `AllowedPatterns` regexp.
//!   - `method_receiver_excluded?(node)` — an `AllowedMethods` entry in
//!     `"receiver.method"` form matches the block's receiver-source (whitespace
//!     stripped) and method name. A dotless entry reduces to the plain
//!     `allowed_method?` case.
//!   - `node.class_constructor?` — `Class.new`/`Module.new`/`Struct.new` /
//!     `Data.define` blocks (RuboCop "does not apply for `Struct` definitions").
//!
//!   Length = code-line count of the block body (RuboCop `code_length` via
//!   `extract_body`): count source lines spanning the body's first..last line,
//!   excluding blank lines and (when `CountComments` is false) `\A\s*#` comment
//!   lines, with a trailing heredoc extended through its terminator.
//!
//!   `CountAsOne` (default `[]`) folds each top-level descendant of a named kind
//!   (`array`/`hash`/`heredoc`/`method_call`) to a single line.
//!
//!   Fires when length > Max (default 25). Message:
//!   "Block has too many lines. [length/Max]". Offense range is the whole block
//!   node (RuboCop's non-LSP `node.source_range`).
//!
//!   No lambda/proc skip: unlike `Metrics/ParameterLists`, BlockLength has no
//!   `argument_to_lambda_or_proc?` guard, so multiline `lambda { … }` and
//!   `-> { … }` blocks ARE measured (matches rubocop 1.87.0).
//!
//!   Gap (murphy-e7bz.70), shared `CountAsOne` fold edge cases inherited from
//!   the `body_code_length` calculator:
//!   1. `omit_length`: RuboCop subtracts the 1-2 "absent brace" lines when an
//!      unbraced trailing-hash kwargs argument is folded as the sole argument of
//!      a parenthesized call. Murphy does not, so it over-counts by 1-2.
//!   2. Node-seeded vs body-seeded fold: RuboCop seeds `each_top_level_descendant`
//!      with the block *node*, so a foldable in the block's call/args (siblings
//!      of the body, e.g. a multiline arg of the block's own method call) can be
//!      folded. Murphy walks the body only, so such constructs are not folded
//!      (over-count). Both are common-case-safe: ordinary block bodies match.
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 25) — body spans 26 code lines
//! foo do
//!   line_1
//!   # ... lines 2 through 25 ...
//!   line_26
//! end
//! ```

use crate::cops::util::{FoldableType, body_code_length, parse_foldable_types};
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct BlockLength;

/// Options for [`BlockLength`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct BlockLengthOptions {
    #[option(
        name = "Max",
        default = 25,
        description = "Maximum allowed block body length in code lines."
    )]
    pub max: i64,
    #[option(
        name = "CountComments",
        default = false,
        description = "Count full-line comments toward the block length."
    )]
    pub count_comments: bool,
    #[option(
        name = "CountAsOne",
        default = [],
        description = "Constructs (array, hash, heredoc, method_call) each counted as one line."
    )]
    pub count_as_one: Vec<String>,
    #[option(
        name = "AllowedMethods",
        default = ["refine"],
        description = "Methods whose blocks are ignored when measuring block length."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Method-name patterns whose blocks are ignored when measuring block length."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Metrics/BlockLength",
    description = "Avoid long blocks with many lines.",
    default_severity = "warning",
    default_enabled = true,
    options = BlockLengthOptions,
)]
impl BlockLength {
    /// RuboCop `on_block`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// RuboCop `alias on_numblock on_block`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// RuboCop `alias on_itblock on_block`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// RuboCop `on_block`: apply the four skips, then measure the block body.
fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<BlockLengthOptions>();
    let method_name = cx.method_name(node);

    // `allowed_method?(node.method_name) || matches_allowed_pattern?(...)`.
    if let Some(name) = method_name {
        if opts.allowed_methods.iter().any(|m| m == name) {
            return;
        }
        if cx.matches_any_pattern(name, &opts.allowed_patterns) {
            return;
        }
    }

    // `method_receiver_excluded?(node)`.
    if method_receiver_excluded(node, method_name, &opts.allowed_methods, cx) {
        return;
    }

    // `node.class_constructor?` — Struct/Class/Module.new, Data.define blocks.
    if cx.is_class_constructor(node) {
        return;
    }

    let Some(body) = cx.block_body(node).get() else {
        return;
    };
    let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
    let length = body_code_length(body, opts.count_comments, &foldable_types, cx);
    if length <= opts.max {
        return;
    }
    let message = format!("Block has too many lines. [{length}/{}]", opts.max);
    cx.emit_offense(cx.range(node), &message, None);
}

/// RuboCop `method_receiver_excluded?`:
///
/// ```ruby
/// node_receiver = node.receiver&.source&.gsub(/\s+/, '')
/// node_method   = String(node.method_name)
/// allowed_methods.any? do |config|
///   next unless config.is_a?(String)
///   receiver, method = config.split('.')
///   unless method
///     method   = receiver
///     receiver = node_receiver
///   end
///   method == node_method && receiver == node_receiver
/// end
/// ```
///
/// `config.split('.')` is take-first-two (destructuring assignment), so
/// `"a.b.c"` yields `receiver = "a"`, `method = "b"` (`".c"` dropped). A dotless
/// entry sets `receiver = node_receiver`, making the receiver check always true —
/// reducing to the plain `allowed_method?` case (already handled in `check`, but
/// mirrored here for fidelity to RuboCop's combined predicate).
fn method_receiver_excluded(
    node: NodeId,
    method_name: Option<&str>,
    allowed_methods: &[String],
    cx: &Cx<'_>,
) -> bool {
    let Some(node_method) = method_name else {
        return false;
    };
    // `node.receiver&.source&.gsub(/\s+/, '')` — the block call's receiver
    // source with all ASCII/Unicode whitespace removed, or `None` when absent.
    let call = cx.block_call(node).get();
    let node_receiver: Option<String> = call
        .and_then(|c| cx.call_receiver(c).get())
        .map(|recv| {
            cx.raw_source(cx.range(recv))
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect()
        });

    allowed_methods.iter().any(|config| {
        let mut parts = config.split('.');
        let first = parts.next().unwrap_or("");
        match parts.next() {
            Some(method) => {
                // `receiver.method` form: receiver is the entry's first segment.
                method == node_method && node_receiver.as_deref() == Some(first)
            }
            None => {
                // Dotless: `method = receiver` (the whole entry), receiver is the
                // node's own receiver, so the receiver comparison is always true.
                first == node_method
            }
        }
    })
}

murphy_plugin_api::submit_cop!(BlockLength);

#[cfg(test)]
mod tests {
    use super::{BlockLength, BlockLengthOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    /// Options with the named `Max`, otherwise RuboCop defaults
    /// (`AllowedMethods: [refine]`).
    fn opts(max: i64) -> BlockLengthOptions {
        BlockLengthOptions {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        }
    }

    /// Run the cop and collect offense messages. The offense range is the whole
    /// (multiline) block node, which `expect_offense` carets cannot express on
    /// multiline input; these tests assert the message (and so the `[length/Max]`
    /// count). The whole-block range is pinned by `single_line_block_offense`.
    fn messages(opts: &BlockLengthOptions, source: &str) -> Vec<String> {
        run_cop_with_options::<BlockLength>(source, opts)
            .into_iter()
            .map(|o| o.message)
            .collect()
    }

    #[test]
    fn default_options_match_rubocop() {
        // Struct Default must equal default.yml so unit tests and the CLI agree.
        let d = BlockLengthOptions::default();
        assert_eq!(d.max, 25);
        assert!(!d.count_comments);
        assert!(d.count_as_one.is_empty());
        assert_eq!(d.allowed_methods, vec!["refine".to_string()]);
        assert!(d.allowed_patterns.is_empty());
    }

    #[test]
    fn single_line_block_offense() {
        // Pins the message format and the whole-block offense range through the
        // normal harness. Body `x = 1` = 1 code line > 0.
        test::<BlockLength>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                foo { x = 1 }
                ^^^^^^^^^^^^^ Block has too many lines. [1/0]
            "});
    }

    #[test]
    fn flags_block_over_max() {
        // Body spans 3 code lines > 2 → [3/2] (verified == rubocop 1.87.0).
        let src = indoc! {"
            foo do
              a = 1
              b = 2
              c = 3
            end
        "};
        assert_eq!(
            messages(&opts(2), src),
            vec!["Block has too many lines. [3/2]".to_string()]
        );
    }

    #[test]
    fn accepts_block_at_max() {
        // Exactly 2 code lines → not > 2.
        test::<BlockLength>().with_options(&opts(2)).expect_no_offenses(indoc! {"
            foo do
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn accepts_empty_block() {
        test::<BlockLength>()
            .with_options(&opts(0))
            .expect_no_offenses("foo { }\n");
    }

    #[test]
    fn blank_lines_not_counted() {
        // 2 code lines + blanks; blanks excluded → [2/1].
        let src = indoc! {"
            foo do
              a = 1


              b = 2

            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
        test::<BlockLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_not_counted_by_default() {
        // Interior comment excluded → 2 code lines. Fires at Max 1, not 2.
        let src = indoc! {"
            foo do
              a = 1
              # an interior comment
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
        test::<BlockLength>().with_options(&opts(2)).expect_no_offenses(src);
    }

    #[test]
    fn comments_counted_when_enabled() {
        // CountComments: true → a=1, #interior, b=2 = 3 (verified == rubocop).
        let cc = BlockLengthOptions {
            max: 2,
            count_comments: true,
            count_as_one: Vec::new(),
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        };
        let src = indoc! {"
            foo do
              a = 1
              # an interior comment
              b = 2
            end
        "};
        assert_eq!(
            messages(&cc, src),
            vec!["Block has too many lines. [3/2]".to_string()]
        );
    }

    #[test]
    fn brace_block_measured() {
        let src = indoc! {"
            foo {
              a = 1
              b = 2
            }
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn block_with_receiver_measured() {
        let src = indoc! {"
            obj.each do |x|
              a = 1
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn numblock_measured() {
        let src = indoc! {"
            foo do
              a = _1
              b = _1
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn itblock_measured() {
        // `alias on_itblock on_block` — `it`-param blocks are measured
        // (verified == rubocop 1.87.0: [2/1]).
        let src = indoc! {"
            foo do
              a = it
              b = it
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn lambda_block_measured() {
        // No lambda/proc skip in BlockLength — multiline lambdas ARE measured
        // (verified == rubocop 1.87.0: [2/1]).
        let src = indoc! {"
            x = -> do
              a = 1
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Block has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn refine_block_skipped_by_default() {
        // `refine` is the default AllowedMethods entry.
        test::<BlockLength>().with_options(&opts(0)).expect_no_offenses(indoc! {"
            refine String do
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn allowed_methods_skips() {
        let o = BlockLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["foo".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&o).expect_no_offenses(indoc! {"
            foo do
              a = 1
            end
        "});
    }

    #[test]
    fn allowed_patterns_skips() {
        let o = BlockLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: vec!["\\Aassert".to_string()],
        };
        test::<BlockLength>().with_options(&o).expect_no_offenses(indoc! {"
            assert_something do
              a = 1
            end
        "});
    }

    #[test]
    fn receiver_qualified_allowed_method_skips() {
        // `AllowedMethods: ['Foo.bar']` skips `Foo.bar { }` (method_receiver_excluded?).
        let o = BlockLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["Foo.bar".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&o).expect_no_offenses(indoc! {"
            Foo.bar do
              a = 1
            end
        "});
    }

    #[test]
    fn receiver_qualified_allowed_method_wrong_receiver_fires() {
        // `AllowedMethods: ['Foo.bar']` does NOT skip `Baz.bar { }`.
        let o = BlockLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["Foo.bar".to_string()],
            allowed_patterns: Vec::new(),
        };
        let src = indoc! {"
            Baz.bar do
              a = 1
            end
        "};
        assert_eq!(
            messages(&o, src),
            vec!["Block has too many lines. [1/0]".to_string()]
        );
    }

    #[test]
    fn class_constructor_block_skipped() {
        // RuboCop "does not apply for Struct definitions" — class_constructor?.
        for ctor in ["Struct.new", "Class.new", "Module.new", "Data.define"] {
            let src = format!("{ctor} do\n  a = 1\n  b = 2\nend\n");
            test::<BlockLength>()
                .with_options(&opts(0))
                .expect_no_offenses(&src);
        }
    }

    #[test]
    fn count_as_one_array() {
        // CountAsOne: ['array'] folds the 3-line array to 1 → body = 1 (assign)
        // + 1 (folded array) = 2, not > 2 (verified == rubocop 1.87.0).
        let with_fold = BlockLengthOptions {
            max: 2,
            count_comments: false,
            count_as_one: vec!["array".to_string()],
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            foo do
              x = [
                1,
                2
              ]
            end
        "});
    }

    #[test]
    fn count_as_one_array_disabled_fires() {
        // Without CountAsOne the array spans real lines: x = [ , 1, 2, ] = 4 > 2.
        let src = indoc! {"
            foo do
              x = [
                1,
                2
              ]
            end
        "};
        assert_eq!(
            messages(&opts(2), src),
            vec!["Block has too many lines. [4/2]".to_string()]
        );
    }

    #[test]
    fn count_as_one_hash() {
        let with_fold = BlockLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["hash".to_string()],
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            foo do
              x = {
                a: 1,
                b: 2
              }
            end
        "});
    }

    #[test]
    fn count_as_one_heredoc() {
        let with_fold = BlockLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["heredoc".to_string()],
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            foo do
              x = <<~TEXT
                line one
                line two
              TEXT
            end
        "});
    }

    #[test]
    fn count_as_one_method_call() {
        let with_fold = BlockLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["method_call".to_string()],
            allowed_methods: vec!["refine".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<BlockLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            foo do
              bar(
                1,
                2
              )
            end
        "});
    }
}
