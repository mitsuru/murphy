//! `Metrics/MethodLength` — flag methods (and `define_method` blocks) whose
//! body exceeds `Max` code lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/MethodLength
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "1.5"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.70, murphy-e7bz.71]
//! notes: >
//!   Mirrors RuboCop's `CodeLength` mixin + `Metrics::Utils::CodeLengthCalculator`,
//!   verified numerically against standalone rubocop 1.87.0
//!   (`--only Metrics/MethodLength`) for: plain code-line counts, `CountComments`,
//!   blank-line exclusion, all four `CountAsOne` types (array/hash/heredoc/
//!   method_call), heredoc-body extension, singleton defs, and `define_method`
//!   blocks. One documented divergence remains (`omit_length`, below).
//!
//!   Measured scopes (RuboCop `on_def`/`on_defs`/`on_block`):
//!   - every `def`/`defs` whose body is non-empty;
//!   - any `define_method` block (`on_block` gates on `method?(:define_method)`
//!     — the method name only; receiver and argument *type* are NOT gated, so
//!     `define_method(dynamic_name) { ... }` is measured) — including numblock
//!     (`{ _1 }`) and itblock (`{ it }`) forms, mirroring RuboCop's
//!     `alias on_numblock on_block`/`alias on_itblock on_block`. The
//!     `AllowedMethods`/`AllowedPatterns` skip applies to a block only when its
//!     first argument is a basic literal (`Sym`/`Str`). Plain
//!     non-`define_method` blocks are NOT measured.
//!   Empty bodies always pass (`extract_body` returns nil → length 0).
//!
//!   Length = code-line count of the body (RuboCop `code_length`): count source
//!   lines spanning the body's first..last line, excluding blank lines and (when
//!   `CountComments` is false) `\A\s*#` comment lines. A trailing heredoc whose
//!   AST range stops at the `<<~LABEL` opener is extended through its terminator
//!   so its body lines are counted (`source_from_node_with_heredoc`).
//!
//!   `CountAsOne` (default `[]`) folds each top-level descendant of a named kind
//!   (`array`/`hash`/`heredoc`/`method_call`) to a single line, via RuboCop's
//!   `each_top_level_descendant` (stop at first matching/classlike node, never
//!   recurse in) + `length - descendant_length + 1`. A folded heredoc counts as
//!   `body_nonblank_lines + 2`.
//!
//!   Fires when length > Max (default 10). Message:
//!   "Method has too many lines. [length/Max]". Offense range is the whole
//!   measured node (RuboCop's non-LSP `node.source_range`).
//!   `AllowedMethods`/`AllowedPatterns` skip by name.
//!
//!   Gap (murphy-e7bz.70), two `CountAsOne` fold edge cases:
//!   1. `omit_length`: RuboCop's `CodeLengthCalculator#omit_length` subtracts
//!      the 1-2 "absent brace" lines when an unbraced trailing-hash kwargs
//!      argument is folded as the sole argument of a parenthesized call. Murphy
//!      does not, so it over-counts by 1-2. Demonstrated (rubocop 1.87.0, Max 1,
//!      `CountAsOne: ['hash']`): `def m; foo(\n a: 1,\n b: 2\n ); end` →
//!      rubocop no offense, Murphy `[3/1]`.
//!   2. Parameter-default folds: RuboCop's `each_top_level_descendant` is seeded
//!      with the def *node*, so a multiline foldable inside a parameter default
//!      (e.g. `def m(x = [\n…\n])` with `CountAsOne: ['array']`) is folded.
//!      Murphy walks the body only, so such defaults are not folded (over-count).
//!
//!   Gap (murphy-e7bz.71): with `CountAsOne: ['heredoc']`, a *nested
//!   interpolated* heredoc (`<<~OUTER` whose body holds `#{<<~INNER}`) is
//!   mispaired by the shared `heredoc_end_line_of_opener` FIFO logic, so the
//!   folded heredoc body extent is wrong (rubocop `[2/0]`, murphy `[4/0]`).
//!   Default config (no `CountAsOne`) is unaffected and matches rubocop.
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 10) — body spans 11 code lines
//! def m
//!   line_1
//!   line_2
//!   # ... lines 3 through 10 ...
//!   line_11
//! end
//! ```

use crate::cops::util::{FoldableType, body_code_length, parse_foldable_types};
use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MethodLength;

/// Options for [`MethodLength`]. Defaults mirror RuboCop's `default.yml`.
#[derive(CopOptions)]
pub struct MethodLengthOptions {
    #[option(
        name = "Max",
        default = 10,
        description = "Maximum allowed method body length in code lines."
    )]
    pub max: i64,
    #[option(
        name = "CountComments",
        default = false,
        description = "Count full-line comments toward the method length."
    )]
    pub count_comments: bool,
    #[option(
        name = "CountAsOne",
        description = "Constructs (array, hash, heredoc, method_call) each counted as one line."
    )]
    pub count_as_one: Vec<String>,
    #[option(
        name = "AllowedMethods",
        description = "Methods to ignore when measuring method length."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        description = "Method-name patterns to ignore when measuring method length."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Metrics/MethodLength",
    description = "Avoid methods longer than 10 lines of code.",
    default_severity = "warning",
    default_enabled = true,
    options = MethodLengthOptions,
)]
impl MethodLength {
    /// RuboCop `on_def`: skip allowed method names, else measure.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(name) = cx.method_name(node) else {
            return;
        };
        if is_allowed(name, cx) {
            return;
        }
        measure(node, cx.def_body(node).get(), cx);
    }

    /// RuboCop `alias on_defs on_def`.
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(name) = cx.method_name(node) else {
            return;
        };
        if is_allowed(name, cx) {
            return;
        }
        measure(node, cx.def_body(node).get(), cx);
    }

    /// RuboCop `on_block`: `define_method` blocks (any argument).
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_numblock on_block`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }

    /// RuboCop `alias on_itblock on_block`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_define_method_block(node, cx);
    }
}

/// RuboCop `on_block`:
///
/// ```ruby
/// return unless node.method?(:define_method)
/// method_name = node.send_node.first_argument
/// return if method_name.basic_literal? && allowed?(method_name.value)
/// check_code_length(node)
/// ```
///
/// The guard is the method name only (`method?(:define_method)`) — receiver and
/// argument *type* are NOT gated, so `define_method(dynamic_name) { ... }` is
/// measured. The `allowed?` skip applies only when the first argument is a basic
/// literal (`Sym`/`Str`); a dynamic name is always measured.
fn check_define_method_block(node: NodeId, cx: &Cx<'_>) {
    let Some(call) = cx.block_call(node).get() else {
        return;
    };
    if cx.method_name(call) != Some("define_method") {
        return;
    }
    // `method_name.basic_literal? && allowed?(method_name.value)` → skip.
    if let Some(name) = basic_literal_name(call, cx)
        && is_allowed(name, cx)
    {
        return;
    }
    measure(node, cx.block_body(node).get(), cx);
}

/// The method name when the `define_method` call's first argument is a basic
/// literal (`Sym`/`Str`) — RuboCop's `method_name.basic_literal?` +
/// `method_name.value`. Returns `None` for a dynamic argument (not a literal),
/// which RuboCop never treats as an allowed-name candidate.
fn basic_literal_name<'a>(call: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let [arg, ..] = cx.call_arguments(call) else {
        return None;
    };
    match cx.kind(*arg) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(*sym)),
        NodeKind::Str(s) => Some(cx.string_str(*s)),
        _ => None,
    }
}

/// RuboCop `allowed?`: AllowedMethods or AllowedPatterns match by name.
fn is_allowed(method_name: &str, cx: &Cx<'_>) -> bool {
    let opts = cx.options_or_default::<MethodLengthOptions>();
    opts.allowed_methods.iter().any(|m| m == method_name)
        || cx.matches_any_pattern(method_name, &opts.allowed_patterns)
}

/// RuboCop `check_code_length`: compute the body code-line count and emit an
/// offense when it exceeds `Max`. Empty bodies pass (`extract_body` → nil →
/// length 0).
fn measure(node: NodeId, body: Option<NodeId>, cx: &Cx<'_>) {
    let Some(body) = body else {
        return;
    };
    let opts = cx.options_or_default::<MethodLengthOptions>();
    let foldable_types: Vec<FoldableType> = parse_foldable_types(&opts.count_as_one);
    let length = body_code_length(body, opts.count_comments, &foldable_types, cx);
    if length <= opts.max {
        return;
    }
    let message = format!("Method has too many lines. [{length}/{}]", opts.max);
    cx.emit_offense(cx.range(node), &message, None);
}

murphy_plugin_api::submit_cop!(MethodLength);


#[cfg(test)]
mod tests {
    use super::{MethodLength, MethodLengthOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn opts(max: i64) -> MethodLengthOptions {
        MethodLengthOptions {
            max,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        }
    }

    /// Run the cop and return the offense messages. The offense range is the
    /// whole (typically multiline) measured node, which the caret-based
    /// `expect_offense` cannot express; these tests assert the message (and so
    /// the `[length/Max]` count) instead. The range==whole-def contract is
    /// pinned separately by `single_line_offense_spans_whole_def`.
    fn messages(opts: &MethodLengthOptions, source: &str) -> Vec<String> {
        run_cop_with_options::<MethodLength>(source, opts)
            .into_iter()
            .map(|o| o.message)
            .collect()
    }

    #[test]
    fn single_line_offense_spans_whole_def() {
        // Pins the message format and the whole-def offense range through the
        // normal harness. Body `a = 1` = 1 code line > 0.
        test::<MethodLength>()
            .with_options(&opts(0))
            .expect_offense(indoc! {"
                def m; a = 1; end
                ^^^^^^^^^^^^^^^^^ Method has too many lines. [1/0]
            "});
    }

    #[test]
    fn flags_method_over_max() {
        // 11 code lines in the body > 10 → [11/10] (verified == rubocop 1.87.0).
        let src = indoc! {"
            def m
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
              g = 7
              h = 8
              i = 9
              j = 10
              k = 11
            end
        "};
        assert_eq!(
            messages(&opts(10), src),
            vec!["Method has too many lines. [11/10]".to_string()]
        );
    }

    #[test]
    fn accepts_method_at_max() {
        // Exactly 10 code lines → not > 10.
        test::<MethodLength>().expect_no_offenses(indoc! {"
            def m
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
              g = 7
              h = 8
              i = 9
              j = 10
            end
        "});
    }

    #[test]
    fn accepts_empty_method() {
        test::<MethodLength>()
            .with_options(&opts(0))
            .expect_no_offenses("def m; end\n");
    }

    #[test]
    fn blank_lines_not_counted() {
        // 2 code lines + blank lines; blanks excluded → [2/1].
        let src = indoc! {"
            def m
              a = 1


              b = 2

            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Method has too many lines. [2/1]".to_string()]
        );
        // And the blank lines mean it does NOT fire at Max 2.
        test::<MethodLength>()
            .with_options(&opts(2))
            .expect_no_offenses(src);
    }

    #[test]
    fn comments_not_counted_by_default() {
        // Interior comment lines excluded → 2 code lines. Fires at Max 1, not 2.
        let src = indoc! {"
            def m
              a = 1
              # an interior comment
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Method has too many lines. [2/1]".to_string()]
        );
        test::<MethodLength>()
            .with_options(&opts(2))
            .expect_no_offenses(src);
    }

    #[test]
    fn comments_counted_when_enabled() {
        // CountComments: true. Body span is first..last statement; the interior
        // comment is inside the span and counted → a=1, #interior, b=2 = 3.
        // (A leading comment above the first statement is outside the body span
        // and never counted — matches rubocop 1.87.0.)
        let cc = MethodLengthOptions {
            max: 2,
            count_comments: true,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        let src = indoc! {"
            def m
              a = 1
              # an interior comment
              b = 2
            end
        "};
        assert_eq!(
            messages(&cc, src),
            vec!["Method has too many lines. [3/2]".to_string()]
        );
    }

    #[test]
    fn leading_comment_not_in_body_span() {
        // A comment above the first statement is outside the body span: with
        // CountComments true the count is still 2 (a=1, b=2), so Max 2 → no fire.
        let cc = MethodLengthOptions {
            max: 2,
            count_comments: true,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&cc).expect_no_offenses(indoc! {"
            def m
              # a leading comment
              a = 1
              b = 2
            end
        "});
    }

    #[test]
    fn singleton_def_measured() {
        let src = indoc! {"
            def self.m
              a = 1
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Method has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn count_as_one_array() {
        // CountAsOne: ['array'] folds the 3-line array to 1 → body = 1 (assign)
        // + 1 (folded array) = 2, not > 2.
        let with_fold = MethodLengthOptions {
            max: 2,
            count_comments: false,
            count_as_one: vec!["array".to_string()],
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            def m
              x = [
                1,
                2
              ]
            end
        "});
    }

    #[test]
    fn count_as_one_array_disabled_fires() {
        // Without CountAsOne the array spans its real lines: x = [ , 1, 2, ]
        // = 4 counted lines > 2.
        let src = indoc! {"
            def m
              x = [
                1,
                2
              ]
            end
        "};
        assert_eq!(
            messages(&opts(2), src),
            vec!["Method has too many lines. [4/2]".to_string()]
        );
    }

    #[test]
    fn count_as_one_hash() {
        let with_fold = MethodLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["hash".to_string()],
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            def m
              x = {
                a: 1,
                b: 2
              }
            end
        "});
    }

    #[test]
    fn count_as_one_heredoc() {
        let with_fold = MethodLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["heredoc".to_string()],
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            def m
              x = <<~TEXT
                line one
                line two
              TEXT
            end
        "});
    }

    #[test]
    fn heredoc_body_counted_without_fold() {
        // No folding: x = <<~TEXT (1) + 2 body lines + 2 delimiters via
        // heredoc_length → code_length 4 for the assignment statement > 3.
        let src = indoc! {"
            def m
              x = <<~TEXT
                line one
                line two
              TEXT
            end
        "};
        assert_eq!(
            messages(&opts(3), src),
            vec!["Method has too many lines. [4/3]".to_string()]
        );
    }

    #[test]
    fn count_as_one_method_call() {
        let with_fold = MethodLengthOptions {
            max: 1,
            count_comments: false,
            count_as_one: vec!["method_call".to_string()],
            allowed_methods: Vec::new(),
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&with_fold).expect_no_offenses(indoc! {"
            def m
              foo(
                1,
                2
              )
            end
        "});
    }

    #[test]
    fn define_method_block_measured() {
        let src = indoc! {"
            define_method(:m) do
              a = 1
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Method has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn define_method_dynamic_name_measured() {
        // RuboCop gates `on_block` on `method?(:define_method)` only — a dynamic
        // (non-literal) name is still measured. Verified == rubocop 1.87.0
        // ([2/1]).
        let src = indoc! {"
            define_method(make_name) do
              a = 1
              b = 2
            end
        "};
        assert_eq!(
            messages(&opts(1), src),
            vec!["Method has too many lines. [2/1]".to_string()]
        );
    }

    #[test]
    fn define_method_literal_name_allowed_skips() {
        // A basic-literal name that is allowed is skipped (RuboCop's
        // `method_name.basic_literal? && allowed?(method_name.value)`).
        let o = MethodLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["m".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&o).expect_no_offenses(indoc! {"
            define_method(:m) do
              a = 1
            end
        "});
    }

    #[test]
    fn plain_block_not_measured() {
        test::<MethodLength>()
            .with_options(&opts(0))
            .expect_no_offenses(indoc! {"
                foo.each do |x|
                  a = 1
                  b = 2
                end
            "});
    }

    #[test]
    fn allowed_methods_skips() {
        let o = MethodLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: vec!["m".to_string()],
            allowed_patterns: Vec::new(),
        };
        test::<MethodLength>().with_options(&o).expect_no_offenses(indoc! {"
            def m
              a = 1
            end
        "});
    }

    #[test]
    fn allowed_patterns_skips() {
        let o = MethodLengthOptions {
            max: 0,
            count_comments: false,
            count_as_one: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_patterns: vec!["\\Am".to_string()],
        };
        test::<MethodLength>().with_options(&o).expect_no_offenses(indoc! {"
            def my_method
              a = 1
            end
        "});
    }
}
