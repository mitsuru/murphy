//! `Naming/PredicatePrefix` — flag method definitions (and dynamically defined
//! methods) whose name begins with a predicate prefix (`is_`, `has_`, `have_`,
//! `does_`) and suggest renaming.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/PredicatePrefix
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-f0xe]
//! notes: >
//!   Mirrors RuboCop's `on_def`/`on_defs` (aliased) and `on_send` (dynamic
//!   method-define macros) exactly for the default config (and any config whose
//!   `NamePrefix` entries are mutually exclusive, which the default is).
//!
//!   Offense predicate, per prefix in `NamePrefix`: emit unless ANY of RuboCop's
//!   `allowed_method_name?` disjuncts hold:
//!     * name does not start with the prefix, OR the char immediately after the
//!       prefix is a digit (`/^prefix[^0-9]/` — `is_1?` and bare `is_` are
//!       allowed);
//!     * name already equals `expected_name` (only reachable when the prefix is
//!       NOT in `ForbiddenPrefixes`, e.g. `ForbiddenPrefixes: []` keeps
//!       `is_even?`; with the default config every prefix is forbidden so this
//!       branch never trips);
//!     * name ends with `=` (setter);
//!     * name is in `AllowedMethods` (exact match incl. `?` — `is_a?` is
//!       allowed, `is_a` is not).
//!   `expected_name`: strip the prefix (first occurrence) iff it is in
//!   `ForbiddenPrefixes`, then append `?` unless the original already ends `?`.
//!   Message: ``Rename `{name}` to `{expected}`.``
//!
//!   Def offense range is the method-name token (`node.loc.name`); for
//!   `def self.is_even` the caret lands on `is_even` after `self.`. Dynamic
//!   defines fire only for a nil-receiver send whose selector is a configured
//!   `MethodDefinitionMacros` entry and whose first argument is a symbol; the
//!   offense covers the whole symbol literal incl. the leading colon
//!   (`node.first_argument`), matching RuboCop's `c15..22` for
//!   `define_method(:is_even)`.
//!
//!   Verified against rubocop 1.87.0: `is_even`→`even?`, `has_value?`→`value?`,
//!   `have_value`→`value?`, `does_thing`→`thing?`, `is_even?`→`even?`,
//!   `is_a`→`a?`, `is_b`→`b?`, `def self.is_even`→`even?`,
//!   `define_method(:is_even)`/`define_singleton_method(:has_thing)` fire, and
//!   `is_a?` (AllowedMethods), `is_1?` (digit), `is_=` (setter), `is_` (no
//!   trailing char), `isolate` (no prefix), `def_node_matcher(:is_odd)` (macro
//!   not in default list), and `even?` all produce no offense.
//!
//!   Report-only, matching RuboCop (no autocorrect).
//!
//!   DIVERGENCE (rare custom config): RuboCop iterates every `NamePrefix` entry
//!   and emits one offense per non-allowed prefix, so OVERLAPPING prefixes can
//!   yield multiple offenses on the same name (e.g. `NamePrefix: [is_, is_a_]`,
//!   `is_a_thing` → two offenses, `a_thing?` and `thing?`). Murphy stops at the
//!   first non-allowed prefix and emits one offense. This is unreachable with
//!   the default config (prefixes are mutually exclusive) and any non-overlapping
//!   `NamePrefix`; verified against rubocop 1.87.0.
//!
//!   GAP (murphy-f0xe): `UseSorbetSigs` is exposed (default false, matching
//!   default.yml) but is a no-op. With `UseSorbetSigs: true` RuboCop reports a
//!   def offense only when the def has a preceding sibling
//!   `sig { returns(T::Boolean) }`; Murphy ignores the flag and would
//!   over-report. Default-config parity is unaffected since the default is
//!   false. RuboCop's `validate_config` (raising when a `ForbiddenPrefixes`
//!   entry is missing from `NamePrefix`) is not reproduced — it is a config
//!   sanity check, not a behavioral difference on valid config.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct PredicatePrefix;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "NamePrefix",
        default = ["is_", "has_", "have_", "does_"],
        description = "Predicate name prefixes."
    )]
    pub name_prefix: Vec<String>,

    #[option(
        name = "ForbiddenPrefixes",
        default = ["is_", "has_", "have_", "does_"],
        description = "Predicate name prefixes that should be removed."
    )]
    pub forbidden_prefixes: Vec<String>,

    #[option(
        name = "AllowedMethods",
        default = ["is_a?"],
        description = "Predicate names which, despite having a forbidden prefix or no `?`, should still be accepted."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "MethodDefinitionMacros",
        default = ["define_method", "define_singleton_method"],
        description = "Method definition macros for dynamically generated methods."
    )]
    pub method_definition_macros: Vec<String>,

    #[option(
        name = "UseSorbetSigs",
        default = false,
        description = "Use Sorbet's T::Boolean return type to detect predicate methods."
    )]
    pub use_sorbet_sigs: bool,
}

#[cop(
    name = "Naming/PredicatePrefix",
    description = "Predicate method names should not be prefixed and end with a `?`.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl PredicatePrefix {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        // `descendants` excludes the root node itself; chain it so a single
        // top-level `def` or `define_method(...)` (whose root *is* that node)
        // is also inspected.
        for id in cx
            .descendants(cx.root())
            .into_iter()
            .chain(std::iter::once(cx.root()))
        {
            match *cx.kind(id) {
                NodeKind::Def { .. } | NodeKind::Defs { .. } => self.check_def(id, &opts, cx),
                // Match `Send` specifically (not the wrapping `Block`) so
                // `define_method(:is_even) { }` emits once, on the send.
                NodeKind::Send { .. } => self.check_send(id, &opts, cx),
                _ => {}
            }
        }
    }
}

impl PredicatePrefix {
    fn check_def(&self, id: NodeId, opts: &Options, cx: &Cx<'_>) {
        let Some(name) = cx.method_name(id) else {
            return;
        };
        // GAP (murphy-f0xe): `UseSorbetSigs: true` should additionally require a
        // preceding `sig { returns(T::Boolean) }`. Not implemented; the flag is
        // a no-op so default-config (false) parity is preserved.
        if let Some(expected) = offense_rename(name, opts) {
            cx.emit_offense(def_name_range(id, name, cx), &rename_message(name, &expected), None);
        }
    }

    fn check_send(&self, id: NodeId, opts: &Options, cx: &Cx<'_>) {
        // RuboCop's `dynamic_method_define`: nil receiver, selector in
        // `MethodDefinitionMacros`, first argument is a symbol literal.
        if cx.call_receiver(id).get().is_some() {
            return;
        }
        let Some(method) = cx.method_name(id) else {
            return;
        };
        if !opts.method_definition_macros.iter().any(|m| m == method) {
            return;
        }
        let Some(&first_arg) = cx.call_arguments(id).first() else {
            return;
        };
        let NodeKind::Sym(sym) = *cx.kind(first_arg) else {
            return;
        };
        let name = cx.symbol_str(sym);
        if let Some(expected) = offense_rename(name, opts) {
            // `node.first_argument` — the whole symbol literal incl. the colon.
            cx.emit_offense(cx.range(first_arg), &rename_message(name, &expected), None);
        }
    }
}

/// Returns the expected (renamed) method name if `name` offends under any prefix
/// in `NamePrefix`, or `None` if every prefix is allowed for this name.
///
/// Short-circuits on the first non-allowed prefix. With the default config (and
/// any non-overlapping `NamePrefix`) the prefixes are mutually exclusive, so this
/// matches RuboCop's per-prefix loop exactly. For overlapping custom prefixes
/// RuboCop would emit one offense per matching prefix; see the DIVERGENCE note.
fn offense_rename(name: &str, opts: &Options) -> Option<String> {
    for prefix in &opts.name_prefix {
        if allowed_method_name(name, prefix, opts) {
            continue;
        }
        return Some(expected_name(name, prefix, opts));
    }
    None
}

/// RuboCop's `allowed_method_name?`: true means "do not flag for this prefix".
fn allowed_method_name(name: &str, prefix: &str, opts: &Options) -> bool {
    // `!(starts_with(prefix) && /^prefix[^0-9]/)`: not a predicate-prefixed name
    // when it doesn't start with the prefix, or the char right after the prefix
    // is a digit (or absent, e.g. bare `is_`).
    let has_predicate_prefix = name
        .strip_prefix(prefix)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| !c.is_ascii_digit());

    !has_predicate_prefix
        || name == expected_name(name, prefix, opts)
        || name.ends_with('=')
        || opts.allowed_methods.iter().any(|m| m == name)
}

/// RuboCop's `expected_name`: strip the prefix (first occurrence) iff it is in
/// `ForbiddenPrefixes`, then append `?` unless the original already ends `?`.
fn expected_name(name: &str, prefix: &str, opts: &Options) -> String {
    let mut new_name = if opts.forbidden_prefixes.iter().any(|p| p == prefix) {
        // `String#sub` replaces the FIRST occurrence; since `prefix` is a known
        // prefix here, `strip_prefix` is equivalent and allocation-light.
        name.strip_prefix(prefix).unwrap_or(name).to_owned()
    } else {
        name.to_owned()
    };
    if !name.ends_with('?') {
        new_name.push('?');
    }
    new_name
}

fn rename_message(name: &str, expected: &str) -> String {
    format!("Rename `{name}` to `{expected}`.")
}

/// Byte range of the method name within a `def`/`defs` definition, mirroring
/// RuboCop's `node.loc.name`.
///
/// Murphy leaves `loc.name` as `Range::ZERO` for `Def`/`Defs`, so the name is
/// located by its first occurrence in the node's source, starting past any
/// singleton receiver (`def self.x` / `def Foo.x`) so a receiver whose source
/// contains the name as a substring cannot mis-anchor the caret. Beyond the
/// receiver the name always precedes the argument list and body. Falls back to a
/// single-byte caret at the node start if the name is somehow not found.
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

#[cfg(test)]
mod tests {
    use super::{Options, PredicatePrefix};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses (ground-truth carets/messages from rubocop 1.87.0) ---

    #[test]
    fn flags_is_prefix_without_question_mark() {
        // rubocop: L1 c5..11 `is_even` -> `even?`.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def is_even(value)
                ^^^^^^^ Rename `is_even` to `even?`.
            end
        "#});
    }

    #[test]
    fn flags_has_prefix_with_question_mark() {
        // `has_value?` -> `value?` (already ends `?`, strip prefix only).
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def has_value?
                ^^^^^^^^^^ Rename `has_value?` to `value?`.
            end
        "#});
    }

    #[test]
    fn flags_have_prefix() {
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def have_value
                ^^^^^^^^^^ Rename `have_value` to `value?`.
            end
        "#});
    }

    #[test]
    fn flags_does_prefix() {
        // `does_` is in the default NamePrefix/ForbiddenPrefixes.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def does_thing
                ^^^^^^^^^^ Rename `does_thing` to `thing?`.
            end
        "#});
    }

    #[test]
    fn flags_is_prefix_with_question_mark() {
        // `is_even?` -> `even?` (strip prefix, keep the `?`).
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def is_even?(value)
                ^^^^^^^^ Rename `is_even?` to `even?`.
            end
        "#});
    }

    #[test]
    fn flags_single_char_after_prefix() {
        // `is_b` -> `b?`.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def is_b
                ^^^^ Rename `is_b` to `b?`.
            end
        "#});
    }

    #[test]
    fn flags_is_a_without_question_mark() {
        // `is_a` -> `a?`; only `is_a?` (with `?`) is in AllowedMethods.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def is_a(x)
                ^^^^ Rename `is_a` to `a?`.
            end
        "#});
    }

    #[test]
    fn flags_singleton_def() {
        // `def self.is_even`: name `is_even` at col 10..16 (after `self.`).
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            def self.is_even(value)
                     ^^^^^^^ Rename `is_even` to `even?`.
            end
        "#});
    }

    #[test]
    fn flags_define_method_macro() {
        // `define_method(:is_even)`: offense covers `:is_even` incl. the colon,
        // rubocop c15..22.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            define_method(:is_even) { |value| }
                          ^^^^^^^^ Rename `is_even` to `even?`.
        "#});
    }

    #[test]
    fn flags_define_singleton_method_macro() {
        // `define_singleton_method(:has_thing)` -> `thing?`, c25..34.
        test::<PredicatePrefix>().expect_offense(indoc! {r#"
            define_singleton_method(:has_thing) { }
                                    ^^^^^^^^^^ Rename `has_thing` to `thing?`.
        "#});
    }

    // --- non-offenses (verified against rubocop: NOT flagged) ---

    #[test]
    fn ignores_allowed_method() {
        // `is_a?` is in the default AllowedMethods.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def is_a?(x)
            end
        "#});
    }

    #[test]
    fn ignores_digit_after_prefix() {
        // `is_1?`: char after `is_` is a digit -> not predicate-prefixed.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def is_1?
            end
        "#});
    }

    #[test]
    fn ignores_setter_suffix() {
        // `is_=`: ends with `=`.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def is_=(x)
            end
        "#});
    }

    #[test]
    fn ignores_bare_prefix() {
        // `is_`: no char after the prefix -> regex `/^is_[^0-9]/` fails.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def is_(x)
            end
        "#});
    }

    #[test]
    fn ignores_name_not_starting_with_prefix() {
        // `isolate` starts with `iso`, not `is_`.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def isolate
            end
        "#});
    }

    #[test]
    fn ignores_proper_predicate() {
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def even?
            end
        "#});
    }

    #[test]
    fn ignores_non_default_macro() {
        // `def_node_matcher` is not in the default MethodDefinitionMacros.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            def_node_matcher(:is_odd) { |value| }
        "#});
    }

    #[test]
    fn ignores_macro_with_receiver() {
        // A non-nil receiver disqualifies the dynamic-define send.
        test::<PredicatePrefix>().expect_no_offenses(indoc! {r#"
            obj.define_method(:is_even) { |value| }
        "#});
    }

    // --- config: ForbiddenPrefixes (the `name == expected_name` branch) ---

    #[test]
    fn forbidden_prefixes_empty_keeps_question_mark_form() {
        // With ForbiddenPrefixes: [], `is_even?` already equals expected_name
        // (`is_even?`) -> allowed. `is_even` (no `?`) -> `is_even?` (prefix
        // retained, `?` appended).
        let opts = Options {
            name_prefix: vec!["is_".to_owned(), "has_".to_owned(), "have_".to_owned()],
            forbidden_prefixes: vec![],
            allowed_methods: vec!["is_a?".to_owned()],
            method_definition_macros: vec![
                "define_method".to_owned(),
                "define_singleton_method".to_owned(),
            ],
            use_sorbet_sigs: false,
        };
        test::<PredicatePrefix>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def is_even?(value)
                end
            "#});
        test::<PredicatePrefix>().with_options(&opts).expect_offense(indoc! {r#"
            def is_even(value)
                ^^^^^^^ Rename `is_even` to `is_even?`.
            end
        "#});
    }

    // --- config: custom NamePrefix ---

    #[test]
    fn custom_name_prefix() {
        let opts = Options {
            name_prefix: vec!["seems_to_be_".to_owned()],
            forbidden_prefixes: vec!["seems_to_be_".to_owned()],
            allowed_methods: vec![],
            method_definition_macros: vec![
                "define_method".to_owned(),
                "define_singleton_method".to_owned(),
            ],
            use_sorbet_sigs: false,
        };
        test::<PredicatePrefix>().with_options(&opts).expect_offense(indoc! {r#"
            def seems_to_be_even(value)
                ^^^^^^^^^^^^^^^^ Rename `seems_to_be_even` to `even?`.
            end
        "#});
        // The default prefixes no longer apply.
        test::<PredicatePrefix>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def is_even(value)
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(PredicatePrefix);
