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
//!   Initial port detects literal self-requires by resolving the requested path
//!   relative to the current file directory and comparing the normalized path
//!   against the current file path. Filesystem symlink/canonicalize behavior is
//!   a v1 gap.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};
use std::path::{Component, Path, PathBuf};

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
        if resolves_to_current_file(cx.string_str(sid), cx.file_path()) {
            cx.emit_offense(
                cx.range(node),
                "Remove the `require_relative` that requires itself.",
                None,
            );
            cx.emit_edit(cx.range(node), "");
        }
    }
}

fn resolves_to_current_file(required: &str, current_file: &str) -> bool {
    let current = normalize_path(Path::new(current_file));
    let required_path = Path::new(required);
    let base = if required_path.is_absolute() {
        required_path.to_path_buf()
    } else {
        Path::new(current_file)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(required_path)
    };
    let mut candidate = normalize_path(&base);
    if candidate.extension().is_none() {
        candidate.set_extension("rb");
    }
    candidate == current
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = out.components().last() {
                    out.pop();
                } else {
                    out.push(component.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

    #[test]
    fn accepts_same_basename_in_different_directory() {
        test::<RequireRelativeSelfPath>()
            .expect_no_offenses("require_relative '../serializers/t'\n");
    }

    #[test]
    fn accepts_leading_parent_directory_that_cannot_be_normalized_away() {
        test::<RequireRelativeSelfPath>().expect_no_offenses("require_relative '../t'\n");
    }
}
