//! Diff-driven lint (`murphy lint --since <ref>`) — Phase 9 B5.
//!
//! `--since` restricts the lint to files changed since `<ref>`, the
//! `Pronto`/`Danger` replacement the roadmap (§7) folds into B5. The file
//! list is computed up front, so unchanged files are never parsed or
//! linted — every `--format` (including `sarif` for Code Scanning) sees
//! the diff subset, and the ADR 0006 default JSON shape is untouched
//! (output-only file restriction, like the B3 baseline filter).
//!
//! ## Git semantics
//!
//! - Base: `git merge-base HEAD <ref>` when it succeeds (PR semantics: diff
//!   against where the branch forked, not where the base has since moved);
//!   otherwise `<ref>` directly (shallow clones, non-branch refs).
//! - Changed: `git diff --name-only -z --diff-filter=ACMR <base> --`
//!   (worktree — staged and unstaged — versus the base) plus untracked
//!   files (`git ls-files --others --exclude-standard -z`).
//! - Deleted files never appear (`ACMR` excludes `D`); renames do.
//! - Intersected with the discovery/explicit-path file set by the caller,
//!   so `--since` composes with paths, `--baseline`, and `--format`.
//!
//! A non-git directory or an unknown ref is a setup error (exit `2`), never
//! silently ignored. CI checkouts need full history for merge-base
//! (`actions/checkout` with `fetch-depth: 0`); without it the `<ref>`
//! fallback still works as long as the ref itself is fetched.

use std::collections::BTreeSet;
use std::fmt;
use std::process::Command;

/// Failure to resolve `--since <ref>` to a changed-file set. The CLI maps
/// every variant to exit `2` (config/setup error) via its `AppError::setup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinceError {
    /// Current directory is not inside a git work tree.
    NotGitRepo,
    /// `<ref>` does not resolve (typo, unfetched branch, shallow checkout).
    UnknownRef(String),
    /// Any other git failure; carries git's stderr.
    GitFailed(String),
}

impl fmt::Display for SinceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SinceError::NotGitRepo => {
                write!(f, "not a git repository: `--since` needs a git checkout")
            }
            SinceError::UnknownRef(git_ref) => write!(
                f,
                "unknown git ref {git_ref:?}: fetch it first (CI: `fetch-depth: 0`)"
            ),
            SinceError::GitFailed(detail) => write!(f, "cannot resolve `--since`: {detail}"),
        }
    }
}

impl std::error::Error for SinceError {}

/// Run git and return stdout on success, or a classified [`SinceError`].
fn git_output(args: &[&str], git_ref: &str) -> Result<Vec<u8>, SinceError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| SinceError::GitFailed(format!("cannot run git: {e}")))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Err(classify_git_failure(&stderr, git_ref))
}

/// Classify git's stderr into the actionable [`SinceError`] variant.
///
/// Matching is case-insensitive: git phrases the same failure differently
/// across subcommands (`fatal: not a git repository` vs `warning: Not a git
/// repository. Use --no-index ...` + a full usage dump). Only the first
/// non-empty line is kept, so a usage dump never floods murphy's stderr.
fn classify_git_failure(stderr: &str, git_ref: &str) -> SinceError {
    let lowered = stderr.to_lowercase();
    if lowered.contains("not a git repository") {
        SinceError::NotGitRepo
    } else if lowered.contains("unknown revision") || lowered.contains("bad revision") {
        SinceError::UnknownRef(git_ref.to_string())
    } else {
        let first = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("git failed with no message");
        SinceError::GitFailed(first.to_string())
    }
}

/// Split NUL-separated git `-z` output into a file-path set.
fn parse_nul_paths(stdout: &[u8]) -> BTreeSet<String> {
    stdout
        .split(|b| *b == 0)
        .filter_map(|chunk| {
            if chunk.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(chunk).into_owned())
            }
        })
        .collect()
}

/// Strip leading `./` segments so discovery-shaped paths (`./a.rb`) and
/// git-shaped paths (`a.rb`) share one key. Mirrors the B3 baseline
/// normalization; matching is otherwise verbatim (relative to the
/// directory murphy runs from).
fn normalize_path(file: &str) -> &str {
    let mut rest = file;
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest
}

/// Resolve `--since <ref>` to the set of changed file paths (git-shaped,
/// relative to the repo root / invocation directory).
pub fn resolve_changed_files(git_ref: &str) -> Result<BTreeSet<String>, SinceError> {
    // PR semantics first: diff against the fork point, not the moving base
    // tip. Falls back to the ref itself when merge-base fails (shallow
    // clone, tag, or anything without a common ancestor with HEAD).
    let base = match git_output(&["merge-base", "HEAD", git_ref], git_ref) {
        Ok(stdout) => String::from_utf8_lossy(&stdout).trim().to_string(),
        Err(_) => git_ref.to_string(),
    };
    if base.is_empty() {
        return Err(SinceError::UnknownRef(git_ref.to_string()));
    }
    let mut changed = parse_nul_paths(&git_output(
        &[
            "diff",
            "--name-only",
            "-z",
            "--diff-filter=ACMR",
            &base,
            "--",
        ],
        git_ref,
    )?);
    changed.extend(parse_nul_paths(&git_output(
        &["ls-files", "--others", "--exclude-standard", "-z"],
        git_ref,
    )?));
    Ok(changed)
}

/// Intersect the lint file set with the `--since` changed set, preserving
/// the lint set's (sorted) order. Comparison is on [`normalize_path`],
/// returned values are the lint set's original strings.
pub fn restrict_to_changed(
    all_paths: &BTreeSet<String>,
    changed: &BTreeSet<String>,
) -> Vec<String> {
    let normalized: BTreeSet<&str> = changed.iter().map(|p| normalize_path(p)).collect();
    all_paths
        .iter()
        .filter(|p| normalized.contains(normalize_path(p)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_dot_slash_segments() {
        assert_eq!(normalize_path("./a.rb"), "a.rb");
        assert_eq!(normalize_path("././a.rb"), "a.rb");
        assert_eq!(normalize_path("a.rb"), "a.rb");
        assert_eq!(normalize_path("sub/b.rb"), "sub/b.rb");
    }

    #[test]
    fn restrict_keeps_sorted_lint_order_and_matches_across_dot_slash() {
        let all: BTreeSet<String> = ["./b.rb", "a.rb", "c.rb"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let changed: BTreeSet<String> = ["a.rb", "b.rb"].iter().map(ToString::to_string).collect();
        assert_eq!(
            restrict_to_changed(&all, &changed),
            vec!["./b.rb".to_string(), "a.rb".to_string()]
        );
    }

    #[test]
    fn restrict_with_empty_changed_set_is_empty() {
        let all: BTreeSet<String> = ["a.rb".to_string()].into_iter().collect();
        assert!(restrict_to_changed(&all, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn classify_git_failure_routes_known_stderr() {
        assert_eq!(
            classify_git_failure("fatal: not a git repository (or any parent)", "HEAD"),
            SinceError::NotGitRepo
        );
        assert_eq!(
            classify_git_failure(
                "warning: Not a git repository. Use --no-index to compare two paths\nusage: git diff ...",
                "HEAD"
            ),
            SinceError::NotGitRepo
        );
        assert_eq!(
            classify_git_failure("fatal: bad revision 'nope'", "nope"),
            SinceError::UnknownRef("nope".to_string())
        );
        assert_eq!(
            classify_git_failure("fatal: unknown revision 'nope'", "nope"),
            SinceError::UnknownRef("nope".to_string())
        );
        assert!(matches!(
            classify_git_failure("fatal: something else", "HEAD"),
            SinceError::GitFailed(_)
        ));
    }

    #[test]
    fn git_failed_keeps_only_the_first_line() {
        let err = classify_git_failure("fatal: boom\nline two\nline three", "HEAD");
        assert_eq!(err, SinceError::GitFailed("fatal: boom".to_string()));
    }

    #[test]
    fn error_messages_guide_the_fix() {
        assert!(SinceError::NotGitRepo.to_string().contains("git checkout"));
        assert!(
            SinceError::UnknownRef("origin/main".to_string())
                .to_string()
                .contains("fetch-depth")
        );
    }

    #[test]
    fn parse_nul_paths_drops_empties() {
        let paths = parse_nul_paths(b"a.rb\0b.rb\0\0");
        assert_eq!(
            paths,
            BTreeSet::from(["a.rb".to_string(), "b.rb".to_string()])
        );
    }
}
