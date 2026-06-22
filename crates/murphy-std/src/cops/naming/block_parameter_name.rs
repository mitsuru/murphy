//! `Naming/BlockParameterName` — flag block parameter names that are too
//! short, end in a number, contain uppercase letters, or are explicitly
//! forbidden.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/BlockParameterName
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the `UncommunicativeName` mixin as driven by `BlockParameterName`'s
//!   single `on_block` hook. Murphy mirrors that scoping with one
//!   `#[on_node(kind = "block")]` method that reads the block's parameter list
//!   via `cx.block_arguments`. `cx.block_arguments` returns the args node only
//!   for `NodeKind::Block`, so `numblock` (`_1`/`_2`) and `itblock` (`it`)
//!   forms are NOT visited — exactly matching RuboCop's `on_block`-only hook
//!   (the cop's own comment notes it skips numblock/itblock). Stabby lambdas
//!   (`->(a) { }`) ARE `Block` nodes in Murphy and ARE checked, matching
//!   rubocop 1.87.0 (verified: `->(a) { a }` flags `a`).
//!
//!   All seven block parameter kinds RuboCop iterates are covered: `arg`,
//!   `optarg`, `restarg`, `kwarg`, `kwoptarg`, `kwrestarg`, `blockarg`. Ruby
//!   blocks accept keyword parameters (`proc { |ab:, cd: 1, **xx| }`), so the
//!   keyword family is checked exactly as `def` parameters are. Offense columns
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
//!   At most ONE offense fires per parameter, in RuboCop's precedence order:
//!   forbidden > uppercase > too-short > ends-in-number. RuboCop's `add_offense`
//!   records each `range` in a per-cop `current_offense_locations` Set and
//!   returns early when the range repeats; the mixin calls all four checks with
//!   the *same* parameter range, so only the first applicable one fires.
//!   Verified vs rubocop 1.87.0: `vX` (uppercase + too short) emits CASE only;
//!   forbidden `ab` (forbidden + too short) emits the forbidden message only.
//!
//!   Skipped, matching the mixin's `next` guards: anonymous parameters whose
//!   name is empty (`*`, `&`), the bare `_`, and allowed names. Destructured
//!   parameters (`proc { |(a, b)| }`) carry no name symbol and are not visited.
//!
//!   `MinNameLength` (1), `AllowNamesEndingInNumbers` (true), `AllowedNames`
//!   (`[]`), and `ForbiddenNames` (`[]`) defaults mirror `config/default.yml`.
//!
//!   Range length is computed in bytes (Murphy ranges are byte offsets), where
//!   RuboCop uses `full_name.size` (a character count) as a byte offset. These
//!   agree for the ASCII parameter names verified above and diverge only for
//!   multibyte names, where Murphy's byte-anchored range is the correct one.
//!
//!   Known gap vs RuboCop (`status: partial`): block-local shadow arguments
//!   (`proc { |a; bb| }`) are NOT checked. Prism/Murphy does not emit a node
//!   for the shadow binding (`proc { |a; bb| nil }` parses to `(args (arg a))`
//!   with no `bb` node), so the shadowarg never reaches this cop. RuboCop's
//!   mixin iterates `node.arguments`, which includes `shadowarg`, and DOES flag
//!   `bb` (verified: `proc { |aa; bb| }` at `MinNameLength: 3` flags both). This
//!   is the same parser-level limitation documented on `Naming/AsciiIdentifiers`
//!   for shadow args; it cannot be closed without an AST/parser change.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct BlockParameterName;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "MinNameLength",
        default = 1,
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
        default = [],
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

const NAME_TYPE: &str = "block parameter";

#[cop(
    name = "Naming/BlockParameterName",
    description = "Checks for block parameter names that contain capital letters, end in numbers, or do not meet a minimal length.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl BlockParameterName {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// `(name_symbol, sigil_extra_bytes)` for the block parameter kinds RuboCop
/// checks. `sigil_extra` is the source-length adjustment applied to `full_name`
/// for the leading sigil: `*` → +1 (restarg), `**` → +2 (kwrestarg), everything
/// else → 0 (RuboCop applies no adjustment to `blockarg`, so its range covers
/// only the `&`). Ruby blocks support keyword parameters (`proc { |ab:, **xx|
/// }`), so `kwarg`/`kwoptarg`/`kwrestarg` are covered, matching rubocop 1.87.0.
/// Returns `None` for non-parameter kinds (e.g. destructuring `mlhs` nodes
/// carry no name symbol).
fn arg_name(id: NodeId, cx: &Cx<'_>) -> Option<(Symbol, u32)> {
    match *cx.kind(id) {
        NodeKind::Arg(name) | NodeKind::Kwarg(name) | NodeKind::Blockarg(name) => Some((name, 0)),
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => Some((name, 0)),
        NodeKind::Restarg(name) => Some((name, 1)),
        NodeKind::Kwrestarg(name) => Some((name, 2)),
        _ => None,
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();

    // RuboCop: `return unless node.arguments?`. `block_arguments` returns the
    // `args` node only for a `Block` (NONE for numblock/itblock).
    let Some(args_node) = cx.block_arguments(node).get() else {
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

        // Anonymous parameters (`*`, `&`) have an empty name; the mixin's
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
/// `length = full_name.size (+1 restarg)`.
fn arg_range(arg: NodeId, full_name: &str, sigil_extra: u32, cx: &Cx<'_>) -> Range {
    let start = cx.range(arg).start;
    let len = full_name.len() as u32 + sigil_extra;
    Range {
        start,
        end: start + len,
    }
}

/// Emit at most ONE offense for `name` on `range`, in RuboCop's precedence
/// order: forbidden > uppercase > too-short > ends-in-number.
///
/// RuboCop's `add_offense` records each `range` into a per-cop
/// `current_offense_locations` Set and returns early if that range is already
/// present. The mixin calls all four checks with the *same* parameter range, so
/// only the first applicable check fires and the rest are dropped. This was
/// verified against rubocop 1.87.0: `vX` (uppercase + too short) emits CASE
/// only; a forbidden `ab` (forbidden + too short) emits the forbidden message
/// only. We reproduce that with an early-return chain.
fn issue_offenses(opts: &Options, name: &str, range: Range, cx: &Cx<'_>) {
    if opts.forbidden_names.iter().any(|f| f == name) {
        let msg = format!("Do not use {name} as a name for a {NAME_TYPE}.");
        cx.emit_offense(range, &msg, None);
        return;
    }

    if name.chars().any(|c| c.is_uppercase()) {
        let msg = format!("Only use lowercase characters for {NAME_TYPE}.");
        cx.emit_offense(range, &msg, None);
        return;
    }

    if (name.chars().count() as i64) < opts.min_name_length {
        let msg = format!(
            "Block parameter must be at least {} characters long.",
            opts.min_name_length
        );
        cx.emit_offense(range, &msg, None);
        return;
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
    use super::{BlockParameterName, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(min: i64, allow_nums: bool, allowed: &[&str], forbidden: &[&str]) -> Options {
        Options {
            min_name_length: min,
            allow_names_ending_in_numbers: allow_nums,
            allowed_names: allowed.iter().map(|s| s.to_string()).collect(),
            forbidden_names: forbidden.iter().map(|s| s.to_string()).collect(),
        }
    }

    // --- length (carets verified vs rubocop 1.87.0) ---

    #[test]
    fn flags_too_short_positional() {
        // rubocop (MinNameLength 3): `a` col 13.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |a| a }
                            ^ Block parameter must be at least 3 characters long.
            "#});
    }

    #[test]
    fn default_min_length_one_allows_single_char() {
        // Default MinNameLength is 1, so a bare `x` is fine. Verified vs rubocop.
        test::<BlockParameterName>().expect_no_offenses(indoc! {r#"
            [1].each { |x| x }
        "#});
    }

    #[test]
    fn flags_each_arg_kind_too_short() {
        // rubocop (MinNameLength 3) across all block parameter kinds:
        //   `x` col 13; `yy` cols 16..17; restarg `*z` cols 24..25 (sigil
        //   included); blockarg `&b` col 28 only (the `&`, no name adjustment).
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |x, yy = 1, *z, &b| nil }
                            ^ Block parameter must be at least 3 characters long.
                               ^^ Block parameter must be at least 3 characters long.
                                       ^^ Block parameter must be at least 3 characters long.
                                           ^ Block parameter must be at least 3 characters long.
            "#});
    }

    #[test]
    fn flags_keyword_block_parameters() {
        // Ruby blocks accept keyword params. rubocop (MinNameLength 3):
        //   `ab` cols 9..10; `cd` cols 14..15; `**xx` cols 21..24 (`**` + name).
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                proc { |ab:, cd: 1, **xx| nil }
                        ^^ Block parameter must be at least 3 characters long.
                             ^^ Block parameter must be at least 3 characters long.
                                    ^^^^ Block parameter must be at least 3 characters long.
            "#});
    }

    // --- stabby lambda IS checked (it is a Block node) ---

    #[test]
    fn flags_stabby_lambda_parameter() {
        // rubocop (MinNameLength 3): `->(a)` flags `a` at col 4.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                ->(a) { a }
                   ^ Block parameter must be at least 3 characters long.
            "#});
    }

    // --- numblock / itblock are NOT checked (on_block-only hook) ---

    #[test]
    fn ignores_numblock() {
        // `_1` is a numbered parameter; `on_block` does not visit numblocks.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { _1 }
            "#});
    }

    #[test]
    fn ignores_itblock() {
        // `it` is an implicit parameter; `on_block` does not visit itblocks.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { it }
            "#});
    }

    // --- underscore handling ---

    #[test]
    fn skips_bare_underscore() {
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { |_| nil }
            "#});
    }

    #[test]
    fn strips_leading_underscore_for_check_but_not_range() {
        // `_x` -> name `x` (len 1 < 3) fires; range covers `_x` (2 cols).
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |_x| nil }
                            ^^ Block parameter must be at least 3 characters long.
            "#});
    }

    #[test]
    fn stripped_name_meeting_length_is_clean() {
        // `_unused` -> name `unused` (len 6) is long enough; no offense.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { |_unused| nil }
            "#});
    }

    // --- uppercase (CASE) — fires under default config (MinNameLength 1) ---

    #[test]
    fn flags_uppercase_letters() {
        // `fooBar` cols 16..21. Default config: CASE fires (len 6 >= 1).
        test::<BlockParameterName>().expect_offense(indoc! {r#"
            [1].each { |fooBar| fooBar }
                        ^^^^^^ Only use lowercase characters for block parameter.
        "#});
    }

    // --- allowed / forbidden names ---

    #[test]
    fn allows_configured_allowed_names() {
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &["id"], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { |id| id }
            "#});
    }

    #[test]
    fn forbidden_name_fires_even_when_long_enough() {
        test::<BlockParameterName>()
            .with_options(&opts(1, true, &[], &["foo"]))
            .expect_offense(indoc! {r#"
                [1].each { |foo| foo }
                            ^^^ Do not use foo as a name for a block parameter.
            "#});
    }

    // --- ends in number ---

    #[test]
    fn allows_trailing_number_by_default() {
        test::<BlockParameterName>().expect_no_offenses(indoc! {r#"
            [1].each { |bar1| bar1 }
        "#});
    }

    #[test]
    fn flags_trailing_number_when_disallowed() {
        test::<BlockParameterName>()
            .with_options(&opts(1, false, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |bar1| bar1 }
                            ^^^^ Do not end block parameter with a number.
            "#});
    }

    // --- precedence: at most one offense per parameter ---

    #[test]
    fn uppercase_wins_over_length() {
        // `vX` -> uppercase + too short (MinNameLength 3). RuboCop dedupes by
        // range: CASE fires, length is suppressed. Verified vs rubocop 1.87.0.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |vX| vX }
                            ^^ Only use lowercase characters for block parameter.
            "#});
    }

    #[test]
    fn forbidden_wins_over_length() {
        // forbidden `ab` (len 2 < 3): forbidden fires, length suppressed.
        // Verified vs rubocop 1.87.0.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &["ab"]))
            .expect_offense(indoc! {r#"
                [1].each { |ab| ab }
                            ^^ Do not use ab as a name for a block parameter.
            "#});
    }

    // --- anonymous / destructured parameters are skipped ---

    #[test]
    fn ignores_anonymous_splat_and_block() {
        test::<BlockParameterName>()
            .with_options(&opts(1, false, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                proc { |*, &| nil }
            "#});
    }

    #[test]
    fn ignores_destructured_parameters() {
        // `(b, c)` carries no name symbol; only `a` is checked.
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |a, (b, c)| nil }
                            ^ Block parameter must be at least 3 characters long.
            "#});
    }

    // --- no params ---

    #[test]
    fn no_offense_for_paramless_block() {
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_no_offenses(indoc! {r#"
                [1].each { nil }
            "#});
    }

    // --- options roundtrip / config wiring ---

    #[test]
    fn respects_custom_min_name_length() {
        test::<BlockParameterName>()
            .with_options(&opts(3, true, &[], &[]))
            .expect_offense(indoc! {r#"
                [1].each { |ab| ab }
                            ^^ Block parameter must be at least 3 characters long.
            "#});
    }
}
murphy_plugin_api::submit_cop!(BlockParameterName);
