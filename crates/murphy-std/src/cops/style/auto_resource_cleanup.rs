//! `Style/AutoResourceCleanup` — suggest using the block version of `open` for resource cleanup.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AutoResourceCleanup
//! upstream_version_checked: 1.86.2
//! version_added: "0.30"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `File.open` and `Tempfile.open` (nil- or cbase-scoped) when not
//!   already in block form. The offense fires when the call has no block-pass
//!   argument and its parent is either absent (top-level expression) or an
//!   `lvasgn` node. All four spec cases are covered: plain, Tempfile,
//!   ::File, ::Tempfile. No autocorrect — RuboCop does not ship one.
//!   Disabled by default (RuboCop ships with Enabled: false).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! f = File.open('file')
//! File.open('file')
//! f = Tempfile.open('temp')
//! f = ::File.open('file')
//!
//! # good
//! File.open('file') { |f| ... }
//! File.open('file', &:read)
//! File.open('file', 'w', 0o777).close
//! @f = File.open('file')   # not lvasgn, no offense
//! ```
//!
//! ## No autocorrect
//!
//! RuboCop does not provide an autocorrect for this cop — the block body is
//! user-supplied and cannot be inferred statically.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct AutoResourceCleanup;

/// Receiver class names that must use the block form.
const RESOURCE_CLASSES: &[&str] = &["File", "Tempfile"];

#[cop(
    name = "Style/AutoResourceCleanup",
    description = "Suggest using the block version of `open` for automatic resource cleanup.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl AutoResourceCleanup {
    #[on_node(kind = "send", methods = ["open"])]
    fn check_open(&self, node: NodeId, cx: &Cx<'_>) {
        // Receiver must be File or Tempfile (nil- or cbase-scoped).
        let Some(recv) = cx.call_receiver(node).get() else {
            return;
        };
        let Some(class_name) = match_resource_class(recv, cx) else {
            return;
        };

        // Skip if already using a block-pass argument (&blk, &:read, etc.).
        let args = cx.call_arguments(node);
        if args
            .last()
            .is_some_and(|&a| matches!(cx.kind(a), NodeKind::BlockPass(_)))
        {
            return;
        }

        // Check the parent: offense fires only when parent is absent or is lvasgn.
        // Mirrors RuboCop's `cleanup?`: returns true (no offense) when parent is a
        // block type or is NOT lvasgn. Returns false (offense) when parent is nil
        // or parent is lvasgn.
        let is_cleanup = match cx.parent(node).get() {
            None => false, // top-level / root — NOT cleanup → offense
            Some(p) => cx.is_any_block_type(p) || !matches!(cx.kind(p), NodeKind::Lvasgn { .. }),
        };
        if is_cleanup {
            return;
        }

        let msg = format!("Use the block version of `{class_name}.open`.");
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

/// Returns the source text for the receiver (e.g. `"File"`, `"::Tempfile"`) if
/// the const is one of [`RESOURCE_CLASSES`] at top-level scope (nil or cbase).
/// Returns `None` otherwise.
fn match_resource_class<'a>(recv: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    let NodeKind::Const { name, scope } = *cx.kind(recv) else {
        return None;
    };
    let class_str = cx.symbol_str(name);
    if !RESOURCE_CLASSES.contains(&class_str) {
        return None;
    }
    // Scope must be absent (nil) or cbase (::).
    match scope.get() {
        None => Some(cx.raw_source(cx.range(recv))),
        Some(p) if matches!(cx.kind(p), NodeKind::Cbase) => Some(cx.raw_source(cx.range(recv))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::AutoResourceCleanup;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense cases ---

    #[test]
    fn flags_file_open_without_block() {
        test::<AutoResourceCleanup>().expect_offense(indoc! {r#"
            File.open("filename")
            ^^^^^^^^^^^^^^^^^^^^^ Use the block version of `File.open`.
        "#});
    }

    #[test]
    fn flags_tempfile_open_without_block() {
        test::<AutoResourceCleanup>().expect_offense(indoc! {r#"
            Tempfile.open("filename")
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Use the block version of `Tempfile.open`.
        "#});
    }

    #[test]
    fn flags_qualified_file_open_without_block() {
        test::<AutoResourceCleanup>().expect_offense(indoc! {r#"
            ::File.open("filename")
            ^^^^^^^^^^^^^^^^^^^^^^^ Use the block version of `::File.open`.
        "#});
    }

    #[test]
    fn flags_qualified_tempfile_open_without_block() {
        test::<AutoResourceCleanup>().expect_offense(indoc! {r#"
            ::Tempfile.open("filename")
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the block version of `::Tempfile.open`.
        "#});
    }

    #[test]
    fn flags_lvasgn_file_open() {
        test::<AutoResourceCleanup>().expect_offense(indoc! {r#"
            f = File.open("file")
                ^^^^^^^^^^^^^^^^^ Use the block version of `File.open`.
        "#});
    }

    // --- no-offense cases ---

    #[test]
    fn accepts_file_open_with_block() {
        test::<AutoResourceCleanup>().expect_no_offenses("File.open(\"file\") { |f| something }\n");
    }

    #[test]
    fn accepts_file_open_with_block_pass() {
        test::<AutoResourceCleanup>().expect_no_offenses("File.open(\"file\", &:read)\n");
    }

    #[test]
    fn accepts_file_open_with_immediate_close() {
        test::<AutoResourceCleanup>()
            .expect_no_offenses("File.open(\"file\", \"w\", 0o777).close\n");
    }

    #[test]
    fn accepts_ivasgn_file_open() {
        // ivasgn is not lvasgn — RuboCop's cleanup? returns true for non-lvasgn parents.
        test::<AutoResourceCleanup>().expect_no_offenses("@f = File.open(\"file\")\n");
    }

    #[test]
    fn accepts_unknown_class_open() {
        test::<AutoResourceCleanup>().expect_no_offenses("io.open(\"file\")\n");
    }

    #[test]
    fn accepts_module_scoped_file_open() {
        // Foo::File is not top-scope.
        test::<AutoResourceCleanup>().expect_no_offenses("Foo::File.open(\"file\")\n");
    }
}

murphy_plugin_api::submit_cop!(AutoResourceCleanup);
