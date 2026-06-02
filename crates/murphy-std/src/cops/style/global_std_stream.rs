//! `Style/GlobalStdStream` — use `$stdout/$stderr/$stdin` instead of
//! `STDOUT/STDERR/STDIN`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/GlobalStdStream
//! upstream_version_checked: 1.86.2
//! version_added: "0.13"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags bare STDIN/STDOUT/STDERR (and ::STDIN/::STDOUT/::STDERR) and
//!   replaces them with the corresponding global variable form.
//!   Namespaced constants like Foo::STDOUT are not flagged.
//!   Assignment exception: $stdin = STDIN (and variants) are not flagged,
//!   mirroring RuboCop's const_to_gvar_assignment? matcher.
//!   Autocorrect is unsafe (STDOUT and $stdout may refer to different objects).
//!   Message always uses the bare constant name even for ::STDOUT.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! STDOUT.puts('hello')
//! STDERR.puts('hello')
//! STDIN.gets
//! hash = { out: STDOUT }
//! ::STDOUT.puts('hello')
//!
//! # good
//! $stdout.puts('hello')
//! $stderr.puts('hello')
//! $stdin.gets
//! $stdin = STDIN         # assignment exception — not flagged
//! Foo::STDOUT            # namespaced — not flagged
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the entire const node (including any leading `::`) with the
//! lowercased global variable form: `$stdout`, `$stderr`, or `$stdin`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct GlobalStdStream;

#[cop(
    name = "Style/GlobalStdStream",
    description = "Use `$stdout/$stderr/$stdin` instead of `STDOUT/STDERR/STDIN`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl GlobalStdStream {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Const { name, .. } = *cx.kind(node) else {
        return;
    };

    let const_name = cx.symbol_str(name);

    // Only STDIN, STDOUT, STDERR.
    let gvar_name = match const_name {
        "STDIN" => "$stdin",
        "STDOUT" => "$stdout",
        "STDERR" => "$stderr",
        _ => return,
    };

    // Must be a global (unscoped or cbase-scoped) constant.
    // cx.is_global_const handles both `STDOUT` (nil scope) and `::STDOUT` (cbase scope).
    if !cx.is_global_const(node, const_name) {
        return;
    }

    // Assignment exception: skip if this node is the value side of a
    // `$stdin = STDIN` / `$stdout = STDOUT` / `$stderr = STDERR` assignment.
    if is_gvar_assignment_rhs(node, gvar_name, cx) {
        return;
    }

    let message = format!("Use `{gvar_name}` instead of `{const_name}`.");
    cx.emit_offense(cx.range(node), &message, None);
    cx.emit_edit(cx.range(node), gvar_name);
}

/// Returns `true` when `node` is the `value` child of a `Gvasgn` node
/// whose name matches `gvar_name` (e.g. `$stdout = STDOUT` → skip).
fn is_gvar_assignment_rhs(node: NodeId, gvar_name: &str, cx: &Cx<'_>) -> bool {
    let Some(parent_id) = cx.parent(node).get() else {
        return false;
    };
    let NodeKind::Gvasgn { name: asgn_name, value } = *cx.kind(parent_id) else {
        return false;
    };
    // The value must be this node.
    let Some(value_id) = value.get() else {
        return false;
    };
    if value_id != node {
        return false;
    }
    cx.symbol_str(asgn_name) == gvar_name
}

#[cfg(test)]
mod tests {
    use super::GlobalStdStream;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense + autocorrect cases ---

    #[test]
    fn flags_stdout_and_corrects() {
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                STDOUT.puts('hello')
                ^^^^^^ Use `$stdout` instead of `STDOUT`.
            "#},
            "$stdout.puts('hello')\n",
        );
    }

    #[test]
    fn flags_stderr_and_corrects() {
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                STDERR.puts('hello')
                ^^^^^^ Use `$stderr` instead of `STDERR`.
            "#},
            "$stderr.puts('hello')\n",
        );
    }

    #[test]
    fn flags_stdin_and_corrects() {
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                STDIN.gets
                ^^^^^ Use `$stdin` instead of `STDIN`.
            "#},
            "$stdin.gets\n",
        );
    }

    #[test]
    fn flags_stdout_in_hash_literal() {
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                hash = { out: STDOUT, key: value }
                              ^^^^^^ Use `$stdout` instead of `STDOUT`.
            "#},
            "hash = { out: $stdout, key: value }\n",
        );
    }

    #[test]
    fn flags_stdout_as_default_parameter() {
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                def m(out = STDOUT)
                            ^^^^^^ Use `$stdout` instead of `STDOUT`.
                  out.puts('hello')
                end
            "#},
            "def m(out = $stdout)\n  out.puts('hello')\nend\n",
        );
    }

    #[test]
    fn flags_cbase_stdout_and_corrects() {
        // ::STDOUT should be corrected to $stdout (no leading ::).
        test::<GlobalStdStream>().expect_correction(
            indoc! {r#"
                ::STDOUT.puts('hello')
                ^^^^^^^^ Use `$stdout` instead of `STDOUT`.
            "#},
            "$stdout.puts('hello')\n",
        );
    }

    // --- negative cases ---

    #[test]
    fn accepts_global_variable_stdout() {
        test::<GlobalStdStream>().expect_no_offenses("$stdout.puts('hello')\n");
    }

    #[test]
    fn accepts_global_variable_stderr() {
        test::<GlobalStdStream>().expect_no_offenses("$stderr.puts('hello')\n");
    }

    #[test]
    fn accepts_global_variable_stdin() {
        test::<GlobalStdStream>().expect_no_offenses("$stdin.gets\n");
    }

    #[test]
    fn accepts_namespaced_stdout() {
        // Foo::STDOUT — namespaced, not flagged.
        test::<GlobalStdStream>().expect_no_offenses("Foo::STDOUT.puts('hello')\n");
    }

    #[test]
    fn accepts_namespaced_stderr() {
        test::<GlobalStdStream>().expect_no_offenses("Foo::STDERR.puts('hello')\n");
    }

    #[test]
    fn accepts_gvasgn_stdout_exception() {
        // $stdout = STDOUT — assignment to corresponding gvar, not flagged.
        test::<GlobalStdStream>().expect_no_offenses("$stdout = STDOUT\n");
    }

    #[test]
    fn accepts_gvasgn_stderr_exception() {
        test::<GlobalStdStream>().expect_no_offenses("$stderr = STDERR\n");
    }

    #[test]
    fn accepts_gvasgn_stdin_exception() {
        test::<GlobalStdStream>().expect_no_offenses("$stdin = STDIN\n");
    }

    #[test]
    fn flags_mismatched_gvasgn() {
        // $stderr = STDOUT — mismatch, STDOUT should still be flagged.
        test::<GlobalStdStream>().expect_offense(indoc! {r#"
            $stderr = STDOUT
                      ^^^^^^ Use `$stdout` instead of `STDOUT`.
        "#});
    }

    #[test]
    fn accepts_unrelated_constant() {
        test::<GlobalStdStream>().expect_no_offenses("File.open('foo')\n");
    }
}
murphy_plugin_api::submit_cop!(GlobalStdStream);
