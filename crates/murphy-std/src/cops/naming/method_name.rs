//! `Naming/MethodName` — enforce the configured style (`snake_case` /
//! `camelCase`) for method definition names (`def` / `defs`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/MethodName
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-e7bz.68]
//! notes: >
//!   Faithful port of RuboCop's `on_def` (aliased to `on_defs`):
//!
//!     return if node.operator_method? || matches_allowed_pattern?(name)
//!     if forbidden_name?(name)         # Forbidden{Identifiers,Patterns}
//!       register_forbidden_name(node)  # MSG_FORBIDDEN, range = loc.name
//!     else
//!       check_name(node, name, loc.name)   # valid_name? = style_regex
//!
//!   Control-flow precedence is exact and DIFFERS from `Naming/VariableName`:
//!   AllowedPatterns is an EARLY return BEFORE the forbidden check, so a name
//!   matching both an AllowedPattern and a ForbiddenPattern is skipped
//!   (verified against rubocop 1.87.0: `AllowedPatterns: ['Foo']` +
//!   `ForbiddenPatterns: ['Foo']` on `def fooFoo` → no offense). Precedence:
//!   operator (skip) > AllowedPatterns (skip) > Forbidden{Identifiers,Patterns}
//!   (forbidden offense) > style-regex (style offense).
//!
//!   Operator methods are exempt via `node.operator_method?`, mirrored by
//!   `method_predicates::is_operator_method` over RuboCop's OPERATOR_METHODS set
//!   (`| ^ & <=> == === =~ > >= < <= << >> + - * / % ** ~ +@ -@ !@ ~@ [] []= !
//!   != !~ \``). Verified: `def coerce` (regular, snake-valid → no offense),
//!   `def []=`, `def +@`, `def \`` → all skipped.
//!
//!   ForbiddenIdentifiers defaults to `[__id__, __send__]` (RuboCop default.yml),
//!   so with no config `def __send__` fires MSG_FORBIDDEN (verified). Identifier
//!   matching uses RuboCop's `name.delete("@$")` then exact membership; method
//!   names carry no sigil, so this is a plain exact compare in practice.
//!   Allowed/Forbidden Patterns match the FULL method name (unanchored) via
//!   `cx.matches_any_pattern`.
//!
//!   Offense range mirrors `node.loc.name`: the bare method-name token,
//!   INCLUDING any trailing `=` (setters), `?` (predicates), or `!` (bang).
//!   Murphy leaves `loc.name == ZERO` on `def`/`defs`, so the name is located by
//!   source search starting past any singleton receiver (`def self.x`). The def
//!   symbol Murphy interns already carries the suffix (`:setSomething=`,
//!   `:isReady?`), so the range spans it (verified `def setSomething=` col 5..17,
//!   `def isReady?` col 5..12).
//!
//!   Style regexes are byte-level ports of RuboCop's FORMATS hash, shared in
//!   spirit with `Naming/VariableName`:
//!     snake_case: /^@{0,2}[\d[[:lower:]]_]+[!?=]?$/
//!     camelCase:  /^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/
//!   The `@{0,2}` prefix is vestigial for method names (they never start `@`),
//!   but is kept verbatim so the port matches RuboCop's source. Verified
//!   column-for-column against rubocop 1.87.0 for both styles.
//!
//!   Known gaps vs RuboCop:
//!     * The `on_send` handler family is NOT ported (gap issue
//!       murphy-e7bz.68): `define_method`/`define_singleton_method`
//!       dynamic definitions, `Struct.new`/`Data.define` member names,
//!       `alias_method`, and `attr_accessor`/`attr_reader`/`attr_writer` accessor
//!       names. Only literal `def`/`defs` are checked.
//!     * `class_emitter_method?` is NOT implemented: RuboCop treats
//!       `def self.Foo` as valid when a sibling `class Foo` exists in the same
//!       parent scope (verified: such a def gets NO offense in rubocop). Murphy
//!       flags it. This is a rare construct.
//!     * `[[:lower:]]`/`[[:upper:]]` are Unicode-aware in Ruby; the byte checks
//!       here are ASCII-only (the same documented limitation `Naming/VariableName`
//!       and `Naming/ConstantName` carry). `Naming/AsciiIdentifiers` already
//!       flags non-ASCII method names.
//! ```
//!
//! ## Offense range
//!
//! `node.loc.name`: the bare method-name token including any trailing
//! `=`/`?`/`!`, excluding a singleton receiver (`def self.x` → `x`).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop, method_predicates};

#[derive(Default)]
pub struct MethodName;

/// Enforced naming style for method definitions.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MethodNameStyle {
    /// `snake_case` — RuboCop default.
    #[default]
    #[option(value = "snake_case")]
    SnakeCase,
    /// `camelCase`.
    #[option(value = "camelCase")]
    CamelCase,
}

impl MethodNameStyle {
    fn as_str(self) -> &'static str {
        match self {
            MethodNameStyle::SnakeCase => "snake_case",
            MethodNameStyle::CamelCase => "camelCase",
        }
    }

    /// RuboCop's `FORMATS.fetch(style).match?(name)`.
    fn matches(self, name: &str) -> bool {
        match self {
            MethodNameStyle::SnakeCase => is_snake_case(name),
            MethodNameStyle::CamelCase => is_camel_case(name),
        }
    }
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "snake_case",
        description = "Required method-name style: `snake_case` (default) or `camelCase`."
    )]
    pub enforced_style: MethodNameStyle,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a method whose name matches any is always allowed."
    )]
    pub allowed_patterns: Vec<String>,
    #[option(
        name = "ForbiddenIdentifiers",
        default = ["__id__", "__send__"],
        description = "Exact method names that are forbidden."
    )]
    pub forbidden_identifiers: Vec<String>,
    #[option(
        name = "ForbiddenPatterns",
        default = [],
        description = "Regexes; a method whose name matches any is forbidden."
    )]
    pub forbidden_patterns: Vec<String>,
}

const MSG_FORBIDDEN: &str = "is forbidden, use another method name instead.";

#[cop(
    name = "Naming/MethodName",
    description = "Use the configured style when naming methods.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MethodName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root; chain it so a lone top-level `def`
        // (whose root *is* the def) is also inspected.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            // `def`/`defs` only — the on_send handler family (define_method,
            // Struct.new, alias_method, attr_accessor) is out of scope.
            let name = match *cx.kind(id) {
                NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => cx.symbol_str(name),
                _ => continue,
            };

            // `return if node.operator_method? || matches_allowed_pattern?(name)`.
            // Both are early returns BEFORE the forbidden check.
            if method_predicates::is_operator_method(name)
                || cx.matches_any_pattern(name, &opts.allowed_patterns)
            {
                continue;
            }

            let range = def_name_range(id, name, cx);

            if forbidden_name(name, &opts, cx) {
                let msg = format!("`{name}` {MSG_FORBIDDEN}");
                cx.emit_offense(range, &msg, None);
            } else if !opts.enforced_style.matches(name) {
                // `check_name`: valid_name? is the style regex only —
                // AllowedPatterns was already handled by the early return above.
                let msg = format!("Use {} for method names.", opts.enforced_style.as_str());
                cx.emit_offense(range, &msg, None);
            }
        }
    }
}

/// `forbidden_name?`: `forbidden_identifier?(name) || forbidden_pattern?(name)`.
fn forbidden_name(name: &str, opts: &Options, cx: &Cx<'_>) -> bool {
    forbidden_identifier(name, &opts.forbidden_identifiers)
        || cx.matches_any_pattern(name, &opts.forbidden_patterns)
}

/// RuboCop's `forbidden_identifier?`: `name.delete("@$")` then exact membership.
/// Method names never carry `@`/`$`, so this is an exact compare in practice;
/// the strip is kept for byte-for-byte fidelity with the mixin.
fn forbidden_identifier(name: &str, forbidden: &[String]) -> bool {
    if forbidden.is_empty() {
        return false;
    }
    let stripped: String = name.chars().filter(|&c| c != '@' && c != '$').collect();
    forbidden.contains(&stripped)
}

/// Byte range of the method name within a `def`/`defs` definition, mirroring
/// RuboCop's `node.loc.name`.
///
/// Murphy leaves `loc.name` as `Range::ZERO` for `Def`/`Defs`, so the name is
/// located by its first occurrence in the node's source, starting past any
/// singleton receiver (`def self.x` / `def Foo.x`) so a receiver whose source
/// contains the name as a substring cannot mis-anchor the caret. Beyond the
/// receiver the name always precedes the argument list and body. The interned
/// def symbol already carries any trailing `=`/`?`/`!`, so the range spans it.
/// Falls back to a zero-width caret at the node start if the name is not found.
fn def_name_range(id: NodeId, name: &str, cx: &Cx<'_>) -> Range {
    let expr = cx.range(id);
    let src = cx.raw_source(expr);
    let from = cx
        .def_receiver(id)
        .get()
        .map_or(0, |r| (cx.range(r).end - expr.start) as usize);
    match src[from..].find(name) {
        Some(off) => {
            let start = expr.start + (from + off) as u32;
            Range {
                start,
                end: start + name.len() as u32,
            }
        }
        None => Range {
            start: expr.start,
            end: expr.start,
        },
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
    use super::{MethodName, MethodNameStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(style: MethodNameStyle) -> Options {
        Options {
            enforced_style: style,
            allowed_patterns: vec![],
            forbidden_identifiers: vec![],
            forbidden_patterns: vec![],
        }
    }

    // --- snake_case (default); carets from rubocop 1.87.0 col..last_column. ---

    #[test]
    fn flags_camel_case_method_definition() {
        // rubocop: `def fooBar` col 5..10.
        test::<MethodName>().expect_offense(indoc! {r#"
            def fooBar
                ^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_camel_case_method_no_underscore() {
        // `def goodName` col 5..12.
        test::<MethodName>().expect_offense(indoc! {r#"
            def goodName
                ^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_setter_definition_including_equals() {
        // `def setSomething=(val)` — loc.name spans the trailing `=`, col 5..17.
        test::<MethodName>().expect_offense(indoc! {r#"
            def setSomething=(val)
                ^^^^^^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_predicate_definition_including_question() {
        // `def isReady?` — loc.name spans the trailing `?`, col 5..12.
        test::<MethodName>().expect_offense(indoc! {r#"
            def isReady?
                ^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_bang_definition_including_bang() {
        // `def doIt!` — loc.name spans the trailing `!`, col 5..9.
        test::<MethodName>().expect_offense(indoc! {r#"
            def doIt!
                ^^^^^ Use snake_case for method names.
            end
        "#});
    }

    #[test]
    fn flags_singleton_method_definition() {
        // `def self.classMethod` — name after `self.`, col 10..20.
        test::<MethodName>().expect_offense(indoc! {r#"
            def self.classMethod
                     ^^^^^^^^^^^ Use snake_case for method names.
            end
        "#});
    }

    // --- conforming / exempt (snake_case) ---

    #[test]
    fn accepts_snake_case_definitions() {
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            def foo_bar
            end
            def foo1
            end
            def foo_1
            end
            def coerce(other)
            end
            def valid?
            end
            def save!
            end
        "#});
    }

    #[test]
    fn ignores_operator_methods() {
        // `==`, `[]=`, `+@`, `` ` `` are operator methods — all exempt.
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            def ==(other)
            end
            def [](key)
            end
            def []=(key, value)
            end
            def +@
            end
        "#});
    }

    #[test]
    fn ignores_method_calls_and_variables() {
        // Only `def`/`defs` are checked, not calls or assignments.
        test::<MethodName>().expect_no_offenses(indoc! {r#"
            obj.fooBar
            barBaz = 1
        "#});
    }

    // --- ForbiddenIdentifiers (default __id__/__send__) ---

    #[test]
    fn flags_forbidden_default_identifier() {
        // Default ForbiddenIdentifiers carries `__send__`; with no options the
        // Rust `Options::default()` must include it.
        test::<MethodName>().expect_offense(indoc! {r#"
            def __send__
                ^^^^^^^^ `__send__` is forbidden, use another method name instead.
            end
        "#});
    }

    #[test]
    fn flags_forbidden_default_id() {
        test::<MethodName>().expect_offense(indoc! {r#"
            def __id__
                ^^^^^^ `__id__` is forbidden, use another method name instead.
            end
        "#});
    }

    #[test]
    fn forbidden_identifier_flags_conforming_name() {
        // A snake_case-valid name still fires when forbidden.
        test::<MethodName>()
            .with_options(&Options {
                forbidden_identifiers: vec!["foo_bar".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                def foo_bar
                    ^^^^^^^ `foo_bar` is forbidden, use another method name instead.
                end
            "#});
    }

    #[test]
    fn forbidden_pattern_flags_name() {
        test::<MethodName>()
            .with_options(&Options {
                forbidden_patterns: vec![r"_v\d+\z".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_offense(indoc! {r#"
                def release_v1
                    ^^^^^^^^^^ `release_v1` is forbidden, use another method name instead.
                end
            "#});
    }

    // --- AllowedPatterns (early-return precedence) ---

    #[test]
    fn allowed_pattern_skips_offense() {
        test::<MethodName>()
            .with_options(&Options {
                allowed_patterns: vec![r"\AonSelection".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                def onSelectionChange
                end
            "#});
    }

    #[test]
    fn allowed_pattern_beats_forbidden_pattern() {
        // RuboCop's AllowedPatterns early-return precedes the forbidden check:
        // a name matching BOTH is skipped (verified against rubocop 1.87.0).
        test::<MethodName>()
            .with_options(&Options {
                allowed_patterns: vec!["Foo".to_string()],
                forbidden_patterns: vec!["Foo".to_string()],
                ..opts(MethodNameStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                def fooFoo
                end
            "#});
    }

    // --- camelCase style ---

    #[test]
    fn camel_flags_snake_case_definition() {
        // With camelCase: `def foo_bar` col 5..11 flagged.
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_offense(indoc! {r#"
                def foo_bar
                    ^^^^^^^ Use camelCase for method names.
                end
            "#});
    }

    #[test]
    fn camel_accepts_camel_case_definitions() {
        test::<MethodName>()
            .with_options(&opts(MethodNameStyle::CamelCase))
            .expect_no_offenses(indoc! {r#"
                def fooBar
                end
                def fooBar2
                end
                def foo
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(MethodName);
