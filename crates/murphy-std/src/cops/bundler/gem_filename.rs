//! `Bundler/GemFilename` — enforce the filename used to manage gems. Unlike the
//! other Bundler cops, this one legitimately inspects the *file path* (via
//! `cx.file_path()`): it verifies the basename matches the configured
//! `EnforcedStyle` (`Gemfile` family vs `gems.rb` family). The host applies the
//! per-cop `Include` from `config/default.yml` (`**/Gemfile`, `**/gems.rb`,
//! `**/Gemfile.lock`, `**/gems.locked`), so this cop only runs on those four
//! filenames; the basename match below then decides whether to flag.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/GemFilename
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 `Bundler/GemFilename` verbatim. RuboCop's
//!   `on_new_investigation` takes `basename = File.basename(file_path)`, returns
//!   early when `expected_gemfile?` (i.e. the basename already matches the
//!   configured style's file family), and otherwise registers a *global*
//!   (whole-file) offense whose message embeds the full `file_path`. We
//!   reproduce this with a pure `check_filename(file_path, style)` returning the
//!   formatted message, and emit at `Range { start: 0, end: 0 }` to model
//!   `add_global_offense` (same modelling as `Lint/EmptyFile`).
//!
//!   The four messages map exactly to RuboCop's:
//!     * `EnforcedStyle: Gemfile` + basename `gems.rb`   -> MSG_GEMFILE_REQUIRED
//!     * `EnforcedStyle: Gemfile` + basename `gems.locked` -> MSG_GEMFILE_MISMATCHED
//!     * `EnforcedStyle: gems.rb` + basename `Gemfile`    -> MSG_GEMS_RB_REQUIRED
//!     * `EnforcedStyle: gems.rb` + basename `Gemfile.lock` -> MSG_GEMS_RB_MISMATCHED
//!   The full `file_path` (not just the basename) is interpolated into each
//!   message, exactly as RuboCop's `format(message, file_path: file_path)`. A
//!   basename outside the four known names (or an unreadable path) produces no
//!   offense, matching `expected_gemfile?` returning false and neither offense
//!   branch matching. `default_enabled = true` matches default.yml.
//!
//!   Because the cop test harness pins the source path to `t.rb`, the
//!   filename logic is exercised through direct `check_filename` unit tests
//!   rather than the caret-annotation harness; CLI firing was verified against
//!   a real `gems.rb`. Behaviour verified against standalone rubocop 1.87.0.
//! ```

use murphy_plugin_api::{cop, CopOptionEnum, CopOptions, Cx, Range};
use std::path::Path;

#[derive(Default)]
pub struct GemFilename;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "Gemfile")]
    Gemfile,
    #[option(value = "gems.rb")]
    GemsRb,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "Gemfile",
        description = "Which filename to enforce for managing gems."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Bundler/GemFilename",
    description = "Enforces the filename for managing gems.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl GemFilename {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if let Some(message) = check_filename(cx.file_path(), opts.enforced_style) {
            // RuboCop's `add_global_offense` — a whole-file offense with no
            // associated source range; modelled as `0..0` like `Lint/EmptyFile`.
            cx.emit_offense(Range { start: 0, end: 0 }, &message, None);
        }
    }
}

/// RuboCop's `on_new_investigation` decision, as a pure function: returns the
/// formatted offense message when `file_path`'s basename violates `style`, else
/// `None`. The full `file_path` (not just the basename) is interpolated into the
/// message, matching `format(message, file_path: file_path)`.
fn check_filename(file_path: &str, style: EnforcedStyle) -> Option<String> {
    let basename = Path::new(file_path).file_name()?.to_str()?;
    match (style, basename) {
        // `EnforcedStyle: Gemfile`: flag the gems.rb file family.
        (EnforcedStyle::Gemfile, "gems.rb") => Some(format!(
            "`gems.rb` file was found but `Gemfile` is required (file path: {file_path})."
        )),
        (EnforcedStyle::Gemfile, "gems.locked") => Some(format!(
            "Expected a `Gemfile.lock` with `Gemfile` but found `gems.locked` file (file path: {file_path})."
        )),
        // `EnforcedStyle: gems.rb`: flag the Gemfile file family.
        (EnforcedStyle::GemsRb, "Gemfile") => Some(format!(
            "`Gemfile` was found but `gems.rb` file is required (file path: {file_path})."
        )),
        (EnforcedStyle::GemsRb, "Gemfile.lock") => Some(format!(
            "Expected a `gems.locked` file with `gems.rb` but found `Gemfile.lock` (file path: {file_path})."
        )),
        // Already the expected family, or an unrelated basename → no offense.
        _ => None,
    }
}

murphy_plugin_api::submit_cop!(GemFilename);

#[cfg(test)]
mod tests {
    use super::{check_filename, EnforcedStyle};

    // ----- EnforcedStyle: Gemfile (default) -----

    #[test]
    fn gemfile_style_accepts_gemfile_family() {
        assert_eq!(check_filename("Gemfile", EnforcedStyle::Gemfile), None);
        assert_eq!(check_filename("Gemfile.lock", EnforcedStyle::Gemfile), None);
        assert_eq!(
            check_filename("path/to/Gemfile", EnforcedStyle::Gemfile),
            None
        );
    }

    #[test]
    fn gemfile_style_flags_gems_rb_with_full_path() {
        assert_eq!(
            check_filename("path/to/gems.rb", EnforcedStyle::Gemfile),
            Some(
                "`gems.rb` file was found but `Gemfile` is required (file path: path/to/gems.rb)."
                    .to_string()
            )
        );
    }

    #[test]
    fn gemfile_style_flags_gems_locked_as_mismatch() {
        assert_eq!(
            check_filename("gems.locked", EnforcedStyle::Gemfile),
            Some(
                "Expected a `Gemfile.lock` with `Gemfile` but found `gems.locked` file (file path: gems.locked)."
                    .to_string()
            )
        );
    }

    // ----- EnforcedStyle: gems.rb -----

    #[test]
    fn gems_rb_style_accepts_gems_rb_family() {
        assert_eq!(check_filename("gems.rb", EnforcedStyle::GemsRb), None);
        assert_eq!(check_filename("gems.locked", EnforcedStyle::GemsRb), None);
    }

    #[test]
    fn gems_rb_style_flags_gemfile_with_full_path() {
        assert_eq!(
            check_filename("path/to/Gemfile", EnforcedStyle::GemsRb),
            Some(
                "`Gemfile` was found but `gems.rb` file is required (file path: path/to/Gemfile)."
                    .to_string()
            )
        );
    }

    #[test]
    fn gems_rb_style_flags_gemfile_lock_as_mismatch() {
        assert_eq!(
            check_filename("Gemfile.lock", EnforcedStyle::GemsRb),
            Some(
                "Expected a `gems.locked` file with `gems.rb` but found `Gemfile.lock` (file path: Gemfile.lock)."
                    .to_string()
            )
        );
    }

    // ----- Inert on unrelated / empty paths (the harness path `t.rb`, etc.) -----

    #[test]
    fn unrelated_basename_is_never_flagged() {
        assert_eq!(check_filename("t.rb", EnforcedStyle::Gemfile), None);
        assert_eq!(check_filename("t.rb", EnforcedStyle::GemsRb), None);
        assert_eq!(check_filename("app/models/user.rb", EnforcedStyle::Gemfile), None);
    }

    #[test]
    fn empty_path_is_safe() {
        assert_eq!(check_filename("", EnforcedStyle::Gemfile), None);
        assert_eq!(check_filename("", EnforcedStyle::GemsRb), None);
    }
}
