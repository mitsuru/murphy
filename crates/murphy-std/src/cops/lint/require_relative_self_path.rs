//! `Lint/RequireRelativeSelfPath` — removes `require_relative` of the current file.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RequireRelativeSelfPath
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port detects literal self-requires by comparing the requested stem
//!   with the current file stem. Directory-sensitive path normalization is a v1
//!   gap.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};
use std::path::Path;

#[derive(Default)]
pub struct RequireRelativeSelfPath;

#[cop(name = "Lint/RequireRelativeSelfPath", description = "Checks for uses a file requiring itself with `require_relative`.", default_severity = "warning", default_enabled = true, options = NoOptions)]
impl RequireRelativeSelfPath {
    #[on_node(kind = "send", methods = ["require_relative"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let args = cx.call_arguments(node);
        let [arg] = args else {
            return;
        };
        let NodeKind::Str(sid) = *cx.kind(*arg) else {
            return;
        };
        let required = Path::new(cx.string_str(sid));
        let current = Path::new(cx.file_path());
        if required.file_stem() == current.file_stem() {
            cx.emit_offense(
                cx.range(node),
                "Remove the `require_relative` that requires itself.",
                None,
            );
            cx.emit_edit(cx.range(node), "");
        }
    }
}

murphy_plugin_api::submit_cop!(RequireRelativeSelfPath);

#[cfg(test)]
mod tests {
    use super::RequireRelativeSelfPath;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_self_require() {
        test::<RequireRelativeSelfPath>().expect_offense(indoc! {r#"
            require_relative 't'
            ^^^^^^^^^^^^^^^^^^^^^^ Remove the `require_relative` that requires itself.
        "#});
    }
}
