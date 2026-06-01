//! `Style/SpecialGlobalVars` — avoid Perl-style global variables.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SpecialGlobalVars
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Three EnforcedStyle values are implemented:
//!     use_english_names (default): flag Perl-style globals, prefer English/stdlib names.
//!     use_perl_names: flag English names, prefer Perl-style globals.
//!     use_builtin_english_names: allow built-in English names and their Perl equivalents.
//!   Message format parity: MSG_ENGLISH, MSG_REGULAR, MSG_BOTH for use_english_names style;
//!   MSG_REGULAR for use_perl_names and use_builtin_english_names.
//!   Autocorrect: replaces the gvar with the first preferred name (simple case).
//!   Gaps vs RuboCop:
//!     - ARGV constant: RuboCop flags the ARGV constant node (not a $-prefixed gvar)
//!       under use_perl_names and use_builtin_english_names. Murphy only handles gvar
//!       nodes; ARGV (a Ruby constant) is not flagged.
//!     - In-string/in-regexp interpolation context: autocorrect wraps English names
//!       in #{} when inside dstr/xstr/regexp — not implemented (requires parent-context
//!       ABI that is not exposed).
//!     - RequireEnglish: auto-insert `require 'English'` at file top — not implemented.
//!     - style_detected / correct_style_detected dual-file tracking — not implemented
//!       (Murphy does not support multi-file style inference).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const MSG_ENGLISH: &str = "Prefer `{prefer}` from the stdlib 'English' module (don't forget to require it) over `{global}`.";
const MSG_REGULAR: &str = "Prefer `{prefer}` over `{global}`.";
const MSG_BOTH: &str = "Prefer `{prefer}` from the stdlib 'English' module (don't forget to require it) or `{regular}` over `{global}`.";

/// Perl-style global var → list of English/preferred names.
/// Order matters: first entry is the "primary" preferred name used in MSG_REGULAR/MSG_BOTH.
const ENGLISH_VARS: &[(&str, &[&str])] = &[
    ("$:", &["$LOAD_PATH"]),
    ("$\"", &["$LOADED_FEATURES"]),
    ("$0", &["$PROGRAM_NAME"]),
    ("$!", &["$ERROR_INFO"]),
    ("$@", &["$ERROR_POSITION"]),
    ("$;", &["$FIELD_SEPARATOR", "$FS"]),
    ("$,", &["$OUTPUT_FIELD_SEPARATOR", "$OFS"]),
    ("$/", &["$INPUT_RECORD_SEPARATOR", "$RS"]),
    ("$\\", &["$OUTPUT_RECORD_SEPARATOR", "$ORS"]),
    ("$.", &["$INPUT_LINE_NUMBER", "$NR"]),
    ("$_", &["$LAST_READ_LINE"]),
    ("$>", &["$DEFAULT_OUTPUT"]),
    ("$<", &["$DEFAULT_INPUT"]),
    ("$$", &["$PROCESS_ID", "$PID"]),
    ("$?", &["$CHILD_STATUS"]),
    ("$~", &["$LAST_MATCH_INFO"]),
    ("$=", &["$IGNORECASE"]),
    ("$*", &["$ARGV", "ARGV"]),
];

/// English names that are built-in (no `require 'English'` needed).
const NON_ENGLISH_VARS: &[&str] = &["$LOAD_PATH", "$LOADED_FEATURES", "$PROGRAM_NAME", "ARGV"];

/// Stateless unit struct.
#[derive(Default)]
pub struct SpecialGlobalVars;

/// Enforcement style for special global vars.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "use_english_names")]
    English,
    #[option(value = "use_perl_names")]
    Perl,
    #[option(value = "use_builtin_english_names")]
    BuiltinEnglish,
}

/// Cop options for SpecialGlobalVars.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "use_english_names",
        description = "Which style of global variable names to prefer."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/SpecialGlobalVars",
    description = "Avoid Perl-style global variables.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl SpecialGlobalVars {
    #[on_node(kind = "gvar")]
    fn check_gvar(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Gvar(sym) = *cx.kind(node) else {
        return;
    };
    let var_name = cx.symbol_str(sym);
    let opts = cx.options_or_default::<Options>();

    match opts.enforced_style {
        EnforcedStyle::English => check_use_english(node, var_name, cx),
        EnforcedStyle::Perl => check_use_perl(node, var_name, cx),
        EnforcedStyle::BuiltinEnglish => check_use_builtin(node, var_name, cx),
    }
}

/// `use_english_names` style: flag Perl-style vars, prefer English names.
/// Also flags English names that have better canonical forms (self-identity check).
fn check_use_english(node: NodeId, var_name: &str, cx: &Cx<'_>) {
    // Check if this var is a Perl-style var that has English alternatives.
    if let Some(english_names) = perl_to_english(var_name) {
        // Split into: those needing English library vs built-in ones.
        let (regular, english): (Vec<&str>, Vec<&str>) = english_names.iter().partition(|v| {
            NON_ENGLISH_VARS.contains(v)
        });
        let msg = format_english_message(var_name, &english, &regular);
        cx.emit_offense(cx.range(node), &msg, None);
        // Autocorrect: replace with the first preferred name.
        // Note: in-string interpolation context is not handled (gap).
        let replacement = if !english.is_empty() {
            english[0]
        } else {
            regular[0]
        };
        cx.emit_edit(cx.range(node), replacement);
    }

    // Check if this is an English-library name that is preferred as-is.
    // If var_name is itself in any ENGLISH_VARS value list, it's already the right name.
    // If it's a non-English builtin ($LOAD_PATH etc.), also correct.
    // No offense needed for already-correct names.
}

/// `use_perl_names` style: flag English names, prefer Perl vars.
fn check_use_perl(node: NodeId, var_name: &str, cx: &Cx<'_>) {
    // Check if this is an English name that has a Perl equivalent.
    if let Some(perl_vars) = english_to_perl(var_name) {
        let msg = format_regular_message(var_name, perl_vars[0]);
        cx.emit_offense(cx.range(node), &msg, None);
        cx.emit_edit(cx.range(node), perl_vars[0]);
    }
}

/// `use_builtin_english_names` style: mirrors RuboCop's `BUILTIN_VARS`.
///
/// BUILTIN_VARS is PERL_VARS with overrides for the three non-ARGV NON_ENGLISH_VARS:
///   $: → $LOAD_PATH   (Perl var → its NON_ENGLISH builtin, not → itself)
///   $" → $LOADED_FEATURES
///   $0 → $PROGRAM_NAME
/// All other entries: Perl vars map to themselves (preferred as-is),
/// English names map to their Perl var.
/// NON_ENGLISH builtins ($LOAD_PATH, $LOADED_FEATURES, $PROGRAM_NAME) map to themselves.
/// ARGV → $* (from PERL_VARS, since ARGV doesn't start with '$' and has no NON_ENGLISH override).
fn check_use_builtin(node: NodeId, var_name: &str, cx: &Cx<'_>) {
    if let Some(preferred) = builtin_vars_preferred(var_name) {
        if preferred == var_name {
            return; // Already in preferred form.
        }
        let msg = format_regular_message(var_name, preferred);
        cx.emit_offense(cx.range(node), &msg, None);
        cx.emit_edit(cx.range(node), preferred);
    }
}

/// Returns the preferred form of `var` under `use_builtin_english_names`, or `None`
/// if the var is not in the known set (no offense for unknown vars).
fn builtin_vars_preferred(var: &str) -> Option<&'static str> {
    // For each ENGLISH_VARS entry (Perl var → English names):
    // - Check if var is the Perl key: preferred is the NON_ENGLISH override if one exists,
    //   otherwise the Perl key itself (Perl vars are OK in builtin mode).
    // - Check if var is one of the English values: preferred is the Perl key.
    // Special case: NON_ENGLISH_VARS map to themselves.

    // Check NON_ENGLISH_VARS directly: these map to themselves.
    if let Some(&builtin) = NON_ENGLISH_VARS.iter().find(|&&v| v == var) {
        return Some(builtin);
    }

    for (perl, english_names) in ENGLISH_VARS {
        if *perl == var {
            // var is a Perl-style var.
            // Check if this Perl var has a NON_ENGLISH builtin equivalent.
            // (Only $:/$"/$0 have this; $* does not since ARGV is not $-prefixed.)
            let non_english: Option<&&str> =
                english_names.iter().find(|v| NON_ENGLISH_VARS.contains(v) && v.starts_with('$'));
            if let Some(ne) = non_english {
                return Some(ne);
            }
            // Perl var is preferred as-is (Perl-style is fine in this mode).
            return Some(perl);
        }

        if english_names.contains(&var) {
            // var is an English-library name or ARGV.
            // NON_ENGLISH_VARS was already handled above.
            // Prefer the Perl var for English-library names.
            return Some(perl);
        }
    }

    None
}

/// Given a Perl var name (e.g. `$;`), return the English names (e.g. `["$FIELD_SEPARATOR", "$FS"]`).
fn perl_to_english(var_name: &str) -> Option<&'static [&'static str]> {
    ENGLISH_VARS.iter().find(|(p, _)| *p == var_name).map(|(_, e)| *e)
}

/// Given an English name (e.g. `$FIELD_SEPARATOR`), return the Perl vars.
fn english_to_perl(var_name: &str) -> Option<&'static [&'static str]> {
    // Build PERL_VARS logic: for each ENGLISH_VARS entry, the English names
    // map back to the Perl var.
    for (perl, english_names) in ENGLISH_VARS {
        if english_names.contains(&var_name) {
            // Return a slice containing just the perl var.
            // We return the whole ENGLISH_VARS tuple's perl part as a single-element.
            return Some(std::slice::from_ref(perl));
        }
    }
    None
}

fn format_english_message(global: &str, english: &[&str], regular: &[&str]) -> String {
    if regular.is_empty() {
        // Only English-library names.
        MSG_ENGLISH
            .replace("{prefer}", &format_list(english))
            .replace("{global}", global)
    } else if english.is_empty() {
        // Only non-library (builtin) names.
        MSG_REGULAR
            .replace("{prefer}", &format_list(regular))
            .replace("{global}", global)
    } else {
        // Both kinds.
        MSG_BOTH
            .replace("{prefer}", &format_list(english))
            .replace("{regular}", &format_list(regular))
            .replace("{global}", global)
    }
}

fn format_regular_message(global: &str, prefer: &str) -> String {
    MSG_REGULAR
        .replace("{prefer}", prefer)
        .replace("{global}", global)
}

fn format_list(items: &[&str]) -> String {
    items.join("` or `")
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- use_english_names (default) ---

    #[test]
    fn flags_perl_var_field_separator() {
        test::<SpecialGlobalVars>().expect_offense(indoc! {"
            $;
            ^^ Prefer `$FIELD_SEPARATOR` or `$FS` from the stdlib 'English' module (don't forget to require it) over `$;`.
        "});
    }

    #[test]
    fn corrects_perl_var_to_english() {
        test::<SpecialGlobalVars>().expect_correction(
            indoc! {"
                $;
                ^^ Prefer `$FIELD_SEPARATOR` or `$FS` from the stdlib 'English' module (don't forget to require it) over `$;`.
            "},
            "$FIELD_SEPARATOR\n",
        );
    }

    #[test]
    fn flags_dollar_slash() {
        test::<SpecialGlobalVars>().expect_offense(indoc! {"
            $/
            ^^ Prefer `$INPUT_RECORD_SEPARATOR` or `$RS` from the stdlib 'English' module (don't forget to require it) over `$/`.
        "});
    }

    #[test]
    fn flags_dollar_bang_with_english_msg() {
        // $! has only $ERROR_INFO — no NON_ENGLISH counterpart, so MSG_ENGLISH.
        test::<SpecialGlobalVars>().expect_offense(indoc! {"
            $!
            ^^ Prefer `$ERROR_INFO` from the stdlib 'English' module (don't forget to require it) over `$!`.
        "});
    }

    #[test]
    fn flags_load_path_perl_var() {
        // $: → $LOAD_PATH (which is a NON_ENGLISH_VAR = builtin, so MSG_REGULAR)
        test::<SpecialGlobalVars>().expect_offense(indoc! {"
            $:
            ^^ Prefer `$LOAD_PATH` over `$:`.
        "});
    }

    #[test]
    fn corrects_load_path_perl_var() {
        test::<SpecialGlobalVars>().expect_correction(
            indoc! {"
                $:
                ^^ Prefer `$LOAD_PATH` over `$:`.
            "},
            "$LOAD_PATH\n",
        );
    }

    #[test]
    fn accepts_english_var() {
        test::<SpecialGlobalVars>().expect_no_offenses("$FIELD_SEPARATOR\n");
    }

    #[test]
    fn accepts_load_path_english() {
        test::<SpecialGlobalVars>().expect_no_offenses("$LOAD_PATH\n");
    }

    #[test]
    fn flags_dollar_star() {
        // $* → $ARGV or ARGV — ARGV is NON_ENGLISH, $ARGV is English
        test::<SpecialGlobalVars>().expect_offense(indoc! {"
            $*
            ^^ Prefer `$ARGV` from the stdlib 'English' module (don't forget to require it) or `ARGV` over `$*`.
        "});
    }

    // --- use_perl_names style ---

    fn perl_opts() -> Options {
        Options { enforced_style: EnforcedStyle::Perl }
    }

    #[test]
    fn flags_english_var_in_perl_mode() {
        test::<SpecialGlobalVars>()
            .with_options(&perl_opts())
            .expect_offense(indoc! {"
                $FIELD_SEPARATOR
                ^^^^^^^^^^^^^^^^ Prefer `$;` over `$FIELD_SEPARATOR`.
            "});
    }

    #[test]
    fn corrects_english_to_perl() {
        test::<SpecialGlobalVars>()
            .with_options(&perl_opts())
            .expect_correction(
                indoc! {"
                    $FIELD_SEPARATOR
                    ^^^^^^^^^^^^^^^^ Prefer `$;` over `$FIELD_SEPARATOR`.
                "},
                "$;\n",
            );
    }

    #[test]
    fn accepts_perl_var_in_perl_mode() {
        test::<SpecialGlobalVars>()
            .with_options(&perl_opts())
            .expect_no_offenses("$;\n");
    }

    // --- use_builtin_english_names style ---

    fn builtin_opts() -> Options {
        Options { enforced_style: EnforcedStyle::BuiltinEnglish }
    }

    #[test]
    fn accepts_perl_var_in_builtin_mode() {
        test::<SpecialGlobalVars>()
            .with_options(&builtin_opts())
            .expect_no_offenses("$;\n");
    }

    #[test]
    fn accepts_load_path_in_builtin_mode() {
        test::<SpecialGlobalVars>()
            .with_options(&builtin_opts())
            .expect_no_offenses("$LOAD_PATH\n");
    }

    #[test]
    fn flags_english_name_in_builtin_mode() {
        // $FIELD_SEPARATOR is not a builtin — should prefer $; in builtin mode
        test::<SpecialGlobalVars>()
            .with_options(&builtin_opts())
            .expect_offense(indoc! {"
                $FIELD_SEPARATOR
                ^^^^^^^^^^^^^^^^ Prefer `$;` over `$FIELD_SEPARATOR`.
            "});
    }

    #[test]
    fn flags_perl_load_path_in_builtin_mode() {
        // $: is the Perl var for $LOAD_PATH; BUILTIN_VARS[$:] = [$LOAD_PATH]
        // so the preferred form is $LOAD_PATH (a NON_ENGLISH_VAR builtin).
        test::<SpecialGlobalVars>()
            .with_options(&builtin_opts())
            .expect_offense(indoc! {"
                $:
                ^^ Prefer `$LOAD_PATH` over `$:`.
            "});
    }

    // Note: ARGV is a Ruby constant (const node), not a gvar — this cop only
    // handles $-prefixed global variables. ARGV flagging is a gap vs RuboCop.
    #[test]
    fn accepts_argv_constant_in_builtin_mode() {
        // ARGV is a const node, not a gvar — this cop cannot flag it.
        test::<SpecialGlobalVars>()
            .with_options(&builtin_opts())
            .expect_no_offenses("ARGV
");
    }
}
murphy_plugin_api::submit_cop!(SpecialGlobalVars);
