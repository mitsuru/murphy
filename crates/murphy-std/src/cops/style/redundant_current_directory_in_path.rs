//! `Style/RedundantCurrentDirectoryInPath` — flags `require_relative` calls
//! whose first argument string starts with a redundant `./` prefix.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantCurrentDirectoryInPath
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - require_relative with a plain string (Str) argument whose content
//!       starts with `./` (or `.//`, `.///`, etc. — one or more slashes).
//!     - Autocorrect: removes the leading `./+` prefix bytes from the
//!       string source (not the string content — uses the raw source
//!       offset so the opening quote is preserved).
//!     - Interpolated/non-string first arguments are skipped conservatively.
//!     - A mid-path `./` (e.g. `'foo/./bar'`) is not flagged — the
//!       content check is anchored to the start of the decoded string.
//!   Safety: the correction is safe — `require_relative 'path/to/feature'`
//!   behaves identically to `require_relative './path/to/feature'`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantCurrentDirectoryInPath;

const MSG: &str = "Remove the redundant current directory path.";

#[cop(
    name = "Style/RedundantCurrentDirectoryInPath",
    description = "Checks for a redundant current directory in a path given to `require_relative`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantCurrentDirectoryInPath {
    #[on_node(kind = "send", methods = ["require_relative"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    let Some(&first_arg) = args.first() else {
        return;
    };

    // Only plain string literals — skip interpolated strings (Dstr), etc.
    let NodeKind::Str(string_id) = *cx.kind(first_arg) else {
        return;
    };

    // The decoded string content must start with `./` (one or more slashes).
    let content = cx.string_str(string_id);
    let redundant_len = redundant_prefix_length(content);
    if redundant_len == 0 {
        return;
    }

    // Locate the `./` bytes in the raw source. The raw source includes the
    // opening quote character, so we search from offset 1.
    // `index` gives the byte offset relative to the start of raw_source.
    let raw = cx.raw_source(cx.range(first_arg));
    let Some(idx) = raw.find("./") else {
        return;
    };

    // The removal range: from the `./` start to the end of the redundant
    // prefix (i.e. `./` plus any additional `/` characters).
    let removal_start = cx.range(first_arg).start + idx as u32;
    let removal_end = removal_start + redundant_len as u32;
    let offense_range = Range {
        start: removal_start,
        end: removal_end,
    };

    cx.emit_offense(offense_range, MSG, None);
    cx.emit_edit(offense_range, "");
}

/// Returns the length (in bytes) of the leading `./+` prefix in `content`,
/// or 0 if the content does not start with `./`.
fn redundant_prefix_length(content: &str) -> usize {
    if !content.starts_with("./") {
        return 0;
    }
    // Count `.` + all consecutive `/` characters.
    let extra_slashes = content[2..].bytes().take_while(|&b| b == b'/').count();
    1 + 1 + extra_slashes // `.` + at least one `/` + any extras
}

#[cfg(test)]
mod tests {
    use super::RedundantCurrentDirectoryInPath;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_no_current_dir_prefix() {
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require_relative 'path/to/feature'"#);
    }

    #[test]
    fn no_offense_parent_dir_prefix() {
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require_relative '../path/to/feature'"#);
    }

    #[test]
    fn no_offense_mid_path_dot_slash() {
        // Only anchored at the start of the content.
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require_relative 'foo/./bar'"#);
    }

    #[test]
    fn no_offense_interpolated_string() {
        // Dstr (interpolated) first arg is skipped conservatively.
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require_relative "./#{feature}""#);
    }

    #[test]
    fn no_offense_not_require_relative() {
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require './path/to/feature'"#);
    }

    // --- Offense cases ---

    #[test]
    fn flags_dot_slash_prefix() {
        test::<RedundantCurrentDirectoryInPath>().expect_offense(indoc! {r#"
            require_relative './path/to/feature'
                              ^^ Remove the redundant current directory path.
        "#});
    }

    #[test]
    fn flags_double_slash_prefix() {
        test::<RedundantCurrentDirectoryInPath>().expect_offense(indoc! {r#"
            require_relative './/path/to/feature'
                              ^^^ Remove the redundant current directory path.
        "#});
    }

    #[test]
    fn flags_dot_slash_only() {
        test::<RedundantCurrentDirectoryInPath>().expect_offense(indoc! {r#"
            require_relative './'
                              ^^ Remove the redundant current directory path.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_dot_slash_prefix() {
        test::<RedundantCurrentDirectoryInPath>().expect_correction(
            indoc! {r#"
                require_relative './path/to/feature'
                                  ^^ Remove the redundant current directory path.
            "#},
            "require_relative 'path/to/feature'\n",
        );
    }

    #[test]
    fn corrects_double_slash_prefix() {
        test::<RedundantCurrentDirectoryInPath>().expect_correction(
            indoc! {r#"
                require_relative './/path/to/feature'
                                  ^^^ Remove the redundant current directory path.
            "#},
            "require_relative 'path/to/feature'\n",
        );
    }

    #[test]
    fn corrects_idempotent_already_clean() {
        // After correction, a second pass should find no offenses.
        test::<RedundantCurrentDirectoryInPath>()
            .expect_no_offenses(r#"require_relative 'path/to/feature'"#);
    }
}

murphy_plugin_api::submit_cop!(RedundantCurrentDirectoryInPath);
