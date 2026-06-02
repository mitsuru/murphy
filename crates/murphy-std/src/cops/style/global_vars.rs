//! `Style/GlobalVars` — do not introduce global variables.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/GlobalVars
//! upstream_version_checked: 1.86.2
//! version_added: "0.13"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Handles gvar (read) and gvasgn (write/assignment) nodes.
//!   Built-in global variables are allowed by default.
//!   Users can allow additional variables via AllowedVariables.
//!   Backreferences like $1, $2 are parsed as nth_ref nodes (not gvar/gvasgn)
//!   and are therefore not flagged.
//!   Offense range covers only the variable name (cx.range(node) for gvar
//!   which coincides with the name; for gvasgn a name range is computed
//!   from node start + symbol byte length since loc.name is Range::ZERO).
//!   No autocorrect — matches RuboCop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! $foo = 2
//! bar = $foo + 5
//!
//! # good
//! FOO = 2
//! foo = 2
//! $stdin.read         # built-in
//! $1                  # backreference (nth_ref node, not gvar)
//! ```
//!
//! ## No autocorrect
//!
//! There is no safe general replacement for a user-defined global variable.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Built-in global variables that are always allowed.
const BUILT_IN_VARS: &[&str] = &[
    "$:",
    "$LOAD_PATH",
    "$\"",
    "$LOADED_FEATURES",
    "$0",
    "$PROGRAM_NAME",
    "$!",
    "$ERROR_INFO",
    "$@",
    "$ERROR_POSITION",
    "$;",
    "$FS",
    "$FIELD_SEPARATOR",
    "$,",
    "$OFS",
    "$OUTPUT_FIELD_SEPARATOR",
    "$/",
    "$RS",
    "$INPUT_RECORD_SEPARATOR",
    "$\\",
    "$ORS",
    "$OUTPUT_RECORD_SEPARATOR",
    "$.",
    "$NR",
    "$INPUT_LINE_NUMBER",
    "$_",
    "$LAST_READ_LINE",
    "$>",
    "$DEFAULT_OUTPUT",
    "$<",
    "$DEFAULT_INPUT",
    "$$",
    "$PID",
    "$PROCESS_ID",
    "$?",
    "$CHILD_STATUS",
    "$~",
    "$LAST_MATCH_INFO",
    "$=",
    "$IGNORECASE",
    "$*",
    "$ARGV",
    "$&",
    "$MATCH",
    "$`",
    "$PREMATCH",
    "$'",
    "$POSTMATCH",
    "$+",
    "$LAST_PAREN_MATCH",
    "$stdin",
    "$stdout",
    "$stderr",
    "$DEBUG",
    "$FILENAME",
    "$VERBOSE",
    "$SAFE",
    "$-0",
    "$-a",
    "$-d",
    "$-F",
    "$-i",
    "$-I",
    "$-l",
    "$-p",
    "$-v",
    "$-w",
    "$CLASSPATH",
    "$JRUBY_VERSION",
    "$JRUBY_REVISION",
    "$ENV_JAVA",
];

/// Cop options for GlobalVars.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedVariables",
        default = [],
        description = "Additional global variables to allow (e.g. `$allowed`)."
    )]
    pub allowed_variables: Vec<String>,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct GlobalVars;

#[cop(
    name = "Style/GlobalVars",
    description = "Do not introduce global variables.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl GlobalVars {
    #[on_node(kind = "gvar")]
    fn check_gvar(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvar(sym) = *cx.kind(node) else {
            return;
        };
        let var_name = cx.symbol_str(sym);
        if is_allowed(var_name, cx) {
            return;
        }
        // For gvar, cx.range(node) covers exactly the variable name.
        cx.emit_offense(cx.range(node), "Do not introduce global variables.", None);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvasgn { name, .. } = *cx.kind(node) else {
            return;
        };
        let var_name = cx.symbol_str(name);
        if is_allowed(var_name, cx) {
            return;
        }
        // For gvasgn, loc.name is Range::ZERO (push() sets no name range).
        // Compute the variable name range from the node start and symbol length.
        let node_start = cx.range(node).start;
        let name_range = Range {
            start: node_start,
            end: node_start + var_name.len() as u32,
        };
        cx.emit_offense(name_range, "Do not introduce global variables.", None);
    }
}

fn is_allowed(var_name: &str, cx: &Cx<'_>) -> bool {
    if BUILT_IN_VARS.contains(&var_name) {
        return true;
    }
    let opts = cx.options_or_default::<Options>();
    opts.allowed_variables.iter().any(|v| v == var_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense cases ---

    #[test]
    fn flags_custom_gvar_read() {
        test::<GlobalVars>().expect_offense(indoc! {"
            puts $foo
                 ^^^^ Do not introduce global variables.
        "});
    }

    #[test]
    fn flags_custom_gvar_assignment() {
        test::<GlobalVars>().expect_offense(indoc! {"
            $foo = 2
            ^^^^ Do not introduce global variables.
        "});
    }

    #[test]
    fn flags_gvar_in_expression() {
        test::<GlobalVars>().expect_offense(indoc! {"
            bar = $foo + 5
                  ^^^^ Do not introduce global variables.
        "});
    }

    // --- allowed built-in variables ---

    #[test]
    fn accepts_stdin() {
        test::<GlobalVars>().expect_no_offenses("$stdin.read\n");
    }

    #[test]
    fn accepts_stdout() {
        test::<GlobalVars>().expect_no_offenses("$stdout.puts('hello')\n");
    }

    #[test]
    fn accepts_stderr() {
        test::<GlobalVars>().expect_no_offenses("$stderr.puts('hello')\n");
    }

    #[test]
    fn accepts_load_path() {
        test::<GlobalVars>().expect_no_offenses("$LOAD_PATH\n");
    }

    #[test]
    fn accepts_loaded_features() {
        test::<GlobalVars>().expect_no_offenses("$LOADED_FEATURES\n");
    }

    #[test]
    fn accepts_program_name() {
        test::<GlobalVars>().expect_no_offenses("$0\n");
    }

    #[test]
    fn accepts_error_info() {
        test::<GlobalVars>().expect_no_offenses("$!\n");
    }

    #[test]
    fn accepts_child_status() {
        test::<GlobalVars>().expect_no_offenses("$?\n");
    }

    #[test]
    fn accepts_debug() {
        test::<GlobalVars>().expect_no_offenses("$DEBUG\n");
    }

    // --- backreferences are not flagged ---

    #[test]
    fn accepts_backref_dollar_one() {
        // $1 is an nth_ref node, not a gvar — not flagged.
        test::<GlobalVars>().expect_no_offenses("$1\n");
    }

    #[test]
    fn accepts_backref_dollar_two() {
        test::<GlobalVars>().expect_no_offenses("$2\n");
    }

    // --- AllowedVariables option ---

    fn allowed_opts() -> Options {
        Options { allowed_variables: vec!["$allowed".to_string()] }
    }

    #[test]
    fn accepts_user_allowed_variable() {
        test::<GlobalVars>()
            .with_options(&allowed_opts())
            .expect_no_offenses("$allowed\n");
    }

    #[test]
    fn flags_non_allowed_variable_with_option() {
        test::<GlobalVars>()
            .with_options(&allowed_opts())
            .expect_offense(indoc! {"
                $foo
                ^^^^ Do not introduce global variables.
            "});
    }

    #[test]
    fn accepts_user_allowed_variable_in_assignment() {
        test::<GlobalVars>()
            .with_options(&allowed_opts())
            .expect_no_offenses("$allowed = 1\n");
    }
}
murphy_plugin_api::submit_cop!(GlobalVars);
