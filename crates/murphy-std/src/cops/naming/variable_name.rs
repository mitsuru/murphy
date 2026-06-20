//! `Naming/VariableName` — enforce the configured style (`snake_case` /
//! `camelCase`) for variable names: local variables, instance/class variables,
//! method arguments (positional, optional, rest, keyword, keyword-splat) and
//! block arguments. Global variables are checked for forbidden names only.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/VariableName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `on_lvasgn` (aliased to `on_ivasgn`,
//!   `on_cvasgn`, `on_arg`, `on_optarg`, `on_restarg`, `on_kwoptarg`,
//!   `on_kwarg`, `on_kwrestarg`, `on_blockarg`, `on_lvar`) plus the
//!   forbidden-only `on_gvasgn`:
//!
//!     return if allowed_identifier?(name)          # AllowedIdentifiers
//!     if forbidden_name?(name)                     # Forbidden{Ident,Pattern}s
//!       add_offense(loc.name, MSG_FORBIDDEN)
//!     else
//!       valid_name? = style_regex(name) || matches AllowedPatterns(name)
//!       add_offense(loc.name, MSG) unless valid_name?
//!
//!   Control-flow precedence is exact: AllowedIdentifiers (full skip) >
//!   Forbidden{Identifiers,Patterns} > (style-regex OR AllowedPatterns).
//!
//!   Node coverage (verified against rubocop 1.87.0):
//!     * local-variable assignments (`lvasgn`) AND reads (`lvar`) — both fire,
//!       so `x = 1; x` is two offenses (RuboCop aliases `on_lvar`);
//!     * instance/class variable ASSIGNMENTS only (`ivasgn`/`cvasgn`) — reads
//!       (`ivar`/`cvar`) are NOT flagged (no `on_ivar`/`on_cvar` alias);
//!     * the full argument family: `arg`, `optarg`, `restarg`, `kwarg`,
//!       `kwoptarg`, `kwrestarg`, `blockarg`;
//!     * global-variable assignments (`gvasgn`) — forbidden names ONLY, never
//!       style-checked (the style regexes permit `@{0,2}` but never `$`).
//!   Method definitions and method calls are deliberately untouched.
//!
//!   Anonymous splat/double-splat/block parameters (`def m(*, **, &)`) are
//!   skipped: RuboCop's `return unless (name = node.name)` short-circuits on
//!   the nil name, and Murphy interns them as empty strings, so an
//!   `is_empty()` guard reproduces it (verified: `def m(badArg, *, **, &)`
//!   flags only `badArg`).
//!
//!   Name source / sigils:
//!     * ivasgn/cvasgn names carry their sigil (`@foo`/`@@foo`), matching
//!       RuboCop's `node.name` (`:@foo`). The style regexes' `@{0,2}` prefix
//!       handles this, so a non-snake `@fooBar` IS flagged.
//!     * Allowed/Forbidden Identifiers do `name.delete("@$")` before an exact
//!       compare, so `ForbiddenIdentifiers: ['fooBar']` matches `@fooBar`,
//!       `@@fooBar`, and `$fooBar` (verified). Allowed/Forbidden Patterns match
//!       the FULL name with sigil via `Regexp.new(p).match?(name)` (unanchored),
//!       mirrored by `cx.matches_any_pattern`.
//!
//!   Offense range mirrors `node.loc.name`:
//!     * assignment family — Murphy leaves `loc.name == ZERO`, so the name
//!       (incl. sigil) is located by source search from the node start; the
//!       caret spans the sigil (verified `@fooBar` col 1..7, `@@bazQux`
//!       col 1..8);
//!     * argument family — `loc.name` is populated and already excludes the
//!       sigil/`*`/`**`/`&` and the trailing `:` of keyword labels (verified
//!       `*restArg` → bare `restArg`, `kwArg:` → bare `kwArg`).
//!
//!   Style regexes are byte-level ports of RuboCop's FORMATS hash:
//!     snake_case: /^@{0,2}[\d[[:lower:]]_]+[!?=]?$/
//!     camelCase:  /^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/
//!   Verified column-for-column against rubocop 1.87.0 for both styles,
//!   including `foo1`/`foo_1`/`_unused`/`__` (snake ok) and
//!   `_foo`/`_`/`fooBar`/`fooBar2` (camel ok).
//!
//!   Known minor gap (documented; status stays `verified` for real-world ASCII
//!   code): `[[:lower:]]`/`[[:upper:]]` are Unicode-aware in Ruby but the byte
//!   checks here (and the `regex` crate's POSIX classes) are ASCII-only. A
//!   variable whose name contains non-ASCII cased letters could diverge — the
//!   same documented limitation `Naming/ConstantName` carries. Non-ASCII
//!   variable names are vanishingly rare; `Naming/AsciiIdentifiers` already
//!   flags them.
//! ```
//!
//! ## Offense range
//!
//! `node.loc.name`: the bare variable name. For the assignment family the caret
//! spans any `@`/`@@` sigil; for the argument family it excludes the
//! `*`/`**`/`&` sigil and a keyword label's trailing `:`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_FORBIDDEN: &str = "is forbidden, use another name instead.";

#[derive(Default)]
pub struct VariableName;

/// Enforced naming style for variables.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VariableNameStyle {
    /// `snake_case` — RuboCop default.
    #[default]
    #[option(value = "snake_case")]
    SnakeCase,
    /// `camelCase`.
    #[option(value = "camelCase")]
    CamelCase,
}

impl VariableNameStyle {
    fn as_str(self) -> &'static str {
        match self {
            VariableNameStyle::SnakeCase => "snake_case",
            VariableNameStyle::CamelCase => "camelCase",
        }
    }

    /// RuboCop's `FORMATS.fetch(style).match?(name)`.
    fn matches(self, name: &str) -> bool {
        match self {
            VariableNameStyle::SnakeCase => is_snake_case(name),
            VariableNameStyle::CamelCase => is_camel_case(name),
        }
    }
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "snake_case",
        description = "Required variable-name style: `snake_case` (default) or `camelCase`."
    )]
    pub enforced_style: VariableNameStyle,
    #[option(
        name = "AllowedIdentifiers",
        default = [],
        description = "Exact variable names that are always allowed (sigils ignored)."
    )]
    pub allowed_identifiers: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a variable whose name matches any is allowed."
    )]
    pub allowed_patterns: Vec<String>,
    #[option(
        name = "ForbiddenIdentifiers",
        default = [],
        description = "Exact variable names that are forbidden (sigils ignored)."
    )]
    pub forbidden_identifiers: Vec<String>,
    #[option(
        name = "ForbiddenPatterns",
        default = [],
        description = "Regexes; a variable whose name matches any is forbidden."
    )]
    pub forbidden_patterns: Vec<String>,
}

#[cop(
    name = "Naming/VariableName",
    description = "Use the configured style when naming variables.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl VariableName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root; chain it so a lone top-level
        // statement (e.g. `fooBar = 1`, whose root *is* the lvasgn) is seen.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            let Some((name, range, is_global)) = variable_target(id, cx) else {
                continue;
            };

            // RuboCop's `return unless (name = node.name)`. Anonymous splat /
            // double-splat / block parameters (`def m(*, **, &)`) intern an
            // empty name in Murphy and a nil `node.name` in RuboCop, which
            // short-circuits before any offense. Skip them.
            if name.is_empty() {
                continue;
            }

            // Global vars: forbidden names only, never style-checked.
            if is_global {
                if forbidden_name(name, &opts, cx) {
                    emit_forbidden(name, range, cx);
                }
                continue;
            }

            // `return if allowed_identifier?(name)` — full skip.
            if allowed_identifier(name, &opts.allowed_identifiers) {
                continue;
            }

            if forbidden_name(name, &opts, cx) {
                emit_forbidden(name, range, cx);
            } else if !valid_name(name, &opts, cx) {
                let msg = format!("Use {} for variable names.", opts.enforced_style.as_str());
                cx.emit_offense(range, &msg, None);
            }
        }
    }
}

/// `valid_name?`: `style_regex(name) || matches_allowed_pattern?(name)`.
fn valid_name(name: &str, opts: &Options, cx: &Cx<'_>) -> bool {
    opts.enforced_style.matches(name) || cx.matches_any_pattern(name, &opts.allowed_patterns)
}

/// `forbidden_name?`: `forbidden_identifier?(name) || forbidden_pattern?(name)`.
fn forbidden_name(name: &str, opts: &Options, cx: &Cx<'_>) -> bool {
    forbidden_identifier(name, &opts.forbidden_identifiers)
        || cx.matches_any_pattern(name, &opts.forbidden_patterns)
}

/// RuboCop's `allowed_identifier?`: `name.delete("@$")` then exact membership.
fn allowed_identifier(name: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let stripped = strip_sigils(name);
    allowed.contains(&stripped)
}

/// RuboCop's `forbidden_identifier?`: `name.delete("@$")` then exact membership.
fn forbidden_identifier(name: &str, forbidden: &[String]) -> bool {
    if forbidden.is_empty() {
        return false;
    }
    let stripped = strip_sigils(name);
    forbidden.contains(&stripped)
}

/// Ruby's `name.delete("@$")` — remove every `@` and `$` (not only leading).
fn strip_sigils(name: &str) -> String {
    name.chars().filter(|&c| c != '@' && c != '$').collect()
}

fn emit_forbidden(name: &str, range: Range, cx: &Cx<'_>) {
    let msg = format!("`{name}` {MSG_FORBIDDEN}");
    cx.emit_offense(range, &msg, None);
}

/// Resolve `(name, loc.name-range, is_global)` for the node kinds RuboCop
/// visits. `None` for every other kind. `is_global` selects the forbidden-only
/// path for `gvasgn`.
fn variable_target<'a>(id: NodeId, cx: &Cx<'a>) -> Option<(&'a str, Range, bool)> {
    match *cx.kind(id) {
        // Assignment family + lvar read: name carries any sigil; `loc.name` is
        // ZERO, so search the source from the node start.
        NodeKind::Lvar(name)
        | NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. } => {
            let s = cx.symbol_str(name);
            Some((s, named_range(id, s, cx), false))
        }
        NodeKind::Gvasgn { name, .. } => {
            let s = cx.symbol_str(name);
            Some((s, named_range(id, s, cx), true))
        }
        // Argument family: `loc.name` is populated and excludes the sigil/colon.
        NodeKind::Arg(name)
        | NodeKind::Restarg(name)
        | NodeKind::Kwarg(name)
        | NodeKind::Kwrestarg(name)
        | NodeKind::Blockarg(name) => {
            let s = cx.symbol_str(name);
            Some((s, arg_range(id, s, cx), false))
        }
        NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. } => {
            let s = cx.symbol_str(name);
            Some((s, arg_range(id, s, cx), false))
        }
        _ => None,
    }
}

/// `loc.name` for the assignment family. Murphy leaves it ZERO, so locate the
/// name (incl. sigil) by its first occurrence from the node start. The name
/// precedes any `=`/`:` so the first hit is correct.
fn named_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    match src.find(name) {
        Some(off) => {
            let start = expr.start + off as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start + name.len() as u32,
        },
    }
}

/// `loc.name` for the argument family. Murphy's `loc.name` starts at the bare
/// name (after `*`/`**`/`&`) but, for keyword parameters, spans the trailing
/// `:` of the label. RuboCop's `node.loc.name` is just the bare name, so anchor
/// at `loc.name.start` and use the symbol length, dropping any trailing colon.
/// Falls back to a source search if `loc.name` is unset.
fn arg_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let name_loc = cx.node(id).loc.name;
    if name_loc == Range::ZERO {
        named_range(id, name, cx)
    } else {
        Range {
            start: name_loc.start,
            end: name_loc.start + name.len() as u32,
        }
    }
}

/// snake_case: RuboCop's `/^@{0,2}[\d[[:lower:]]_]+[!?=]?$/`.
///
/// Leading `@{0,2}` (0–2 `@`), then one-or-more of digit / ASCII-lowercase /
/// `_`, then an optional single `!`/`?`/`=`.
fn is_snake_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;

    // `@{0,2}`
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }

    // optional trailing `[!?=]`
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }

    // `[\d[[:lower:]]_]+` — at least one character required.
    if end <= i {
        return false;
    }
    bytes[i..end]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// camelCase: RuboCop's
/// `/^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/`.
///
/// Leading `@{0,2}`; then EITHER a single `_`, OR an optional leading `_`
/// followed by an ASCII-lowercase letter and any run of digit / lower / upper;
/// then an optional single `!`/`?`/`=`.
fn is_camel_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;

    // `@{0,2}`
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }

    // optional trailing `[!?=]`
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }

    let body = &bytes[i..end];

    // Alternative 1: a single `_`.
    if body == b"_" {
        return true;
    }

    // Alternative 2: `_?[[:lower:]][\d[[:lower:]][[:upper:]]]*`.
    let mut j = 0;
    if j < body.len() && body[j] == b'_' {
        j += 1;
    }
    // Required ASCII-lowercase letter.
    if j >= body.len() || !body[j].is_ascii_lowercase() {
        return false;
    }
    j += 1;
    // Remaining: digit / lower / upper.
    body[j..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{Options, VariableName, VariableNameStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(style: VariableNameStyle) -> Options {
        Options {
            enforced_style: style,
            allowed_identifiers: vec![],
            allowed_patterns: vec![],
            forbidden_identifiers: vec![],
            forbidden_patterns: vec![],
        }
    }

    // --- snake_case (default); carets from rubocop 1.87.0 col..last_column. ---

    #[test]
    fn flags_camel_local_variable() {
        // rubocop: `fooBar = 1` col 1..6.
        test::<VariableName>().expect_offense(indoc! {r#"
            fooBar = 1
            ^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_local_variable_read() {
        // `on_lvar` is aliased: assignment AND read both fire.
        test::<VariableName>().expect_offense(indoc! {r#"
            fooBar = 1
            ^^^^^^ Use snake_case for variable names.
            puts fooBar
                 ^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_instance_variable_with_sigil() {
        // `@fooBar = 1` col 1..7 — caret spans the `@`.
        test::<VariableName>().expect_offense(indoc! {r#"
            @fooBar = 1
            ^^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_class_variable_with_sigil() {
        // `@@bazQux = 2` col 1..8 — caret spans the `@@`.
        test::<VariableName>().expect_offense(indoc! {r#"
            @@bazQux = 2
            ^^^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn ignores_instance_variable_read() {
        // No `on_ivar` alias: a bare ivar read is not flagged.
        test::<VariableName>().expect_offense(indoc! {r#"
            @fooBar = 1
            ^^^^^^^ Use snake_case for variable names.
            @fooBar
        "#});
    }

    #[test]
    fn ignores_class_variable_read() {
        test::<VariableName>().expect_offense(indoc! {r#"
            @@barBaz = 2
            ^^^^^^^^ Use snake_case for variable names.
            @@barBaz
        "#});
    }

    // --- argument family (carets from rubocop 1.87.0) ---

    #[test]
    fn flags_positional_argument() {
        // `def m(argOne)` — arg `argOne` col 7..12, plus the body read col 3..8.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(argOne)
                  ^^^^^^ Use snake_case for variable names.
              argOne
              ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_optional_argument() {
        // `def m(optOne = 1)` — `optOne` col 7..12.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(optOne = 1)
                  ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_splat_argument() {
        // `def m(*restArg)` — bare `restArg` after the `*`, col 8..14.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(*restArg)
                   ^^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_required_keyword_argument() {
        // `def m(kwArg:)` — bare `kwArg` (no colon), col 7..11.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(kwArg:)
                  ^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_optional_keyword_argument() {
        // `def m(kwOpt: 1)` — bare `kwOpt`, col 7..11.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(kwOpt: 1)
                  ^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_keyword_splat_argument() {
        // `def m(**kwRest)` — bare `kwRest` after `**`, col 9..14.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(**kwRest)
                    ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_block_argument() {
        // `def m(&blkArg)` — bare `blkArg` after `&`, col 8..13.
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(&blkArg)
                   ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn ignores_anonymous_arguments() {
        // Anonymous `*`/`**`/`&` intern an empty name; RuboCop short-circuits
        // on the nil name, so only the named `badArg` fires (col 7..12).
        test::<VariableName>().expect_offense(indoc! {r#"
            def m(badArg, *, **, &)
                  ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    #[test]
    fn flags_block_parameter_and_read() {
        // `proc { |blkParam| blkParam }` — param col 9..16, read col 19..26.
        test::<VariableName>().expect_offense(indoc! {r#"
            proc { |blkParam| blkParam }
                    ^^^^^^^^ Use snake_case for variable names.
                              ^^^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_multiple_assignment_targets() {
        // `fooBar, barBaz = 1, 2` — both masgn targets fire (col 1..6, 9..14).
        test::<VariableName>().expect_offense(indoc! {r#"
            fooBar, barBaz = 1, 2
            ^^^^^^ Use snake_case for variable names.
                    ^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_op_assignment_target() {
        // `fooBar += 1` — op-asgn target, col 1..6.
        test::<VariableName>().expect_offense(indoc! {r#"
            fooBar += 1
            ^^^^^^ Use snake_case for variable names.
        "#});
    }

    #[test]
    fn flags_for_loop_variable_and_read() {
        // `for forVar in [1]` — binding col 5..10, read in body col 3..8.
        test::<VariableName>().expect_offense(indoc! {r#"
            for forVar in [1]
                ^^^^^^ Use snake_case for variable names.
              forVar
              ^^^^^^ Use snake_case for variable names.
            end
        "#});
    }

    // --- global variables: forbidden-only ---

    #[test]
    fn ignores_global_variable_style() {
        // Globals are never style-checked.
        test::<VariableName>().expect_no_offenses("$globVar = 3\n");
    }

    #[test]
    fn flags_forbidden_global_variable() {
        // ForbiddenIdentifiers strips `@$`, so `$fooBar` matches `fooBar`.
        test::<VariableName>()
            .with_options(&Options {
                forbidden_identifiers: vec!["fooBar".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                $fooBar = 1
                ^^^^^^^ `$fooBar` is forbidden, use another name instead.
            "#});
    }

    // --- conforming names (snake_case) ---

    #[test]
    fn accepts_snake_case_names() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
            foo_bar = 1
            foo1 = 2
            foo_1 = 3
            a = 4
            _unused = 5
            __ = 6
            @ivar_ok = 7
            @@cvar_ok = 8
        "#});
    }

    // --- camelCase style ---

    #[test]
    fn camel_flags_snake_case_name() {
        // With camelCase: `foo_bar` col 1..7 flagged.
        test::<VariableName>()
            .with_options(&opts(VariableNameStyle::CamelCase))
            .expect_offense(indoc! {r#"
                foo_bar = 1
                ^^^^^^^ Use camelCase for variable names.
            "#});
    }

    #[test]
    fn camel_accepts_camel_case_names() {
        test::<VariableName>()
            .with_options(&opts(VariableNameStyle::CamelCase))
            .expect_no_offenses(indoc! {r#"
                fooBar = 1
                fooBar2 = 2
                _foo = 3
                foo = 4
            "#});
    }

    // --- AllowedIdentifiers / AllowedPatterns ---

    #[test]
    fn allowed_identifier_skips_offense() {
        test::<VariableName>()
            .with_options(&Options {
                allowed_identifiers: vec!["fooBar".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_no_offenses("fooBar = 1\n");
    }

    #[test]
    fn allowed_identifier_strips_sigil() {
        // `AllowedIdentifiers: ['fooBar']` allows `@fooBar` (sigil stripped).
        test::<VariableName>()
            .with_options(&Options {
                allowed_identifiers: vec!["fooBar".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_no_offenses("@fooBar = 1\n");
    }

    #[test]
    fn allowed_pattern_skips_offense() {
        // camelCase + AllowedPatterns `_v\d+\z` allows `release_v1`.
        test::<VariableName>()
            .with_options(&Options {
                allowed_patterns: vec![r"_v\d+\z".to_string()],
                ..opts(VariableNameStyle::CamelCase)
            })
            .expect_no_offenses("release_v1 = 1\n");
    }

    // --- ForbiddenIdentifiers / ForbiddenPatterns ---

    #[test]
    fn forbidden_identifier_flags_conforming_name() {
        // A snake_case-valid name still fires when forbidden.
        test::<VariableName>()
            .with_options(&Options {
                forbidden_identifiers: vec!["foo_bar".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                foo_bar = 1
                ^^^^^^^ `foo_bar` is forbidden, use another name instead.
            "#});
    }

    #[test]
    fn forbidden_identifier_strips_sigil_on_ivar() {
        // `ForbiddenIdentifiers: ['fooBar']` matches `@fooBar` (sigil stripped);
        // message keeps the full sigil-bearing name.
        test::<VariableName>()
            .with_options(&Options {
                forbidden_identifiers: vec!["fooBar".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                @fooBar = 1
                ^^^^^^^ `@fooBar` is forbidden, use another name instead.
            "#});
    }

    #[test]
    fn forbidden_pattern_flags_name() {
        test::<VariableName>()
            .with_options(&Options {
                forbidden_patterns: vec![r"_v\d+\z".to_string()],
                ..opts(VariableNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                release_v1 = 1
                ^^^^^^^^^^ `release_v1` is forbidden, use another name instead.
            "#});
    }

    // --- not affected: method defs/calls, constants ---

    #[test]
    fn ignores_method_definition_and_call() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
            def fooBar
            end
            obj.barBaz
        "#});
    }

    #[test]
    fn ignores_constant() {
        // `FooBar = 1` is a casgn, handled by Naming/ConstantName.
        test::<VariableName>().expect_no_offenses("FooBar = 1\n");
    }
}
murphy_plugin_api::submit_cop!(VariableName);
