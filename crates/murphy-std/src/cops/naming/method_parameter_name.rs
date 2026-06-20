//! `Naming/MethodParameterName` — flag method parameter names that are too
//! short, end in a number, contain uppercase letters, or are explicitly
//! forbidden.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/MethodParameterName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports the `UncommunicativeName` mixin as driven by `MethodParameterName`'s
//!   `on_def`/`on_defs` hooks. Murphy mirrors that scoping with two
//!   `#[on_node(kind = "def"/"defs")]` methods that read the def's *direct*
//!   parameter list via `cx.def_arguments` — block parameters and lambda
//!   parameters nested in default values are NOT visited, exactly as RuboCop's
//!   def-only hook excludes them.
//!
//!   All seven parameter kinds RuboCop iterates are covered: `arg`, `optarg`,
//!   `restarg`, `kwarg`, `kwoptarg`, `kwrestarg`, `blockarg`. Offense columns
//!   for every kind were verified byte-for-byte against rubocop 1.87.0.
//!
//!   Two name values, kept distinct (matching the mixin):
//!     * the *checked* name is the basename with leading underscores stripped
//!       (`_foo` → `foo`); allowed/forbidden/uppercase/min-length/ends-in-number
//!       all test this value;
//!     * the *range length* is `full_name` (leading underscores included), so a
//!       too-short `_x` highlights `_x` (2 cols), not just `x`.
//!
//!   Range start is the parameter's source start, which INCLUDES the sigil. The
//!   span is `full_name` plus `+1` for `restarg` (`*`) and `+2` for `kwrestarg`
//!   (`**`). `blockarg` gets NO sigil adjustment, so `&d` highlights only the
//!   `&` — this is RuboCop's actual (verified) behavior, reproduced for parity.
//!
//!   Multiple offenses can stack on one parameter (forbidden + uppercase +
//!   length + number), in that order — non-exclusive, matching `issue_offenses`.
//!
//!   Skipped, matching the mixin's `next` guards: anonymous parameters whose
//!   name is empty (`*`, `**`, `&`), the bare `_`, and allowed names. Destructured
//!   parameters (`def foo((a, b))`) carry no name symbol and are not visited.
//!
//!   `AllowedNames`/`AllowNamesEndingInNumbers` defaults mirror
//!   `config/default.yml` (the 15-name list and `true`, NOT the doc comment's
//!   stale `false`).
//!
//!   Range length is computed in bytes (Murphy ranges are byte offsets), where
//!   RuboCop uses `full_name.size` (a character count) as a byte offset. These
//!   agree for the ASCII parameter names verified above and diverge only for
//!   multibyte names, where Murphy's byte-anchored range is the correct one.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct MethodParameterName;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "MinNameLength",
        default = 3,
        description = "Parameter names may be equal to or greater than this value."
    )]
    pub min_name_length: i64,

    #[option(
        name = "AllowNamesEndingInNumbers",
        default = true,
        description = "Allow names ending in numbers."
    )]
    pub allow_names_ending_in_numbers: bool,

    #[option(
        name = "AllowedNames",
        default = [
            "as", "at", "by", "cc", "db", "id", "if", "in", "io", "ip", "of",
            "on", "os", "pp", "to"
        ],
        description = "Allowed names that will not register an offense."
    )]
    pub allowed_names: Vec<String>,

    #[option(
        name = "ForbiddenNames",
        default = [],
        description = "Forbidden names that will register an offense."
    )]
    pub forbidden_names: Vec<String>,
}

const NAME_TYPE: &str = "method parameter";

#[cop(
    name = "Naming/MethodParameterName",
    description = "Checks for method parameter names that contain capital letters, end in numbers, or do not meet a minimal length.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MethodParameterName {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// `(name_symbol, sigil_extra_bytes)` for the parameter kinds RuboCop checks.
/// `sigil_extra` is the source-length adjustment applied to `full_name` for the
/// leading sigil: `*` → +1 (restarg), `**` → +2 (kwrestarg), everything else
/// → 0 (RuboCop applies no adjustment to `blockarg`, so its range covers only
/// the `&`). Returns `None` for non-parameter kinds (e.g. destructuring
/// `mlhs`/`unknown` nodes carry no name symbol).
fn arg_name(id: NodeId, cx: &Cx<'_>) -> Option<(Symbol, u32)> {
    match *cx.kind(id) {
        NodeKind::Arg(name)
        | NodeKind::Kwarg(name)
        | NodeKind::Blockarg(name) => Some((name, 0)),
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => Some((name, 0)),
        NodeKind::Restarg(name) => Some((name, 1)),
        NodeKind::Kwrestarg(name) => Some((name, 2)),
        _ => None,
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();

    // RuboCop: `return unless node.arguments?`.
    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = cx.kind(args_node) else {
        return;
    };

    for &arg in cx.list(*list) {
        let Some((name_sym, sigil_extra)) = arg_name(arg, cx) else {
            continue;
        };
        let full_name = cx.symbol_str(name_sym);

        // Anonymous parameters (`*`, `**`, `&`) have an empty name; the mixin's
        // `name_child.nil?` guard skips them. Also skip the bare `_`.
        if full_name.is_empty() || full_name == "_" {
            continue;
        }

        // Strip leading underscores for the *checked* name; the range length
        // still uses `full_name`.
        let name = full_name.trim_start_matches('_');
        if opts.allowed_names.iter().any(|a| a == name) {
            continue;
        }

        let range = arg_range(arg, full_name, sigil_extra, cx);
        issue_offenses(&opts, name, range, cx);
    }
}

/// Range spanning the sigil (if any) plus `full_name`, anchored at the
/// parameter's source start. Mirrors RuboCop's
/// `Range.new(buffer, begin_pos, begin_pos + length)` where
/// `length = full_name.size (+1 restarg / +2 kwrestarg)`.
fn arg_range(arg: NodeId, full_name: &str, sigil_extra: u32, cx: &Cx<'_>) -> Range {
    let start = cx.range(arg).start;
    let len = full_name.len() as u32 + sigil_extra;
    Range {
        start,
        end: start + len,
    }
}

/// Emit every applicable offense for `name` on `range`, in RuboCop's order:
/// forbidden, uppercase, too-short, ends-in-number. Non-exclusive — a single
/// parameter may stack several.
fn issue_offenses(opts: &Options, name: &str, range: Range, cx: &Cx<'_>) {
    if opts.forbidden_names.iter().any(|f| f == name) {
        let msg = format!("Do not use {name} as a name for a {NAME_TYPE}.");
        cx.emit_offense(range, &msg, None);
    }

    if name.chars().any(|c| c.is_uppercase()) {
        let msg = format!("Only use lowercase characters for {NAME_TYPE}.");
        cx.emit_offense(range, &msg, None);
    }

    if (name.chars().count() as i64) < opts.min_name_length {
        let msg = format!(
            "Method parameter must be at least {} characters long.",
            opts.min_name_length
        );
        cx.emit_offense(range, &msg, None);
    }

    if !opts.allow_names_ending_in_numbers && ends_with_num(name) {
        let msg = format!("Do not end {NAME_TYPE} with a number.");
        cx.emit_offense(range, &msg, None);
    }
}

/// RuboCop: `/\d/.match?(name[-1])` — true when the final character is an
/// ASCII digit.
fn ends_with_num(name: &str) -> bool {
    name.chars().next_back().is_some_and(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{MethodParameterName, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(
        min: i64,
        allow_nums: bool,
        allowed: &[&str],
        forbidden: &[&str],
    ) -> Options {
        Options {
            min_name_length: min,
            allow_names_ending_in_numbers: allow_nums,
            allowed_names: allowed.iter().map(|s| s.to_string()).collect(),
            forbidden_names: forbidden.iter().map(|s| s.to_string()).collect(),
        }
    }

    // --- length (default MinNameLength 3) — carets verified vs rubocop 1.87.0 ---

    #[test]
    fn flags_too_short_positional() {
        // rubocop: `ab` col 9..10.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(ab)
                    ^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    #[test]
    fn allows_name_at_min_length() {
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(abc)
            end
        "#});
    }

    #[test]
    fn flags_each_arg_kind_too_short() {
        // Mirrors the empirical rubocop run across all seven parameter kinds.
        // restarg `*b` -> cols 19..20 (sigil included); kwrestarg `**c` ->
        // 33..35; blockarg `&d` -> col 38 only (the `&`, no name adjustment).
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(a, e = 1, *b, g: 2, f:, **c, &d)
                    ^ Method parameter must be at least 3 characters long.
                       ^ Method parameter must be at least 3 characters long.
                              ^^ Method parameter must be at least 3 characters long.
                                  ^ Method parameter must be at least 3 characters long.
                                        ^ Method parameter must be at least 3 characters long.
                                            ^^^ Method parameter must be at least 3 characters long.
                                                 ^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    // --- underscore handling ---

    #[test]
    fn skips_bare_underscore() {
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(_)
            end
        "#});
    }

    #[test]
    fn strips_leading_underscore_for_check_but_not_range() {
        // `_x` -> name `x` (len 1 < 3) fires; range covers `_x` (2 cols).
        // rubocop: col 9..10.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(_x)
                    ^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    #[test]
    fn double_underscore_range_covers_full_name() {
        // `__y` -> name `y`; range covers `__y` (3 cols). rubocop: 9..11.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(__y)
                    ^^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    #[test]
    fn stripped_name_meeting_length_is_clean() {
        // `_abc` -> name `abc` (len 3) is long enough; no offense even though
        // the full name is 4 chars.
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(_abc)
            end
        "#});
    }

    // --- uppercase (CASE) ---

    #[test]
    fn flags_uppercase_letters() {
        // `varOne` col 9..14. rubocop emits CASE only (len 6 ok).
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(varOne)
                    ^^^^^^ Only use lowercase characters for method parameter.
            end
        "#});
    }

    // --- allowed / forbidden names ---

    #[test]
    fn allows_default_allowed_names() {
        // `id` and `io` are in the default AllowedNames list.
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(id, io)
            end
        "#});
    }

    #[test]
    fn forbidden_name_fires_even_when_long_enough() {
        test::<MethodParameterName>()
            .with_options(&opts(3, true, &[], &["data"]))
            .expect_offense(indoc! {r#"
                def foo(data)
                        ^^^^ Do not use data as a name for a method parameter.
                end
            "#});
    }

    // --- ends in number ---

    #[test]
    fn allows_trailing_number_by_default() {
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(bar1)
            end
        "#});
    }

    #[test]
    fn flags_trailing_number_when_disallowed() {
        test::<MethodParameterName>()
            .with_options(&opts(1, false, &[], &[]))
            .expect_offense(indoc! {r#"
                def foo(bar1)
                        ^^^^ Do not end method parameter with a number.
                end
            "#});
    }

    // --- stacking: multiple offenses on one parameter ---

    #[test]
    fn stacks_case_and_length() {
        // `Ab` -> uppercase + too short. Both fire on the same range.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(Ab)
                    ^^ Only use lowercase characters for method parameter.
                    ^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    // --- scope: def vs block / nested lambda defaults ---

    #[test]
    fn flags_singleton_def() {
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def self.foo(ab)
                         ^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    #[test]
    fn ignores_block_parameters() {
        // RuboCop's on_def/on_defs hook does not visit block params.
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            [1].each { |ab| ab }
        "#});
    }

    #[test]
    fn ignores_lambda_params_in_default_value() {
        // A lambda nested in a default value is not a direct def parameter.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(cd = ->(xy) { xy })
                    ^^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    // --- anonymous / destructured parameters are skipped ---

    #[test]
    fn ignores_anonymous_splat_and_block() {
        test::<MethodParameterName>()
            .with_options(&opts(1, false, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                def foo(*, &)
                end
            "#});
    }

    #[test]
    fn ignores_destructured_parameters() {
        // `(b, c)` carries no name symbol; only `a` is checked.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(a, (b, c))
                    ^ Method parameter must be at least 3 characters long.
            end
        "#});
    }

    #[test]
    fn ignores_forward_args() {
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo(...)
            end
        "#});
    }

    // --- no params ---

    #[test]
    fn flags_endless_method_parameter() {
        // Endless methods are still `def` nodes — parameters must be checked.
        test::<MethodParameterName>().expect_offense(indoc! {r#"
            def foo(ab) = ab
                    ^^ Method parameter must be at least 3 characters long.
        "#});
    }

    #[test]
    fn no_offense_for_paramless_def() {
        test::<MethodParameterName>().expect_no_offenses(indoc! {r#"
            def foo
            end
        "#});
    }

    // --- options roundtrip / config wiring ---

    #[test]
    fn respects_custom_min_name_length() {
        test::<MethodParameterName>()
            .with_options(&opts(1, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                def foo(a, b)
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(MethodParameterName);
