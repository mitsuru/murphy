//! `Naming/FileName` — require source file names to use snake_case.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/FileName
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-ugzq]
//! notes: >
//!   Implements RuboCop's DEFAULT behavior end-to-end: the basename of the
//!   source file (`cx.file_path()` → last path component) must match
//!   `SNAKE_CASE = /^[\d[[:lower:]]_.?!]+$/` after RuboCop's exact basename
//!   normalization (delete one leading dot, strip only the last extension,
//!   replace the first `+` with `_`). `IgnoreExecutableScripts` (default
//!   true) suppresses the offense when the source begins with a `#!` shebang.
//!   The offense is RuboCop's `add_global_offense`, rendered at line 1
//!   column 1 (a single-column range at byte 0, clamped on empty source).
//!   Verified against rubocop 1.87.0 across snake_case/CamelCase/dash/space/
//!   multi-dot/leading-dot/`+`/digit/`?`/`!` basenames and the shebang skip.
//!
//!   GAP (murphy-ugzq) — the non-default, opt-in machinery is NOT
//!   implemented:
//!     * ExpectMatchingDefinition (default false) — requiring the file to
//!       define a matching class/module/Struct;
//!     * CheckDefinitionPathHierarchy / CheckDefinitionPathHierarchyRoots —
//!       namespace-hierarchy-vs-subdirectory matching;
//!     * AllowedAcronyms — acronym tolerance for the matching-definition
//!       check (feeds only the above, so it rides along in the deferral);
//!     * Regex — custom per-file-name pattern overriding SNAKE_CASE.
//!   These options are intentionally NOT declared on the cop's `Options`
//!   struct so a user setting them is not silently treated as supported.
//!   The shipped default.yml still carries all keys for forward config
//!   compatibility; the host tolerates config keys the cop does not declare.
//! ```

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

#[derive(Default)]
pub struct FileName;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "IgnoreExecutableScripts",
        default = true,
        description = "Don't report offending filenames for executable scripts (i.e. source files with a shebang in the first line)."
    )]
    pub ignore_executable_scripts: bool,
}

#[cop(
    name = "Naming/FileName",
    description = "Use snake_case for source file names.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl FileName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let basename = basename(cx.file_path());
        // The ABI returns an empty `file_path` when the host cannot expose one
        // (e.g. stdin). RuboCop never lints a pathless source, so guard against
        // emitting a nonsense offense on an empty basename.
        if basename.is_empty() {
            return;
        }
        if filename_good(basename) {
            return;
        }

        // `IgnoreExecutableScripts` (default true): a leading shebang means
        // the file is an executable script, so its name is not checked.
        if opts.ignore_executable_scripts && cx.source().starts_with("#!") {
            return;
        }

        let message =
            format!("The name of this source file (`{basename}`) should use snake_case.");
        cx.emit_offense(global_offense_range(cx), &message, None);
    }
}

/// Last path component of `path`, mirroring Ruby's `File.basename` for the
/// shapes a lint target takes (an empty path yields an empty basename).
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether `basename` satisfies RuboCop's default snake_case rule.
///
/// Mirrors RuboCop's `filename_good?` exactly, in order:
///   1. delete one leading `.` (`delete_prefix('.')`);
///   2. strip only the last extension (`sub(/\.[^.]+$/, '')`);
///   3. replace the first `+` with `_` (`sub('+', '_')`);
///   4. match `SNAKE_CASE = /^[\d[[:lower:]]_.?!]+$/` — non-empty, every
///      char a digit, lowercase letter, `_`, `.`, `?`, or `!`.
fn filename_good(basename: &str) -> bool {
    let stripped = basename.strip_prefix('.').unwrap_or(basename);
    let no_ext = strip_last_extension(stripped);
    let normalized = replace_first_plus(no_ext);
    matches_snake_case(&normalized)
}

/// `sub(/\.[^.]+$/, '')` — remove only the final extension. The extension is
/// the last `.` followed by ≥1 non-`.` characters at end of string. A
/// trailing `.` (no chars after) is NOT an extension and is left in place.
fn strip_last_extension(name: &str) -> &str {
    match name.rfind('.') {
        // `.` must be followed by at least one non-`.` char to count as an
        // extension (`[^.]+$`); a trailing dot leaves the name untouched.
        Some(dot) if dot + 1 < name.len() && !name[dot + 1..].contains('.') => &name[..dot],
        // No dot, or the only chars after the last dot include another dot
        // (impossible since it's the last dot) / it's a trailing dot.
        _ => name,
    }
}

/// `sub('+', '_')` — replace only the FIRST `+`. Returns the input borrowed
/// when there is no `+`, allocating only when a replacement is needed.
fn replace_first_plus(name: &str) -> std::borrow::Cow<'_, str> {
    match name.find('+') {
        Some(idx) => {
            let mut out = String::with_capacity(name.len());
            out.push_str(&name[..idx]);
            out.push('_');
            out.push_str(&name[idx + 1..]);
            std::borrow::Cow::Owned(out)
        }
        None => std::borrow::Cow::Borrowed(name),
    }
}

/// `SNAKE_CASE = /^[\d[[:lower:]]_.?!]+$/` — non-empty, and every char is an
/// ASCII digit, a lowercase letter, `_`, `.`, `?`, or `!`. `[[:lower:]]` is
/// Unicode-aware in Ruby; mirror that with `char::is_lowercase`.
fn matches_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_lowercase() || matches!(c, '_' | '.' | '?' | '!'))
}

/// RuboCop's `add_global_offense` renders at line 1, column 1 (length 0). The
/// caret-based test harness needs a non-empty range to carry a caret, so use
/// a single-column range at byte 0, clamped to the source length so an empty
/// source yields a valid zero-width range instead of an out-of-bounds end.
fn global_offense_range(cx: &Cx<'_>) -> Range {
    let end = 1.min(cx.source().len() as u32);
    Range { start: 0, end }
}

#[cfg(test)]
mod tests {
    use super::{FileName, Options, basename, filename_good};
    use murphy_plugin_api::test_support::test;

    // --- pure-function matrix (basename normalization + snake_case rule),
    //     ground truth = rubocop 1.87.0 (verified directly). ---

    #[test]
    fn snake_case_basenames_are_good() {
        assert!(filename_good("good_name.rb"));
        assert!(filename_good("foo.rb"));
        assert!(filename_good("snake_case_with_digits123.rb"));
        // `?`/`!` are allowed by SNAKE_CASE.
        assert!(filename_good("foo?.rb"));
        assert!(filename_good("foo!.rb"));
        // all-digits basename.
        assert!(filename_good("123.rb"));
        // only the LAST extension is stripped; `.` is allowed in SNAKE_CASE,
        // so multi-dot names are good.
        assert!(filename_good("foo.bar.rb"));
        // one leading dot is deleted, then the extension stripped.
        assert!(filename_good(".hidden.rb"));
        // first `+` becomes `_`.
        assert!(filename_good("foo+bar.rb"));
    }

    #[test]
    fn non_snake_case_basenames_are_bad() {
        assert!(!filename_good("badName.rb"));
        assert!(!filename_good("Foo.rb"));
        assert!(!filename_good("UPPER.rb"));
        assert!(!filename_good("with-dash.rb"));
        assert!(!filename_good("with space.rb"));
        // `sub('+', '_')` replaces only the FIRST `+`; a second `+` survives
        // and fails SNAKE_CASE.
        assert!(!filename_good("foo+bar+baz.rb"));
    }

    #[test]
    fn basename_takes_last_path_component() {
        assert_eq!(basename("/tmp/BadName.rb"), "BadName.rb");
        assert_eq!(basename("lib/foo/bar.rb"), "bar.rb");
        assert_eq!(basename("bar.rb"), "bar.rb");
        assert_eq!(basename("a\\b\\Win.rb"), "Win.rb");
    }

    // --- end-to-end through the file-path-aware harness. The source body is
    //     decoupled from the on-disk path, so the Ruby content is arbitrary
    //     while `with_file_path` carries the path under test. ---

    #[test]
    fn flags_camel_case_file_name() {
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_offense(
                "x = 1\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n",
            );
    }

    #[test]
    fn flags_dashed_file_name_with_full_path() {
        // A full path is reduced to its basename before the check.
        test::<FileName>()
            .with_file_path("/tmp/lib/with-dash.rb")
            .expect_offense(
                "x = 1\n\
                 ^ The name of this source file (`with-dash.rb`) should use snake_case.\n",
            );
    }

    #[test]
    fn accepts_snake_case_file_name() {
        test::<FileName>()
            .with_file_path("good_name.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn accepts_multi_dot_file_name() {
        test::<FileName>()
            .with_file_path("foo.bar.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn ignores_executable_script_by_default() {
        // Default IgnoreExecutableScripts: true — a shebang suppresses the
        // bad-name offense.
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_no_offenses("#!/usr/bin/env ruby\nx = 1\n");
    }

    #[test]
    fn checks_executable_script_when_option_disabled() {
        test::<FileName>()
            .with_options(&Options { ignore_executable_scripts: false })
            .with_file_path("BadName.rb")
            .expect_offense(
                "#!/usr/bin/env ruby\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n\
                 x = 1\n",
            );
    }

    #[test]
    fn empty_file_path_is_not_flagged() {
        // The host returns an empty `file_path` when it cannot expose one
        // (e.g. stdin); an empty basename must not produce a nonsense offense.
        test::<FileName>()
            .with_file_path("")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn regular_hash_comment_is_not_a_shebang() {
        // A leading `#` that is not `#!` does not count as a shebang.
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_offense(
                "# frozen_string_literal: true\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n\
                 x = 1\n",
            );
    }
}
murphy_plugin_api::submit_cop!(FileName);
