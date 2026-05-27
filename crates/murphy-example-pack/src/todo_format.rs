//! `Example/TodoFormat` — flags TODO/FIXME comments. Demo of:
//! ## Murphy catalog
//!
//! ```murphy-parity
//! cop: Example/TodoFormat
//! status: custom
//! notes: >
//!   Demo cop for plugin authors; no RuboCop upstream target.
//! ```
//!
//!
//! - `#[on_new_investigation]` — the macro's per-file investigation
//!   hook (RuboCop's `on_new_investigation` lookalike). Lowered to
//!   `KINDS = &[]` so the host calls the cop exactly once per file.
//! - `#[derive(CopOptions)]` with a `Vec<String>` and a `bool` option,
//!   showing the array-default literal syntax (`default = ["TODO",
//!   "FIXME"]`) and a boolean toggle.
//! - `cx.comments()` iteration: the right primitive for comment-shaped
//!   cops (RuboCop's `processed_source.comments` analogue). Raw-source
//!   scanning (`cx.source()`) is the escape-hatch path and is
//!   intentionally avoided here.
//!
//! ## Detection heuristic (substring, not anchored)
//!
//! Tag detection uses a plain `contains` after `#`-stripping —
//! intentionally false-positive-tolerant. Real packs likely want the
//! anchored "comment-start + tag + word-boundary" form instead; this
//! demo stays simple so the template reads as a template.
//!
//! ## Why the demo emits on every match (even with `require_author = false`)
//!
//! Real linters would normally treat `require_author = false` as "this
//! cop is fully relaxed and emits nothing"; the e2e test in
//! `crates/murphy-cli/tests/plugin_pack_e2e.rs` needs the cop to fire on
//! a vanilla `# TODO: ...` line so this demo intentionally still emits a
//! "tag detected" warning in that mode. Documented oddity, kept for the
//! e2e fixture.

use murphy_plugin_api::{Comment, CopOptions, Cx, cop};

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

#[cop(
    name = "Example/TodoFormat",
    description = "Check format of TODO/FIXME comments (optionally require @author tag).",
    default_severity = "warning",
    default_enabled = true,
    options = TodoFormatOptions
)]
impl TodoFormat {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // Runtime option access (murphy-9cr.9) is not yet wired through
        // `Cx`; v1 honours the `Default` (same staging as `StringLiterals`).
        // Until that lands, declaring e.g. `tags = ["XXX"]` in `murphy.toml`
        // for this cop is silently ignored. Plugin authors copying this
        // template should expect the same staging.
        let opts = TodoFormatOptions::default();
        for comment in cx.comments() {
            check_comment(cx, comment, &opts);
        }
    }
}

/// Inspect one comment and emit if it contains one of the configured tags.
///
/// Iterates per-comment rather than line-by-line: the cop now fires on
/// trailing comments too (`code # TODO foo` was previously skipped
/// because the line didn't start with `#`). For `CommentKind::Block`
/// (`=begin`…`=end`) the substring search runs over the whole block;
/// this is a demo cop, so the tolerated false-positive is fine.
fn check_comment(cx: &Cx<'_>, comment: &Comment, opts: &TodoFormatOptions) {
    // Targeted per-comment access; this is the read-bytes-for-one-Range
    // case `raw_source` is intended for, not the file-wide scan that
    // would be the escape-hatch use of `cx.source()`.
    let text = cx.raw_source(comment.range);
    // Strip the leading `#` (inline comment); Block comments fall
    // through with their `=begin` head intact, which the substring
    // search below tolerates.
    let body = text.strip_prefix('#').unwrap_or(text);
    let Some(tag) = opts.tags.iter().find(|t| {
        // `# TAG` and `#TAG` adjacency forms — case-sensitive, by design.
        body.contains(&format!(" {}", t.as_str())) || body.starts_with(t.as_str())
    }) else {
        return;
    };

    if opts.require_author && !text.contains("@author") {
        cx.emit_offense(
            comment.range,
            &format!("{tag} comment lacks @author tag"),
            None,
        );
    } else if !opts.require_author {
        // Demo path: emit a soft warning on every tag-bearing comment so
        // the e2e fixture has something to assert on. See module doc.
        cx.emit_offense(
            comment.range,
            &format!("{tag} comment detected (example demo cop)"),
            None,
        );
    }
}
