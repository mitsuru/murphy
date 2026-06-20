//! `Metrics/ParameterLists` — flag method/block parameter lists that are too
//! long, and methods with too many optional parameters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/ParameterLists
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "1.5"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Two independent offenses, mirroring RuboCop's `on_args` and
//!   `on_def`/`on_defs`:
//!
//!   1. Too many parameters (`on_args`): fires on any `args` node whose
//!      counted parameter count exceeds `Max` (default 5). Counting follows
//!      RuboCop's `args_count`: every child except the explicit block argument
//!      (`&block`) is counted; when `CountKeywordArgs` is `false`, required and
//!      optional keyword arguments (`kwarg`/`kwoptarg`) are also excluded.
//!      The offense range mirrors RuboCop's `args` node `source_range`: it
//!      includes the surrounding `(...)` for a parenthesized `def` or the
//!      `|...|` pipes for a block, and spans first-param-start..last-param-end
//!      for a paren-less `def`. Message:
//!      "Avoid parameter lists longer than 5 parameters. [6/5]".
//!
//!      Skips:
//!      - `initialize` defined inside a `Struct.new`/`Data.define` block
//!        (RuboCop's `struct_new_or_data_define_block?` guard).
//!      - block parameter lists whose block is a lambda or proc
//!        (`->(){}`, `lambda {}`, `proc {}`, `Proc.new {}`) — RuboCop's
//!        `argument_to_lambda_or_proc?` guard.
//!
//!   2. Too many optional parameters (`on_def`/`on_defs`): fires on a `def`
//!      node when the count of positional optional parameters (`optarg` only —
//!      `kwoptarg` is NOT counted) exceeds `MaxOptionalParameters` (default 3).
//!      The offense range is the whole `def`/`defs` node, matching RuboCop's
//!      `add_offense(node)`. Message:
//!      "Method has too many optional parameters. [4/3]".
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 5)
//! def foo(a, b, c, d, e, f); end
//! foo { |a, b, c, d, e, f| }
//!
//! # bad (MaxOptionalParameters: 3)
//! def bar(a = 1, b = 2, c = 3, d = 4); end
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ParameterLists;

/// Options for [`ParameterLists`]. All three keys match RuboCop's defaults.
#[derive(CopOptions)]
pub struct ParameterListsOptions {
    #[option(
        name = "Max",
        default = 5,
        description = "Maximum number of parameters allowed in a method/block parameter list."
    )]
    pub max: i64,
    #[option(
        name = "CountKeywordArgs",
        default = true,
        description = "Count keyword arguments toward the Max threshold."
    )]
    pub count_keyword_args: bool,
    #[option(
        name = "MaxOptionalParameters",
        default = 3,
        description = "Maximum number of optional parameters allowed in a method definition."
    )]
    pub max_optional_parameters: i64,
}

#[cop(
    name = "Metrics/ParameterLists",
    description = "Avoid parameter lists longer than three or four parameters.",
    default_severity = "warning",
    default_enabled = true,
    options = ParameterListsOptions,
)]
impl ParameterLists {
    /// RuboCop `on_args`: too many parameters.
    #[on_node(kind = "args")]
    fn check_args(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ParameterListsOptions>();

        let NodeKind::Args(list) = cx.kind(node) else {
            return;
        };
        let params = cx.list(*list);

        let Some(parent) = cx.parent(node).get() else {
            return;
        };

        // RuboCop: skip `initialize` defined inside `Struct.new`/`Data.define`.
        if is_struct_or_data_initialize(parent, cx) {
            return;
        }

        let count = args_count(params, opts.count_keyword_args, cx);
        if count <= opts.max {
            return;
        }

        // RuboCop: `argument_to_lambda_or_proc?` — the args node's parent block
        // is a lambda or proc; such parameter lists are never flagged.
        if is_lambda_or_proc_block(parent, cx) {
            return;
        }

        let Some(range) = args_offense_range(params, cx) else {
            return;
        };
        let message =
            format!("Avoid parameter lists longer than {} parameters. [{count}/{}]", opts.max, opts.max);
        cx.emit_offense(range, &message, None);
    }

    /// RuboCop `on_def`/`on_defs`: too many optional parameters.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_optional_parameters(node, cx);
    }

    /// RuboCop `alias on_defs on_def`.
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_optional_parameters(node, cx);
    }
}

/// RuboCop `on_def`: count positional optional parameters (`optarg` only) and
/// flag if the count exceeds `MaxOptionalParameters`. Offense on the whole
/// `def`/`defs` node.
fn check_optional_parameters(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ParameterListsOptions>();

    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = cx.kind(args_node) else {
        return;
    };

    let optarg_count = cx
        .list(*list)
        .iter()
        .filter(|&&p| matches!(*cx.kind(p), NodeKind::Optarg { .. }))
        .count() as i64;

    if optarg_count <= opts.max_optional_parameters {
        return;
    }

    let message = format!(
        "Method has too many optional parameters. [{optarg_count}/{}]",
        opts.max_optional_parameters
    );
    cx.emit_offense(cx.range(node), &message, None);
}

/// RuboCop `args_count`: count every parameter except the explicit block
/// argument (`&block`). When `count_keyword_args` is `false`, required and
/// optional keyword arguments (`kwarg`/`kwoptarg`) are also excluded.
fn args_count(params: &[NodeId], count_keyword_args: bool, cx: &Cx<'_>) -> i64 {
    params
        .iter()
        .filter(|&&p| {
            match *cx.kind(p) {
                NodeKind::Blockarg(_) => false,
                NodeKind::Kwarg(_) | NodeKind::Kwoptarg { .. } => count_keyword_args,
                _ => true,
            }
        })
        .count() as i64
}

/// Reconstruct RuboCop's `args` node `source_range`: span the parameters and
/// expand to include the surrounding `(...)` (parenthesized `def`) or `|...|`
/// (block) delimiters when present. For a paren-less `def`, the range is
/// first-param-start..last-param-end. Returns `None` for an empty list (no
/// offense can be raised on an empty parameter list anyway).
fn args_offense_range(params: &[NodeId], cx: &Cx<'_>) -> Option<Range> {
    let first = *params.first()?;
    let last = *params.last()?;
    let mut start = cx.range(first).start;
    let mut end = cx.range(last).end;

    // Extend backward over an opening `(` or `|` immediately before the first
    // parameter.
    if let Some(open) = cx.token_before(start) {
        let is_open = open.kind == SourceTokenKind::LeftParen
            || (open.kind == SourceTokenKind::Other && cx.raw_source(open.range) == "|");
        if is_open {
            start = open.range.start;
        }
    }

    // Extend forward over a closing `)` or `|` immediately after the last
    // parameter.
    if let Some(close) = cx.token_after(end) {
        let is_close = close.kind == SourceTokenKind::RightParen
            || (close.kind == SourceTokenKind::Other && cx.raw_source(close.range) == "|");
        if is_close {
            end = close.range.end;
        }
    }

    Some(Range { start, end })
}

/// RuboCop `argument_to_lambda_or_proc?` (`^lambda_or_proc?`): the args node's
/// parent block is a lambda (`->(){}` / `lambda {}`) or proc (`proc {}` /
/// `Proc.new {}`).
fn is_lambda_or_proc_block(parent: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_lambda(parent) {
        return true;
    }
    let Some(call) = cx.block_call(parent).get() else {
        return false;
    };
    // `proc { }` — receiverless `proc` send.
    if cx.call_receiver(call).get().is_none() && cx.method_name(call) == Some("proc") {
        return true;
    }
    // `Proc.new { }` — `Proc.new`, where `Proc` is the global constant.
    if cx.method_name(call) == Some("new")
        && let Some(recv) = cx.call_receiver(call).get()
        && cx.is_global_const(recv, "Proc")
    {
        return true;
    }
    false
}

/// RuboCop `struct_new_or_data_define_block?(parent.parent)` combined with
/// `parent.method?(:initialize)`: the args node's parent is an `initialize`
/// `def` whose enclosing block is a `Struct.new`/`Data.define` block.
fn is_struct_or_data_initialize(parent: NodeId, cx: &Cx<'_>) -> bool {
    // `parent` must be a def named `initialize`.
    if !matches!(*cx.kind(parent), NodeKind::Def { .. } | NodeKind::Defs { .. }) {
        return false;
    }
    if cx.method_name(parent) != Some("initialize") {
        return false;
    }
    // Grandparent must be a `Struct.new`/`Data.define` block.
    let Some(grandparent) = cx.parent(parent).get() else {
        return false;
    };
    is_struct_new_or_data_define_block(grandparent, cx)
}

/// RuboCop `struct_new_or_data_define_block?`: a block whose call is
/// `Struct.new(...)` or `Data.define(...)` (where `Struct`/`Data` are global
/// constants).
fn is_struct_new_or_data_define_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(call) = cx.block_call(node).get() else {
        return false;
    };
    let Some(recv) = cx.call_receiver(call).get() else {
        return false;
    };
    match cx.method_name(call) {
        Some("new") => cx.is_global_const(recv, "Struct"),
        Some("define") => cx.is_global_const(recv, "Data"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{ParameterLists, ParameterListsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(max: i64, count_keyword_args: bool, max_optional: i64) -> ParameterListsOptions {
        ParameterListsOptions {
            max,
            count_keyword_args,
            max_optional_parameters: max_optional,
        }
    }

    #[test]
    fn flags_def_with_too_many_params() {
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo(a, b, c, d, e, f)
                   ^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
            end
        "});
    }

    #[test]
    fn flags_paren_less_def() {
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo a, b, c, d, e, f
                    ^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
            end
        "});
    }

    #[test]
    fn flags_block_with_too_many_params() {
        test::<ParameterLists>().expect_offense(indoc! {"
            foo { |a, b, c, d, e, f| }
                  ^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
        "});
    }

    #[test]
    fn flags_singleton_def() {
        test::<ParameterLists>().expect_offense(indoc! {"
            def self.foo(a, b, c, d, e, f)
                        ^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
            end
        "});
    }

    #[test]
    fn flags_endless_def() {
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo(a, b, c, d, e, f) = nil
                   ^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
        "});
    }

    #[test]
    fn accepts_exactly_max_params() {
        test::<ParameterLists>().expect_no_offenses("def foo(a, b, c, d, e); end\n");
    }

    #[test]
    fn accepts_def_with_no_params() {
        test::<ParameterLists>().expect_no_offenses("def foo; end\n");
    }

    #[test]
    fn block_arg_not_counted() {
        // 5 positional + `&block` → 5 counted params, not 6.
        test::<ParameterLists>().expect_no_offenses("def foo(a, b, c, d, e, &block); end\n");
    }

    #[test]
    fn flags_too_many_optional_params() {
        // Single-line def so the whole-def offense range fits one annotation
        // line; RuboCop highlights the entire `def ... end` (here cols 1-40).
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo(a = 1, b = 2, c = 3, d = 4); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Method has too many optional parameters. [4/3]
        "});
    }

    #[test]
    fn accepts_max_optional_params() {
        test::<ParameterLists>().expect_no_offenses("def foo(a = 1, b = 2, c = 3); end\n");
    }

    #[test]
    fn kwoptarg_not_counted_as_optional() {
        // 3 optargs + 3 kwoptargs: optional-params offense does NOT fire
        // (only 3 optargs ≤ 3). But 6 total params > 5 → too-many-params fires.
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo(a = 1, b = 2, c = 3, d: 4, e: 5, f: 6)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
            end
        "});
    }

    #[test]
    fn keyword_args_counted_by_default() {
        // 3 positional + 3 keyword = 6 > 5.
        test::<ParameterLists>().expect_offense(indoc! {"
            def foo(a, b, c, d: 1, e: 2, f: 3)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
            end
        "});
    }

    #[test]
    fn keyword_args_excluded_when_configured() {
        // With CountKeywordArgs: false, the 3 kwargs are not counted: 3 ≤ 5.
        test::<ParameterLists>()
            .with_options(&opts(5, false, 3))
            .expect_no_offenses("def foo(a, b, c, d: 1, e: 2, f: 3); end\n");
    }

    #[test]
    fn skips_lambda_literal_params() {
        test::<ParameterLists>().expect_no_offenses("->(a, b, c, d, e, f) { }\n");
    }

    #[test]
    fn skips_lambda_method_params() {
        test::<ParameterLists>().expect_no_offenses("lambda { |a, b, c, d, e, f| }\n");
    }

    #[test]
    fn skips_proc_params() {
        test::<ParameterLists>().expect_no_offenses("proc { |a, b, c, d, e, f| }\n");
    }

    #[test]
    fn skips_proc_new_params() {
        test::<ParameterLists>().expect_no_offenses("Proc.new { |a, b, c, d, e, f| }\n");
    }

    #[test]
    fn skips_struct_new_initialize() {
        test::<ParameterLists>().expect_no_offenses(indoc! {"
            Struct.new(:one, :two, :three, :four, :five) do
              def initialize(one:, two:, three:, four:, five:, six:)
              end
            end
        "});
    }

    #[test]
    fn skips_data_define_initialize() {
        test::<ParameterLists>().expect_no_offenses(indoc! {"
            Data.define(:one, :two) do
              def initialize(one:, two:, three:, four:, five:, six:)
              end
            end
        "});
    }

    #[test]
    fn flags_non_initialize_in_struct_block() {
        // Only `initialize` is exempt inside Struct.new; other methods are not.
        test::<ParameterLists>().expect_offense(indoc! {"
            Struct.new(:one) do
              def other(one:, two:, three:, four:, five:, six:)
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid parameter lists longer than 5 parameters. [6/5]
              end
            end
        "});
    }

    #[test]
    fn custom_max_is_honored() {
        test::<ParameterLists>()
            .with_options(&opts(2, true, 3))
            .expect_offense(indoc! {"
                def foo(a, b, c)
                       ^^^^^^^^^ Avoid parameter lists longer than 2 parameters. [3/2]
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(ParameterLists);
