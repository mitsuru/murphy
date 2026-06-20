//! `Naming/VariableNumber` — enforce the configured style (`normalcase` /
//! `snake_case` / `non_integer`) when numbering variables, method names and
//! symbols.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/VariableNumber
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `ConfigurableNumbering` mixin as used by
//!   `Naming/VariableNumber`. Handlers (verified against rubocop 1.87.0):
//!
//!     on_arg                              # required positional args only
//!     on_lvasgn/on_ivasgn/on_cvasgn/on_gvasgn  # variable ASSIGNMENTS
//!     on_def/on_defs   (CheckMethodNames) # method-definition names
//!     on_sym           (CheckSymbols)     # every symbol literal
//!
//!   Per-node control flow:
//!     return if allowed_identifier?(name)        # AllowedIdentifiers (skip)
//!     valid = FORMATS[style].match?(name)
//!             || matches_allowed_pattern?(name)  # AllowedPatterns
//!     add_offense(name_range) unless valid
//!
//!   Node coverage (each row verified against rubocop 1.87.0):
//!     * variable ASSIGNMENTS only — `lvasgn`, `ivasgn` (`@x`), `cvasgn`
//!       (`@@x`), `gvasgn` (`$x`). Reads (`lvar`/`ivar`/`cvar`/`gvar`) are NOT
//!       flagged (no `on_lvar`/etc. alias). `x_1 = 1; x_1` fires once.
//!       Unlike `Naming/VariableName`, globals ARE style-checked here.
//!     * required positional arguments only (`arg`). `optarg`, `restarg`,
//!       `kwarg`, `kwoptarg`, `kwrestarg`, `blockarg` are NOT checked — the
//!       mixin only aliases `on_arg` (verified `def m(a1, b2=1, *c3, d4:,
//!       e5:1, **f6, &g7)` flags only `a1`).
//!     * method-definition names (`def`/`defs`) when CheckMethodNames (default
//!       true). Method CALLS (`send`/`csend`) are NOT checked.
//!     * every symbol literal (`sym`) when CheckSymbols (default true),
//!       including hash-key labels, `attr_accessor :foo1`, method-call
//!       symbol args, and `alias_method :new1, :old2`.
//!
//!   Name source / sigils: `cx.symbol_str` carries the sigil for ivar/cvar/
//!   gvar (`@x`/`@@x`/`$x`), matching RuboCop's `node.name` (`:@x` etc.). The
//!   style regexes test the name suffix, so the leading sigil is inert.
//!   AllowedIdentifiers mirrors RuboCop's shared mixin
//!   `allowed_identifiers.include?(name.to_s.delete("@$"))`: every `@`/`$` is
//!   stripped before an exact membership test (`SIGILS == "@$"`, the `:` of a
//!   symbol is not a sigil). Verified: `capture3` allowed excludes `@capture3`,
//!   `$capture3`, and `@@capture3` against rubocop 1.87.0.
//!
//!   Offense range mirrors RuboCop's `name_range`:
//!     * variable assignment / arg / def — `node.loc.name` (the bare name; for
//!       ivar/cvar/gvar the caret spans the sigil, verified `@foo1` col 1..5,
//!       `@@bar1` col 1..6, `$baz1` col 1..5). Murphy leaves `loc.name == ZERO`
//!       on assignment nodes, so the name is located by source search; `arg`
//!       and `def` carry a populated `loc.name`.
//!     * symbol — the whole `sym` node: standalone `:sym1` spans the colon
//!       (col 5..9), a hash-key label `key1:` does not (col 7..10). Murphy's
//!       `cx.range(sym)` reproduces both, matching RuboCop's `node` range.
//!
//!   Style regexes are byte-level ports of RuboCop's `ConfigurableNumbering`
//!   FORMATS hash (cross-checked against a Ruby oracle for
//!   x1/x_1/foo1/_1/_42/v2_3/method1/valid1?/foo10bar/123/a1b2 × 3 styles):
//!     snake_case:  /(?:\D|_\d+|\A\d+)\z/
//!     normalcase:  /(?:\D|[^_\d]\d+|\A\d+)\z|\A_\d+\z/
//!     non_integer: /(\D|\A\d+)\z|\A_\d+\z/
//!
//!   Documented gap (niche; status stays `verified`): RuboCop's
//!   `class_emitter_method?` exception (a `def self.X` matching an inner class
//!   name) is not reproduced — it suppresses offenses for a rare singleton-
//!   method-vs-class-name collision and never fires on normal numbered names.
//! ```
//!
//! ## Offense range
//!
//! `name_range`: the bare name for variable/arg/def (caret spans the
//! ivar/cvar/gvar sigil); the whole `sym` node for symbols (the colon is
//! included for a standalone symbol, excluded for a hash-key label).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct VariableNumber;

/// Enforced numbering style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VariableNumberStyle {
    /// `normalcase` — `variable1` (RuboCop default).
    #[default]
    #[option(value = "normalcase")]
    NormalCase,
    /// `snake_case` — `variable_1`.
    #[option(value = "snake_case")]
    SnakeCase,
    /// `non_integer` — no integer suffix (`variableone`).
    #[option(value = "non_integer")]
    NonInteger,
}

impl VariableNumberStyle {
    fn as_str(self) -> &'static str {
        match self {
            VariableNumberStyle::NormalCase => "normalcase",
            VariableNumberStyle::SnakeCase => "snake_case",
            VariableNumberStyle::NonInteger => "non_integer",
        }
    }

    /// RuboCop's `FORMATS.fetch(style).match?(name)`.
    fn matches(self, name: &str) -> bool {
        match self {
            VariableNumberStyle::NormalCase => is_normalcase(name),
            VariableNumberStyle::SnakeCase => is_snake_case(name),
            VariableNumberStyle::NonInteger => is_non_integer(name),
        }
    }
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "normalcase",
        description = "Required numbering style: `normalcase` (default), `snake_case`, or `non_integer`."
    )]
    pub enforced_style: VariableNumberStyle,
    #[option(
        name = "CheckMethodNames",
        default = true,
        description = "Check method-definition names for the configured numbering style."
    )]
    pub check_method_names: bool,
    #[option(
        name = "CheckSymbols",
        default = true,
        description = "Check symbol literals for the configured numbering style."
    )]
    pub check_symbols: bool,
    #[option(
        name = "AllowedIdentifiers",
        default = [
            "TLS1_1",
            "TLS1_2",
            "capture3",
            "iso8601",
            "rfc1123_date",
            "rfc822",
            "rfc2822",
            "rfc3339",
            "x86_64"
        ],
        description = "Exact names that are always allowed."
    )]
    pub allowed_identifiers: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a name matching any is allowed."
    )]
    pub allowed_patterns: Vec<String>,
}

/// What kind of identifier the offending node is, for the message.
#[derive(Clone, Copy)]
enum IdentifierType {
    Variable,
    MethodName,
    Symbol,
}

impl IdentifierType {
    fn as_str(self) -> &'static str {
        match self {
            IdentifierType::Variable => "variable",
            IdentifierType::MethodName => "method name",
            IdentifierType::Symbol => "symbol",
        }
    }
}

#[cop(
    name = "Naming/VariableNumber",
    description = "Use the configured style when numbering symbols, methods and variables.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl VariableNumber {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root; chain it so a lone top-level
        // statement (e.g. `x_1 = 1`, whose root *is* the lvasgn) is seen.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            let Some((name, range, kind)) = numbered_target(id, &opts, cx) else {
                continue;
            };

            // RuboCop short-circuits on a nil name (anonymous splat/block).
            if name.is_empty() {
                continue;
            }

            // `return if allowed_identifier?(name)` — RuboCop's
            // `allowed_identifiers.include?(name.to_s.delete("@$"))`: strip every
            // `@`/`$` sigil, then exact membership.
            if allowed_identifier(name, &opts.allowed_identifiers) {
                continue;
            }

            // `valid_name? = FORMATS[style].match?(name) || allowed_pattern?`.
            if opts.enforced_style.matches(name)
                || cx.matches_any_pattern(name, &opts.allowed_patterns)
            {
                continue;
            }

            let msg = format!(
                "Use {} for {} numbers.",
                opts.enforced_style.as_str(),
                kind.as_str()
            );
            cx.emit_offense(range, &msg, None);
        }
    }
}

/// RuboCop's `allowed_identifier?`: `name.to_s.delete("@$")` then exact
/// membership in the (non-empty) allow list. The `:` of a symbol is not a
/// sigil here — `name` for a `sym` is already the bare symbol name.
fn allowed_identifier(name: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let stripped: String = name.chars().filter(|&c| c != '@' && c != '$').collect();
    allowed.contains(&stripped)
}

/// Resolve `(name, name_range, identifier_type)` for the node kinds RuboCop's
/// `ConfigurableNumbering` mixin visits. `None` for every other kind. The
/// `CheckMethodNames` / `CheckSymbols` options gate `def`/`defs` and `sym`.
fn numbered_target<'a>(
    id: NodeId,
    opts: &Options,
    cx: &Cx<'a>,
) -> Option<(&'a str, Range, IdentifierType)> {
    match *cx.kind(id) {
        // Variable ASSIGNMENTS (not reads). Names carry any sigil; Murphy
        // leaves `loc.name == ZERO`, so locate the name by source search.
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. } => {
            let s = cx.symbol_str(name);
            Some((s, named_range(id, s, cx), IdentifierType::Variable))
        }
        // Required positional arguments only. `loc.name` is populated.
        NodeKind::Arg(name) => {
            let s = cx.symbol_str(name);
            Some((s, arg_range(id, s, cx), IdentifierType::Variable))
        }
        // Method-definition names (gated by CheckMethodNames). `loc.name` is
        // populated and already excludes the `def`/receiver.
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } if opts.check_method_names => {
            let s = cx.symbol_str(name);
            Some((s, arg_range(id, s, cx), IdentifierType::MethodName))
        }
        // Every symbol literal (gated by CheckSymbols). RuboCop reports the
        // whole `sym` node: a standalone `:sym1` keeps its leading colon, a
        // hash-key label `key1:` drops its trailing colon. Murphy's
        // `cx.range(sym)` keeps the leading colon (matching RuboCop) but also
        // keeps the label's trailing colon, so strip a trailing `:`.
        NodeKind::Sym(name) if opts.check_symbols => {
            let s = cx.symbol_str(name);
            Some((s, sym_range(id, cx), IdentifierType::Symbol))
        }
        _ => None,
    }
}

/// `loc.name` for the assignment family. Murphy leaves it ZERO, so locate the
/// name (incl. sigil) by its first occurrence from the node start. The name
/// precedes any `=` so the first hit is correct.
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

/// `loc.name` for nodes that populate it (`arg`, `def`, `defs`). RuboCop's
/// `node.loc.name` is the bare name, so anchor at `loc.name.start` and use the
/// symbol length. Falls back to a source search if `loc.name` is unset.
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

/// Offense range for a `sym` node, mirroring RuboCop's `node` range. Murphy's
/// `cx.range(sym)` keeps a hash-key label's trailing `:` (e.g. `key1:`), which
/// RuboCop excludes, so drop a single trailing `:`. A standalone symbol's
/// leading `:` is kept by both.
fn sym_range(id: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(id);
    let src = cx.raw_source(range);
    if src.ends_with(':') {
        Range {
            start: range.start,
            end: range.end - 1,
        }
    } else {
        range
    }
}

// --- FORMATS regexes (byte-level ports of RuboCop's ConfigurableNumbering) ---

/// True if `name` is the implicit-numbered-parameter form `\A_\d+\z`: a single
/// leading `_` followed by one-or-more digits and nothing else.
fn is_implicit_param(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'_' && bytes[1..].iter().all(u8::is_ascii_digit)
}

/// True if every byte of `name` is an ASCII digit (non-empty) — RuboCop's
/// `\A\d+\z` alternative.
fn is_all_digits(name: &str) -> bool {
    !name.is_empty() && name.as_bytes().iter().all(u8::is_ascii_digit)
}

/// Index where the trailing maximal run of ASCII digits begins. Equals
/// `name.len()` when the last byte is not a digit.
fn trailing_digit_run_start(name: &str) -> usize {
    let bytes = name.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    i
}

/// snake_case: RuboCop's `/(?:\D|_\d+|\A\d+)\z/`.
///
/// Valid when the name ends in a non-digit, OR its trailing digit run is
/// immediately preceded by `_`, OR the whole name is digits.
fn is_snake_case(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // `\D\z` — ends in a non-digit byte.
    if !name.as_bytes()[name.len() - 1].is_ascii_digit() {
        return true;
    }
    // `\A\d+\z` — all digits.
    if is_all_digits(name) {
        return true;
    }
    // `_\d+\z` — trailing digit run preceded by `_`.
    let ds = trailing_digit_run_start(name);
    ds > 0 && name.as_bytes()[ds - 1] == b'_'
}

/// normalcase: RuboCop's `/(?:\D|[^_\d]\d+|\A\d+)\z|\A_\d+\z/`.
///
/// Valid when the name ends in a non-digit, OR its trailing digit run is
/// immediately preceded by a byte that is neither `_` nor a digit, OR the whole
/// name is digits, OR it is the implicit-param form `_\d+`.
fn is_normalcase(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if is_implicit_param(name) {
        return true;
    }
    if !name.as_bytes()[name.len() - 1].is_ascii_digit() {
        return true;
    }
    if is_all_digits(name) {
        return true;
    }
    let ds = trailing_digit_run_start(name);
    let prev = name.as_bytes()[ds - 1];
    ds > 0 && prev != b'_' && !prev.is_ascii_digit()
}

/// non_integer: RuboCop's `/(\D|\A\d+)\z|\A_\d+\z/`.
///
/// Valid only when the name ends in a non-digit, OR is all digits, OR is the
/// implicit-param form `_\d+`.
fn is_non_integer(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if is_implicit_param(name) {
        return true;
    }
    if !name.as_bytes()[name.len() - 1].is_ascii_digit() {
        return true;
    }
    is_all_digits(name)
}

#[cfg(test)]
mod tests {
    use super::{Options, VariableNumber, VariableNumberStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(style: VariableNumberStyle) -> Options {
        Options {
            enforced_style: style,
            check_method_names: true,
            check_symbols: true,
            allowed_identifiers: vec![],
            allowed_patterns: vec![],
        }
    }

    // --- normalcase (default); carets from rubocop 1.87.0 col..last_column. ---

    #[test]
    fn normalcase_flags_snake_case_variable() {
        // `x_1 = 1`: snake-style number is invalid in normalcase, col 1..3.
        test::<VariableNumber>().expect_offense(indoc! {r#"
            x_1 = 1
            ^^^ Use normalcase for variable numbers.
        "#});
    }

    #[test]
    fn normalcase_accepts_normal_variable() {
        // `x1 = 1` conforms; `xOne` has no digit.
        test::<VariableNumber>().expect_no_offenses(indoc! {r#"
            x1 = 1
            xOne = 2
        "#});
    }

    // --- snake_case style: ranges + sigils verified against rubocop 1.87.0. ---

    #[test]
    fn snake_flags_normalcase_variable() {
        // `x1 = 1`: col 1..2.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                x1 = 1
                ^^ Use snake_case for variable numbers.
            "#});
    }

    #[test]
    fn snake_flags_instance_variable_with_sigil() {
        // `@foo1 = 1`: caret spans the `@`, col 1..5.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                @foo1 = 1
                ^^^^^ Use snake_case for variable numbers.
            "#});
    }

    #[test]
    fn snake_flags_class_variable_with_sigil() {
        // `@@bar1 = 2`: caret spans the `@@`, col 1..6.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                @@bar1 = 2
                ^^^^^^ Use snake_case for variable numbers.
            "#});
    }

    #[test]
    fn snake_flags_global_variable_with_sigil() {
        // `$baz1 = 3`: caret spans the `$`, col 1..5. Globals ARE checked here.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                $baz1 = 3
                ^^^^^ Use snake_case for variable numbers.
            "#});
    }

    #[test]
    fn snake_flags_positional_argument_only() {
        // Only the required positional `a1` (col 8..9) fires; optarg/restarg/
        // kwarg/kwoptarg/kwrestarg/blockarg are not checked. Method name `m1`
        // (col 5..6) also fires.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                def m1(a1, b2 = 1, *c3, d4:, e5: 1, **f6, &g7)
                    ^^ Use snake_case for method name numbers.
                       ^^ Use snake_case for variable numbers.
                end
            "#});
    }

    #[test]
    fn snake_flags_method_definition_name() {
        // `def method1`: name col 5..11.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                def method1
                    ^^^^^^^ Use snake_case for method name numbers.
                end
            "#});
    }

    #[test]
    fn snake_flags_singleton_method_definition_name() {
        // `def self.smethod1`: name col 10..17.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                def self.smethod1
                         ^^^^^^^^ Use snake_case for method name numbers.
                end
            "#});
    }

    #[test]
    fn snake_flags_standalone_symbol_including_colon() {
        // `:sym1`: range spans the colon, col 7..11.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                sym = :sym1
                      ^^^^^ Use snake_case for symbol numbers.
            "#});
    }

    #[test]
    fn snake_flags_hash_key_label_excluding_colon() {
        // `key1:`: range is the bare `key1` (no colon), col 10..13.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                hash = { key1: 1 }
                         ^^^^ Use snake_case for symbol numbers.
            "#});
    }

    #[test]
    fn snake_flags_symbol_method_argument() {
        // `attr_accessor :foo1`: symbol arg, col 15..19.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                attr_accessor :foo1
                              ^^^^^ Use snake_case for symbol numbers.
            "#});
    }

    // --- non_integer style: any integer-suffixed name is invalid. ---

    #[test]
    fn non_integer_flags_normalcase_variable() {
        // `x1 = 1`: col 1..2 (normalcase-valid but non_integer-invalid).
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::NonInteger))
            .expect_offense(indoc! {r#"
                x1 = 1
                ^^ Use non_integer for variable numbers.
            "#});
    }

    #[test]
    fn non_integer_accepts_non_integer_name() {
        // No integer suffix → valid in every style.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::NonInteger))
            .expect_no_offenses("xOne = 1\n");
    }

    // --- reads are NOT flagged (no on_lvar/on_ivar alias). ---

    #[test]
    fn ignores_variable_read() {
        // `x_1 = 1; x_1` — the read must NOT double-report; only the assign.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::NormalCase))
            .expect_offense(indoc! {r#"
                x_1 = 1
                ^^^ Use normalcase for variable numbers.
                x_1
            "#});
    }

    #[test]
    fn ignores_instance_variable_read() {
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                @foo1 = 1
                ^^^^^ Use snake_case for variable numbers.
                @foo1
            "#});
    }

    // --- method calls are NOT flagged. ---

    #[test]
    fn ignores_method_call() {
        // Only `def`/`defs` names are checked, not `send`/`csend`.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_no_offenses("obj.method1\n");
    }

    // --- CheckMethodNames / CheckSymbols toggles. ---

    #[test]
    fn respects_check_method_names_false() {
        test::<VariableNumber>()
            .with_options(&Options {
                check_method_names: false,
                ..opts(VariableNumberStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                def method1
                end
            "#});
    }

    #[test]
    fn respects_check_symbols_false() {
        test::<VariableNumber>()
            .with_options(&Options {
                check_symbols: false,
                ..opts(VariableNumberStyle::SnakeCase)
            })
            .expect_no_offenses("sym = :sym1\n");
    }

    // --- masgn / op-asgn targets route through `Lvasgn`. ---

    #[test]
    fn snake_flags_multiple_assignment_targets() {
        // `x1, y_1 = 1, 2` — `x1` fires (col 1..2); `y_1` conforms.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                x1, y_1 = 1, 2
                ^^ Use snake_case for variable numbers.
            "#});
    }

    #[test]
    fn snake_flags_op_assignment_target() {
        // `z1 += 1` — op-asgn target, col 1..2.
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_offense(indoc! {r#"
                z1 += 1
                ^^ Use snake_case for variable numbers.
            "#});
    }

    // --- AllowedIdentifiers (default list) / AllowedPatterns. ---

    #[test]
    fn bare_default_allowed_identifiers_skip_offense() {
        // No `with_options`: pins the derive `default = [...]` to default.yml.
        // `x86_64` is normalcase-INVALID (trailing `64` preceded by `_`) but is
        // in the default AllowedIdentifiers list, so the default cop is silent.
        test::<VariableNumber>().expect_no_offenses("x86_64 = 1\n");
    }

    #[test]
    fn default_allowed_identifiers_skip_offense() {
        // `x86_64`, `capture3`, `TLS1_1` are in the default AllowedIdentifiers
        // and must not fire even under snake_case/non_integer.
        test::<VariableNumber>()
            .with_options(&Options {
                allowed_identifiers: vec![
                    "x86_64".to_string(),
                    "capture3".to_string(),
                    "TLS1_1".to_string(),
                ],
                ..opts(VariableNumberStyle::SnakeCase)
            })
            .expect_no_offenses(indoc! {r#"
                x86_64 = 1
                capture3 = 2
                TLS1_1 = 3
            "#});
    }

    #[test]
    fn allowed_identifier_strips_sigil() {
        // RuboCop's `name.to_s.delete("@$")`: `capture3` allowed also excludes
        // `@capture3` (and `$capture3`/`@@capture3`), so no offense even under
        // non_integer. Verified against rubocop 1.87.0.
        test::<VariableNumber>()
            .with_options(&Options {
                allowed_identifiers: vec!["capture3".to_string()],
                ..opts(VariableNumberStyle::NonInteger)
            })
            .expect_no_offenses(indoc! {r#"
                @capture3 = 1
                $capture3 = 2
                @@capture3 = 3
            "#});
    }

    #[test]
    fn allowed_pattern_skips_offense() {
        // snake_case + AllowedPatterns `\Ax\d+\z` allows `x1`.
        test::<VariableNumber>()
            .with_options(&Options {
                allowed_patterns: vec![r"\Ax\d+\z".to_string()],
                ..opts(VariableNumberStyle::SnakeCase)
            })
            .expect_no_offenses("x1 = 1\n");
    }

    // --- conforming names per style (no offenses). ---

    #[test]
    fn accepts_conforming_snake_case_names() {
        test::<VariableNumber>()
            .with_options(&opts(VariableNumberStyle::SnakeCase))
            .expect_no_offenses(indoc! {r#"
                x_1 = 1
                v2_3 = 2
                method_1 = 3
                foo = 4
            "#});
    }

    // --- FORMATS regex unit tests (oracle-cross-checked). ---

    #[test]
    fn format_regex_snake_case() {
        use super::is_snake_case;
        // `x__1` is snake-valid: its trailing `_1` matches `_\d+\z`.
        for n in
            ["x_1", "_1", "_42", "v2_3", "method_1", "valid?", "valid1?", "foo10bar", "123", "x__1"]
        {
            assert!(is_snake_case(n), "{n} should be snake_case-valid");
        }
        for n in ["x1", "foo1", "method1", "a1b2", "sym1", ""] {
            assert!(!is_snake_case(n), "{n} should be snake_case-invalid");
        }
    }

    #[test]
    fn format_regex_normalcase() {
        use super::is_normalcase;
        for n in ["x1", "foo1", "_1", "_42", "method1", "valid?", "valid1?", "foo10bar", "123", "a1b2", "sym1"]
        {
            assert!(is_normalcase(n), "{n} should be normalcase-valid");
        }
        for n in ["x_1", "v2_3", "method_1", "x__1", ""] {
            assert!(!is_normalcase(n), "{n} should be normalcase-invalid");
        }
    }

    #[test]
    fn format_regex_non_integer() {
        use super::is_non_integer;
        for n in ["_1", "_42", "valid?", "valid1?", "foo10bar", "123"] {
            assert!(is_non_integer(n), "{n} should be non_integer-valid");
        }
        for n in ["x1", "x_1", "foo1", "method1", "a1b2", "sym1", ""] {
            assert!(!is_non_integer(n), "{n} should be non_integer-invalid");
        }
    }
}
murphy_plugin_api::submit_cop!(VariableNumber);
