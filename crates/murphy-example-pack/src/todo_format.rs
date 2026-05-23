//! `Example/TodoFormat` — flags TODO/FIXME comments. Demo of:
//!
//! - file-visit dispatch (`KINDS = &[]`, see `NodeCop` doc + the
//!   `dispatch::run_cops` empty-kinds branch — same shape as
//!   `Layout/TrailingWhitespace`).
//! - `#[derive(CopOptions)]` with a `Vec<String>` and a `bool` option,
//!   showing the array-default literal syntax (`default = ["TODO",
//!   "FIXME"]`) and a boolean toggle.
//! - raw-source byte scanning. Offense ranges are byte offsets (ADR 0001).
//!
//! ## Detection heuristic (substring, not anchored)
//!
//! Tag detection uses a plain `contains` for `"# Tag"` / `"#Tag"` substrings,
//! intentionally false-positive-tolerant. Real packs likely want the
//! anchored "comment-start + tag + word-boundary" form instead — this demo
//! is kept simple so the template stays readable.
//!
//! ## Why the demo emits on every match (even with `require_author = false`)
//!
//! Real linters would normally treat `require_author = false` as "this
//! cop is fully relaxed and emits nothing"; the e2e test in
//! `crates/murphy-cli/tests/plugin_pack_e2e.rs` needs the cop to fire on
//! a vanilla `# TODO: ...` line so this demo intentionally still emits a
//! "tag detected" warning in that mode. Documented oddity, kept for the
//! e2e fixture.

use murphy_plugin_api::{Cop, CopOptions, Cx, NodeCop, NodeId, NodeKindTag, Range, Severity};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TodoFormat;

/// Cop options for [`TodoFormat`]. The schema is exported via the
/// `#[derive(CopOptions)]` macro for the host's validation gate; the
/// runtime path uses `Default` until §12d's options plumbing lands
/// (same staging as `Style/StringLiterals`).
#[derive(CopOptions)]
pub struct TodoFormatOptions {
    #[option(
        default = ["TODO", "FIXME"],
        description = "Tags treated as todo-style markers."
    )]
    pub tags: Vec<String>,
    #[option(
        default = false,
        description = "When true, require an @author <name> annotation on the same line."
    )]
    pub require_author: bool,
}

impl Cop for TodoFormat {
    type Options = TodoFormatOptions;
    const NAME: &'static str = "Example/TodoFormat";
    const DESCRIPTION: &'static str =
        "Check format of TODO/FIXME comments (optionally require @author tag).";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for TodoFormat {
    /// `KINDS = &[]` → file-visit dispatch (`TrailingWhitespace` same form).
    const KINDS: &'static [NodeKindTag] = &[];

    fn check(&self, _node: NodeId, cx: &Cx<'_>) {
        // Runtime option access (murphy-9cr.9) is not yet wired through
        // `Cx`; v1 honours the `Default` (same staging as `StringLiterals`).
        // Until that lands, declaring e.g. `tags = ["XXX"]` in `murphy.toml`
        // for this cop is silently ignored. Plugin authors copying this
        // template should expect the same staging.
        let opts = TodoFormatOptions::default();

        let src = cx.source();
        let bytes = src.as_bytes();
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                check_line(cx, src, line_start, i, &opts);
                line_start = i + 1;
            }
            i += 1;
        }
        // Final line (no trailing newline).
        if line_start < bytes.len() {
            check_line(cx, src, line_start, bytes.len(), &opts);
        }
    }
}

/// Inspect bytes `[line_start, line_end)` and emit if the line is a `#`
/// comment containing one of the configured tags.
fn check_line(
    cx: &Cx<'_>,
    src: &str,
    line_start: usize,
    line_end: usize,
    opts: &TodoFormatOptions,
) {
    let line = &src[line_start..line_end];
    // Strip leading whitespace; only `#`-prefixed lines are comments.
    let stripped = line.trim_start();
    if !stripped.starts_with('#') {
        return;
    }
    // Match any configured tag in one of `# TAG` / `#TAG` adjacency forms.
    // Demo cop: false-positive-tolerant by design (`# todoist:` would
    // false-match `TODO` if `TODO` were lowercased — kept simple).
    let Some(tag) = opts.tags.iter().find(|t| {
        line.contains(&format!("# {}", t.as_str())) || line.contains(&format!("#{}", t.as_str()))
    }) else {
        return;
    };

    let range = Range {
        start: line_start as u32,
        end: line_end as u32,
    };
    if opts.require_author && !line.contains("@author") {
        cx.emit_offense(range, &format!("{tag} comment lacks @author tag"), None);
    } else if !opts.require_author {
        // Demo path: emit a soft warning on every tag-bearing line so the
        // e2e fixture has something to assert on. See module doc comment.
        cx.emit_offense(
            range,
            &format!("{tag} comment detected (example demo cop)"),
            None,
        );
    }
}
