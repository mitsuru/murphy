//! `Lint/ScriptPermission` — reports Ruby scripts with a shebang.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ScriptPermission
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 exposes source text and file path but not executable file stat
//!   metadata, and `emit_edit` cannot chmod. This port is report-only for files
//!   whose source starts with a shebang; exact non-executable filtering and
//!   chmod autocorrection are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, Range};
use std::path::Path;

#[derive(Default)]
pub struct ScriptPermission;

#[cop(
    name = "Lint/ScriptPermission",
    description = "Checks shebang scripts that may need execute permission.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ScriptPermission {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let source = cx.source();
        if !source.starts_with("#!") {
            return;
        }

        let end = source.find('\n').unwrap_or(source.len()) as u32;
        let basename = Path::new(cx.file_path())
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(cx.file_path());
        let message = format!("Script file {basename} doesn't have execute permission.");
        cx.emit_offense(Range { start: 0, end }, &message, None);
    }
}

murphy_plugin_api::submit_cop!(ScriptPermission);

#[cfg(test)]
mod tests {
    use super::ScriptPermission;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn reports_shebang_source() {
        test::<ScriptPermission>().expect_offense(indoc! {r#"
            #!/usr/bin/env ruby
            ^^^^^^^^^^^^^^^^^^^^ Script file t.rb doesn't have execute permission.
            puts 'hello'
        "#});
    }

    #[test]
    fn accepts_source_without_shebang() {
        test::<ScriptPermission>().expect_no_offenses("puts 'hello'\n");
    }
}
