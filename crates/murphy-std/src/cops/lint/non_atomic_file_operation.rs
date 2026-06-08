//! `Lint/NonAtomicFileOperation` — Checks for non-atomic file operations guarded
//! by a file existence check (TOCTOU race condition).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NonAtomicFileOperation
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Known v1 limitations: (1) Autocorrect does not insert `mode:` keyword when
//!   `Dir.mkdir` has 2 arguments (would need `FileUtils.mkdir_p(path, mode: 0o755)`).
//!   (2) Offense range for the existence check in modifier form may include the
//!   leading body expression rather than just the keyword-to-condition span.
//!   (3) The cop fires on all files; RuboCop's per-file Include/Exclude gating
//!   is not yet available in Murphy v1. Covers all upstream shapes: unless/if/elsif,
//!   modifier forms, negated conditions, force: true/false options, fully-qualified
//!   constant names (::FileTest, ::FileUtils), and all file operation methods
//!   (mkdir, remove, delete, unlink, rm, rmdir, remove_dir, remove_entry, etc.).
//!   Force methods (makedirs, mkdir_p, mkpath, rm_f, rm_rf) trigger only the
//!   existence-check offense. Complex conditionals (&&, ||) and if-with-else
//!   are correctly excluded.
//! ```
//!
//! ## Matched shapes
//!
//! - `unless FileTest.exist?(path); FileUtils.mkdir(path); end`
//! - `if FileTest.exist?(path); FileUtils.remove(path); end`
//! - `if !FileTest.exist?(path); FileUtils.makedirs(path); end`
//! - `FileUtils.mkdir(path) unless FileTest.exist?(path)` (modifier form)
//! - `FileUtils.remove(path) if FileTest.exist?(path)` (modifier form)
//! - `elsif FileTest.exist?(path); FileUtils.rm_f(path); end`
//! - `::FileTest.exist?(path)` / `::FileUtils.mkdir(path)` (fully-qualified)
//! - `File.exist?`, `Dir.exist?`, `Shell.exist?`, and `exists?` alias
//!
//! ## Autocorrect
//!
//! Replaces non-atomic method with atomic equivalent:
//! - `mkdir` → `mkdir_p`
//! - `remove`/`delete`/`unlink`/`remove_file`/`rm`/`rmdir`/`safe_unlink` → `rm_f`
//! - `remove_dir`/`remove_entry`/`remove_entry_secure` → `rm_rf`
//!
//! Removes the existence-check condition and the closing `end` (block form)
//! or the modifier keyword (modifier form). Autocorrect is suppressed for
//! `elsif` conditions. Force methods (makedirs, mkdir_p, mkpath, rm_f, rm_rf)
//! and `force: true` option suppress the method-replacement offense but not
//! the existence-check offense.
//!
//! ## Why this shape
//!
//! Mirrors RuboCop's `Lint/NonAtomicFileOperation` which subscribes to
//! `on_send` with `RESTRICT_ON_SEND` for file operation methods. The core
//! logic checks that a file operation Send node is a direct child of an
//! `if`/`unless`/`elsif` node whose condition contains a file existence
//! check (`FileTest.exist?`, `File.exist?`, `Dir.exist?`, `Shell.exist?`,
//! or `exists?` alias), and that both operate on the same path argument.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range};

// ── method sets ───────────────────────────────────────────────────────────────

/// File creation methods — replace with `mkdir_p`.
const MAKE_METHODS: &[&str] = &["mkdir"];
/// Already-force creation methods — no replacement needed.
const MAKE_FORCE_METHODS: &[&str] = &["makedirs", "mkdir_p", "mkpath"];
/// File removal methods — replace with `rm_f`.
const REMOVE_METHODS: &[&str] = &[
    "remove", "delete", "unlink", "remove_file", "rm", "rmdir", "safe_unlink",
];
/// Recursive removal methods — replace with `rm_rf`.
const RECURSIVE_REMOVE_METHODS: &[&str] = &["remove_dir", "remove_entry", "remove_entry_secure"];
/// Already-force removal methods — no replacement needed.
const REMOVE_FORCE_METHODS: &[&str] = &["rm_f", "rm_rf"];

// ── existence check constants ────────────────────────────────────────────────

/// Methods that constitute an existence check.
const EXIST_METHODS: &[&str] = &["exist?", "exists?"];
/// Receiver constant names for existence checks.
const EXIST_RECEIVERS: &[&str] = &["FileTest", "File", "Dir", "Shell"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NonAtomicFileOperation;

#[cop(
    name = "Lint/NonAtomicFileOperation",
    description = "Checks for non-atomic file operations guarded by a file existence check.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NonAtomicFileOperation {
    #[on_node(kind = "send", methods = ["mkdir", "makedirs", "mkdir_p", "mkpath", "remove", "delete", "unlink", "remove_file", "rm", "rmdir", "safe_unlink", "remove_dir", "remove_entry", "remove_entry_secure", "rm_f", "rm_rf"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
            ..
        } = *cx.kind(node)
        else {
            return;
        };

        // Receiver must be a constant (FileUtils, ::FileUtils, etc.)
        if !receiver
            .get()
            .is_some_and(|r| matches!(*cx.kind(r), NodeKind::Const { .. }))
        {
            return;
        }

        // Parent must be an If node (if/unless/elsif)
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        if !matches!(*cx.kind(parent), NodeKind::If { .. }) {
            return;
        }

        // Skip ternary (ternary is also If in Murphy)
        if cx.is_ternary(parent) {
            return;
        }

        // No explicit `else` branch allowed
        if cx.is_else(parent) {
            return;
        }

        // Condition must not be a complex conditional (&& / ||)
        let cond = cx.if_condition(parent);
        if let Some(cond_id) = cond.get()
            && matches!(
                *cx.kind(cond_id),
                NodeKind::And { .. } | NodeKind::Or { .. }
            )
        {
            return;
        }

        // No `force: false` option
        if has_explicit_not_force(node, cx) {
            return;
        }

        // Find existence check within the If node
        let Some(exist_node) = find_existence_check(parent, cx) else {
            return;
        };

        // First argument must match between the file operation and the exist check
        let file_args = cx.list(args);
        let exist_args = match *cx.kind(exist_node) {
            NodeKind::Send { args, .. } => cx.list(args),
            _ => return,
        };

        if file_args.is_empty() || exist_args.is_empty() {
            return;
        }

        if cx.raw_source(cx.range(file_args[0])) != cx.raw_source(cx.range(exist_args[0])) {
            return;
        }

        // Extract exist-check metadata
        let exist_method_sym;
        let exist_receiver_name;
        match *cx.kind(exist_node) {
            NodeKind::Send {
                method: m,
                receiver: r,
                ..
            } => {
                let Some(recv_id) = r.get() else { return };
                let NodeKind::Const { name: n, .. } = *cx.kind(recv_id) else {
                    return;
                };
                exist_method_sym = m;
                exist_receiver_name = cx.symbol_str(n).to_string();
            }
            _ => return,
        }

        let method_name = cx.symbol_str(method);
        let exist_method_name = cx.symbol_str(exist_method_sym);
        let is_force = is_force_method_name(method_name);

        // Offense 1: non-atomic file operation (only if not already a force method)
        if !is_force {
            let replacement = replacement_method(method_name);
            let msg = format!(
                "Use atomic file operation method `FileUtils.{replacement}`."
            );
            cx.emit_offense(cx.range(node), &msg, None);
        }

        // Offense 2: unnecessary existence check
        let kw_loc = cx.if_keyword_loc(parent);
        let cond_range = cx.range(cond.get().unwrap_or(exist_node));
        let check_range = Range {
            start: kw_loc.start,
            end: cond_range.end,
        };
        let msg = format!(
            "Remove unnecessary existence check `{exist_receiver_name}.{exist_method_name}`."
        );
        cx.emit_offense(check_range, &msg, None);

        // Autocorrect (suppressed for elsif)
        if cx.is_elsif(parent) {
            return;
        }

        // Autocorrect: replace method name
        if !is_force {
            emit_method_replacement(node, method_name, cx);
        }

        // Autocorrect: remove the existence-check condition
        emit_condition_removal(parent, check_range, node, cx);
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` when the method is already an atomic/force variant.
fn is_force_method_name(name: &str) -> bool {
    MAKE_FORCE_METHODS.contains(&name) || REMOVE_FORCE_METHODS.contains(&name)
}

/// Returns the replacement method name for a non-atomic file operation.
fn replacement_method(name: &str) -> &str {
    if MAKE_METHODS.contains(&name) {
        "mkdir_p"
    } else if REMOVE_METHODS.contains(&name) {
        "rm_f"
    } else if RECURSIVE_REMOVE_METHODS.contains(&name) {
        "rm_rf"
    } else {
        name
    }
}

/// Checks whether the file operation node has a `force: false` keyword argument.
fn has_explicit_not_force(node: NodeId, cx: &Cx<'_>) -> bool {
    for &desc in cx.descendants(node).iter() {
        if let NodeKind::Pair { key, value } = *cx.kind(desc)
            && let NodeKind::Sym(s) = *cx.kind(key)
            && cx.symbol_str(s) == "force"
            && matches!(*cx.kind(value), NodeKind::False_)
        {
            return true;
        }
    }
    false
}

/// Searches the If node for a file existence check (`FileTest.exist?`, etc.).
/// Returns `Some(exist_node_id)` if found.
fn find_existence_check(if_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    for &desc in cx.descendants(if_node).iter() {
        if let Some((node, _)) = as_existence_check(desc, cx) {
            return Some(node);
        }
    }
    // Also check the condition directly (it might be the first child but
    // descendants already covers it; this is a belt-and-suspenders check).
    let cond = cx.if_condition(if_node);
    if let Some(cond_id) = cond.get()
        && let Some((node, _)) = as_existence_check(cond_id, cx)
    {
        return Some(node);
    }
    None
}

/// If `node` is a `FileTest.exist?` / `File.exist?` / etc. call, returns
/// `Some((node_id, receiver_name_str))`.
fn as_existence_check<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<(NodeId, &'a str)> {
    let (method, receiver) = match *cx.kind(node) {
        NodeKind::Send {
            receiver, method, ..
        } => (method, receiver),
        NodeKind::Csend {
            receiver, method, ..
        } => (method, OptNodeId::from(Some(receiver))),
        _ => return None,
    };

    let method_str = cx.symbol_str(method);
    if !EXIST_METHODS.contains(&method_str) {
        return None;
    }

    let recv = receiver.get()?;

    let NodeKind::Const { name, scope } = *cx.kind(recv) else {
        return None;
    };

    let receiver_name = cx.symbol_str(name);
    if !EXIST_RECEIVERS.contains(&receiver_name) {
        return None;
    }

    // Scope must be nil (bare `FileTest`) or cbase (`::FileTest`)
    if let Some(s) = scope.get()
        && !matches!(*cx.kind(s), NodeKind::Cbase)
    {
        return None;
    }

    Some((node, receiver_name))
}

// ── autocorrect helpers ──────────────────────────────────────────────────────

/// Emit an edit to replace the method name with the atomic equivalent.
fn emit_method_replacement(node: NodeId, current_name: &str, cx: &Cx<'_>) {
    let replacement = replacement_method(current_name);
    // Find the method name within the node's source range.
    let r = cx.range(node);
    let src = cx.raw_source(r);
    // Find the method name token: after the last `.` separator or at the start.
    let method_range = if let Some(dot) = src.rfind('.') {
        Range {
            start: r.start + (dot + 1) as u32,
            end: r.start + (dot + 1 + current_name.len()) as u32,
        }
    } else if let Some(open_paren) = src.find('(') {
        // Bare method call with parens: `mkdir(path)` → method is before `(`
        Range {
            start: r.start,
            end: r.start + open_paren as u32,
        }
    } else if let Some(space) = src.find(' ') {
        // Bare method call with space: `mkdir path` → method is before space
        Range {
            start: r.start,
            end: r.start + space as u32,
        }
    } else {
        // Bare method call with no args: `mkdir`
        Range {
            start: r.start,
            end: r.end,
        }
    };
    cx.emit_edit(method_range, replacement);
}

/// Emit edits to remove the existence-check condition range from the source.
///
/// For block-form (has `end`): removes the condition range and the closing `end`.
/// For modifier-form (no `end`): removes the condition range and the keyword
/// plus leading space (e.g. ` unless ...`).
fn emit_condition_removal(
    parent: NodeId,
    check_range: Range,
    file_op_node: NodeId,
    cx: &Cx<'_>,
) {
    // Remove the condition range (e.g. `unless FileTest.exist?(path)`)
    cx.emit_edit(check_range, "");

    if cx.is_modifier_form(parent) {
        // Modifier form: also remove ` unless` (space + keyword) before the
        // condition. The condition range starts at the keyword, so the gap
        // between the body and the keyword needs removal too.
        // Compute range from end of file operation to start of keyword.
        let file_op_end = cx.range(file_op_node).end;
        let kw_start = cx.if_keyword_loc(parent).start;
        if file_op_end <= kw_start {
            cx.emit_edit(
                Range {
                    start: file_op_end,
                    end: kw_start,
                },
                "",
            );
        }
    } else {
        // Block form: also remove the closing `end` keyword.
        let end_range = cx.loc(parent).end_keyword();
        if end_range != Range::ZERO {
            cx.emit_edit(end_range, "");
        }
    }
}

murphy_plugin_api::submit_cop!(NonAtomicFileOperation);

#[cfg(test)]
mod tests {
    use super::NonAtomicFileOperation;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── basic creation patterns ──────────────────────────────────────────

    #[test]
    fn flags_unless_exist_before_mkdir() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.mkdir(path)
              ^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
            end
        "#});
    }

    #[test]
    fn flags_if_exist_before_remove() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.remove(path)
              ^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    // ── force make methods ───────────────────────────────────────────────

    #[test]
    fn flags_exist_before_makedirs_no_method_offense() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.makedirs(path)
            end
        "#});
    }

    #[test]
    fn flags_exist_before_mkdir_p_no_method_offense() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.mkdir_p(path)
            end
        "#});
    }

    #[test]
    fn flags_exist_before_mkpath_no_method_offense() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.mkpath(path)
            end
        "#});
    }

    // ── remove methods ───────────────────────────────────────────────────

    #[test]
    fn flags_exist_before_delete() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.delete(path)
              ^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    #[test]
    fn flags_exist_before_unlink() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.unlink(path)
              ^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    // ── recursive remove methods ─────────────────────────────────────────

    #[test]
    fn flags_exist_before_remove_dir() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.remove_dir(path)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_rf`.
            end
        "#});
    }

    #[test]
    fn flags_exist_before_remove_entry() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.remove_entry(path)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_rf`.
            end
        "#});
    }

    // ── force remove methods ─────────────────────────────────────────────

    #[test]
    fn flags_exist_before_rm_f_no_method_offense() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.rm_f(path)
            end
        "#});
    }

    #[test]
    fn flags_exist_before_rm_rf_no_method_offense() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.rm_rf(path)
            end
        "#});
    }

    // ── non-flagged recursive methods ────────────────────────────────────

    #[test]
    fn accepts_rm_r_before_exist() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if FileTest.exist?(path)
              FileUtils.rm_r(path)
            end
        "#});
    }

    #[test]
    fn accepts_rmtree_before_exist() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if FileTest.exist?(path)
              FileUtils.rmtree(path)
            end
        "#});
    }

    // ── force: true ──────────────────────────────────────────────────────

    #[test]
    fn flags_exist_before_makedirs_force_true() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exists?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exists?`.
              FileUtils.makedirs(path, force: true)
            end
        "#});
    }

    // ── force: false (excluded) ──────────────────────────────────────────

    #[test]
    fn accepts_force_false() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            unless FileTest.exists?(path)
              FileUtils.makedirs(path, force: false)
            end
        "#});
    }

    // ── force: not present (not false) ───────────────────────────────────

    #[test]
    fn flags_exist_before_makedirs_with_verbose() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exists?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exists?`.
              FileUtils.makedirs(path, verbose: true)
            end
        "#});
    }

    // ── exists? alias ────────────────────────────────────────────────────

    #[test]
    fn flags_exists_alias() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            unless FileTest.exists?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exists?`.
              FileUtils.makedirs(path)
            end
        "#});
    }

    // ── negated if ───────────────────────────────────────────────────────

    #[test]
    fn flags_negated_if_exist_before_makedirs() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if !FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.makedirs(path)
            end
        "#});
    }

    // ── modifier forms ───────────────────────────────────────────────────

    #[test]
    fn flags_modifier_unless_mkdir() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            FileUtils.mkdir(path) unless FileTest.exist?(path)
                                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
            ^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
        "#});
    }

    #[test]
    fn flags_modifier_if_remove() {
        use murphy_plugin_api::test_support::run_cop;
        let offenses = run_cop::<NonAtomicFileOperation>(
            "FileUtils.remove(path) if FileTest.exist?(path)\n",
        );
        assert_eq!(offenses.len(), 2, "should have 2 offenses");
        assert!(
            offenses.iter().any(|o| o.message.contains("Use atomic file operation method")),
            "should have method replacement offense"
        );
        assert!(
            offenses.iter().any(|o| o.message.contains("Remove unnecessary existence check")),
            "should have existence check offense"
        );
    }

    // ── line-break modifier forms ────────────────────────────────────────

    #[test]
    fn flags_modifier_unless_mkdir_line_break() {
        use murphy_plugin_api::test_support::run_cop;
        let offenses = run_cop::<NonAtomicFileOperation>(
            "FileUtils.mkdir(path) unless\n  FileTest.exist?(path)\n",
        );
        assert_eq!(offenses.len(), 2, "should have 2 offenses");
        assert!(
            offenses.iter().any(|o| o.message.contains("Use atomic file operation method `FileUtils.mkdir_p`")),
            "should have mkdir_p suggestion"
        );
        assert!(
            offenses.iter().any(|o| o.message.contains("Remove unnecessary existence check")),
            "should have existence check offense"
        );
    }

    #[test]
    fn flags_modifier_unless_mkdir_paren_wrapped_line_break() {
        use murphy_plugin_api::test_support::run_cop;
        let offenses = run_cop::<NonAtomicFileOperation>(
            "FileUtils.mkdir(path) unless (\n  FileTest.exist?(path))\n",
        );
        assert_eq!(offenses.len(), 2, "should have 2 offenses");
        assert!(
            offenses.iter().any(|o| o.message.contains("Use atomic file operation method `FileUtils.mkdir_p`")),
            "should have mkdir_p suggestion"
        );
        assert!(
            offenses.iter().any(|o| o.message.contains("Remove unnecessary existence check")),
            "should have existence check offense"
        );
    }

    // ── fully-qualified constant names ───────────────────────────────────

    #[test]
    fn flags_cbase_exist_check() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if ::FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.delete(path)
              ^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    #[test]
    fn flags_cbase_file_operation() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              ::FileUtils.delete(path)
              ^^^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    #[test]
    fn flags_both_cbase() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if ::FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              ::FileUtils.delete(path)
              ^^^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
            end
        "#});
    }

    // ── elsif ────────────────────────────────────────────────────────────

    #[test]
    fn flags_elsif_exist_check() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            if condition
              do_something
            elsif FileTest.exist?(path)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
              FileUtils.rm_f path
            end
        "#});
    }

    // ── Dir receiver ─────────────────────────────────────────────────────

    #[test]
    fn flags_dir_exist_before_dir_mkdir() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            Dir.mkdir(path) unless Dir.exist?(path)
                            ^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `Dir.exist?`.
            ^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
        "#});
    }

    #[test]
    fn flags_dir_mkdir_two_args() {
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            Dir.mkdir(path, 0o0755) unless Dir.exist?(path)
                                    ^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `Dir.exist?`.
            ^^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
        "#});
    }

    // ── no-offense cases ─────────────────────────────────────────────────

    #[test]
    fn accepts_no_exist_check() {
        test::<NonAtomicFileOperation>().expect_no_offenses("FileUtils.mkdir_p(path)\n");
    }

    #[test]
    fn accepts_different_files() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.mkdir_p(y) unless FileTest.exist?(path)
        "#});
    }

    #[test]
    fn accepts_non_file_operation() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            unless FileUtils.exist?(path)
              FileUtils.options_of(:rm)
            end
            unless FileUtils.exist?(path)
              NotFile.remove(path)
            end
        "#});
    }

    #[test]
    fn accepts_non_exist_check() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            unless FileUtils.options_of(:rm)
              FileUtils.mkdir_p(path)
            end
            if FileTest.executable?(path)
              FileUtils.remove(path)
            end
        "#});
    }

    #[test]
    fn accepts_other_processing_in_body() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            unless FileTest.exist?(path)
              FileUtils.makedirs(path)
              do_something
            end

            unless FileTest.exist?(path)
              do_something
              FileUtils.makedirs(path)
            end
        "#});
    }

    #[test]
    fn accepts_if_with_else() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if FileTest.exist?(path)
              FileUtils.mkdir(path)
            else
              do_something
            end
        "#});
    }

    #[test]
    fn accepts_complex_conditional_and() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if FileTest.exist?(path) && File.stat(path).socket?
              FileUtils.mkdir(path)
            end
        "#});
    }

    #[test]
    fn accepts_complex_conditional_or() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if FileTest.exist?(path) || condition
              FileUtils.mkdir(path)
            end
        "#});
    }

    #[test]
    fn accepts_no_explicit_receiver() {
        // No receiver → not flagged (RuboCop checks for const receiver)
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            mkdir(path) unless FileTest.exist?(path)
        "#});
    }

    #[test]
    fn accepts_non_constant_receiver() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            storage[:files].delete(file) unless File.exists?(file)
        "#});
    }

    // ── autocorrect: basic block form ───────────────────────────────────

    #[test]
    fn corrects_unless_exist_before_mkdir() {
        test::<NonAtomicFileOperation>().expect_correction(
            indoc! {r#"
                unless FileTest.exist?(path)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
                  FileUtils.mkdir(path)
                  ^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
                end
            "#},
            "\n  FileUtils.mkdir_p(path)\n\n",
        );
    }

    #[test]
    fn corrects_if_exist_before_remove() {
        test::<NonAtomicFileOperation>().expect_correction(
            indoc! {r#"
                if FileTest.exist?(path)
                ^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
                  FileUtils.remove(path)
                  ^^^^^^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
                end
            "#},
            "\n  FileUtils.rm_f(path)\n\n",
        );
    }

    #[test]
    fn corrects_modifier_unless() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let run = run_cop_with_edits::<NonAtomicFileOperation>(
            "FileUtils.mkdir(path) unless FileTest.exist?(path)\n",
        );
        assert_eq!(run.offenses.len(), 2, "should have 2 offenses");
        assert_eq!(run.edits.len(), 3, "should have 3 edits (method, condition, gap)");
        // Check the method replacement edit
        assert!(
            run.edits.iter().any(|e| e.replacement == "mkdir_p"),
            "should replace mkdir with mkdir_p"
        );
    }

    #[test]
    fn corrects_modifier_if() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let run = run_cop_with_edits::<NonAtomicFileOperation>(
            "FileUtils.remove(path) if FileTest.exist?(path)\n",
        );
        assert_eq!(run.offenses.len(), 2, "should have 2 offenses");
        assert_eq!(run.edits.len(), 3, "should have 3 edits (method, condition, gap)");
        assert!(
            run.edits.iter().any(|e| e.replacement == "rm_f"),
            "should replace remove with rm_f"
        );
    }

    #[test]
    fn no_corrections_for_elsif() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let run = run_cop_with_edits::<NonAtomicFileOperation>(
            "if condition\n  do_something\nelsif FileTest.exist?(path)\n  FileUtils.rm_f path\nend\n",
        );
        assert!(!run.offenses.is_empty(), "should have offense");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits for elsif");
    }
}
