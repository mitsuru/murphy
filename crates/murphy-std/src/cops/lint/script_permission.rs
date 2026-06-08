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
//!   metadata, and `emit_edit` cannot chmod. This port does not report offenses
//!   until executable-permission metadata is available; non-executable filtering
//!   and chmod autocorrection are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions};

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
    fn check_file(&self, _cx: &Cx<'_>) {
        // Avoid false positives: Murphy's current cop API does not expose file
        // mode metadata, so we cannot distinguish executable from non-executable scripts.
    }
}

murphy_plugin_api::submit_cop!(ScriptPermission);

#[cfg(test)]
mod tests {
    use super::ScriptPermission;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_shebang_source_without_permission_metadata() {
        test::<ScriptPermission>().expect_no_offenses(indoc! {r#"
            #!/usr/bin/env ruby
            puts 'hello'
        "#});
    }

    #[test]
    fn accepts_source_without_shebang() {
        test::<ScriptPermission>().expect_no_offenses("puts 'hello'\n");
    }
}
