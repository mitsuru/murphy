//! `Style/FileNull` — use `File::NULL` instead of hardcoding null devices.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileNull
//! upstream_version_checked: 1.86.2
//! version_added: "1.69"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects '/dev/null', 'NUL', and 'NUL:' string literals and replaces them
//!   with `File::NULL`.
//!   Strings nested inside arrays or hashes are ignored (acceptable per RuboCop).
//!   Bare 'NUL' (case-insensitive) is flagged only when the source contains a
//!   '/dev/null' string elsewhere — implemented via a case-insensitive source
//!   scan of cx.source() in on_new_investigation, matching RuboCop's heuristic
//!   to avoid false positives since "NUL" has other meanings.
//!   Note: the source scan also matches '/dev/null' appearing in comments or
//!   heredocs; this is a minor fidelity gap vs RuboCop's AST-only scan.
//!   Matching is case-insensitive, full-string only (entire string content).
//!   Autocorrect is unsafe (value changes when run on non-Windows platforms).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! '/dev/null'
//! 'NUL'
//! 'NUL:'
//!
//! # good
//! File::NULL
//!
//! # ok — inside array or hash
//! null_devices = %w[/dev/null nul]
//! { unix: "/dev/null", windows: "nul" }
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the entire string literal node with `File::NULL`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FileNull;

#[cop(
    name = "Style/FileNull",
    description = "Use `File::NULL` instead of hardcoding null devices.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl FileNull {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // Determine whether the file contains '/dev/null' anywhere in its source.
        // This mirrors RuboCop's `@contain_dev_null_string_in_file` flag —
        // bare `'NUL'` is only flagged when '/dev/null' is present in the file.
        let contains_dev_null = cx
            .source()
            .to_ascii_lowercase()
            .contains("/dev/null");

        for node in std::iter::once(cx.root()).chain(cx.descendants(cx.root())) {
            check_node(node, cx, contains_dev_null);
        }
    }
}

fn check_node(node: NodeId, cx: &Cx<'_>, contains_dev_null: bool) {
    let NodeKind::Str(string_id) = *cx.kind(node) else {
        return;
    };

    let value = cx.string_str(string_id);

    // Empty strings are not null devices.
    if value.is_empty() {
        return;
    }

    let lower = value.to_ascii_lowercase();

    // Determine if this string is a null device reference.
    let is_null_device = match lower.as_str() {
        "/dev/null" => true,
        "nul:" => true,
        // Bare 'NUL' only if file also contains '/dev/null' — reduces false positives.
        "nul" => contains_dev_null,
        _ => false,
    };

    if !is_null_device {
        return;
    }

    // Skip strings inside arrays or hashes (acceptable per RuboCop).
    if is_inside_array_or_hash(node, cx) {
        return;
    }

    let msg = format!("Use `File::NULL` instead of `{value}`.");
    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(cx.range(node), "File::NULL");
}

/// Returns true if the node's parent is an array or hash pair.
fn is_inside_array_or_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(*cx.kind(parent), NodeKind::Array(_) | NodeKind::Pair { .. })
}

#[cfg(test)]
mod tests {
    use super::FileNull;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense + autocorrect cases ---

    #[test]
    fn flags_dev_null() {
        test::<FileNull>().expect_correction(
            indoc! {r#"
                open('/dev/null')
                     ^^^^^^^^^^^ Use `File::NULL` instead of `/dev/null`.
            "#},
            "open(File::NULL)\n",
        );
    }

    #[test]
    fn flags_nul_colon() {
        test::<FileNull>().expect_correction(
            indoc! {r#"
                open('NUL:')
                     ^^^^^^ Use `File::NULL` instead of `NUL:`.
            "#},
            "open(File::NULL)\n",
        );
    }

    #[test]
    fn flags_nul_when_dev_null_also_present() {
        // Both NUL and /dev/null are flagged when both appear in the file.
        test::<FileNull>().expect_offense(indoc! {r#"
            null_device = 'NUL'
                          ^^^^^ Use `File::NULL` instead of `NUL`.
            dev_null = '/dev/null'
                       ^^^^^^^^^^^ Use `File::NULL` instead of `/dev/null`.
        "#});
    }

    #[test]
    fn flags_dev_null_case_insensitive() {
        test::<FileNull>().expect_correction(
            indoc! {r#"
                open('/DEV/NULL')
                     ^^^^^^^^^^^ Use `File::NULL` instead of `/DEV/NULL`.
            "#},
            "open(File::NULL)\n",
        );
    }

    // --- allowed cases ---

    #[test]
    fn accepts_file_null_constant() {
        test::<FileNull>().expect_no_offenses("open(File::NULL)\n");
    }

    #[test]
    fn accepts_nul_without_dev_null_in_file() {
        // Bare 'NUL' is not flagged when no '/dev/null' is present in the file.
        test::<FileNull>().expect_no_offenses("open('NUL')\n");
    }

    #[test]
    fn accepts_dev_null_in_array() {
        test::<FileNull>().expect_no_offenses("null_devices = ['/dev/null', 'NUL']\n");
    }

    #[test]
    fn accepts_dev_null_in_hash() {
        test::<FileNull>().expect_no_offenses("{ unix: '/dev/null', windows: 'NUL' }\n");
    }

    #[test]
    fn accepts_unrelated_string() {
        test::<FileNull>().expect_no_offenses("open('/tmp/file')\n");
    }

    #[test]
    fn accepts_empty_string() {
        test::<FileNull>().expect_no_offenses("open('')\n");
    }
}
murphy_plugin_api::submit_cop!(FileNull);
