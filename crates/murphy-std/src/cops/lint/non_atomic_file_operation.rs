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

/// Receiver constant names that are valid for file operations (FileUtils, Dir, File).
/// Only operations on these receivers are flagged and autocorrected.
const OPERATION_RECEIVERS: &[&str] = &["FileUtils", "Dir", "File"];

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

        // Receiver must be a known file-utility constant (FileUtils, Dir, File).
        if !is_valid_operation_receiver(receiver, cx) {
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

        // Condition must not be a complex conditional (&& / ||) anywhere
        // in the condition subtree (handles negated compounds like
        // `!(exist? || disabled)`).
        let cond = cx.if_condition(parent);
        if let Some(cond_id) = cond.get() {
            if matches!(*cx.kind(cond_id), NodeKind::And { .. } | NodeKind::Or { .. }) {
                return;
            }
            // Check inside negation wrappers too.
            for &desc in cx.descendants(cond_id).iter() {
                if desc == cond_id {
                    continue;
                }
                if matches!(*cx.kind(desc), NodeKind::And { .. } | NodeKind::Or { .. }) {
                    return;
                }
            }
        }

        // No `force: false` option
        if has_explicit_not_force(node, cx) {
            return;
        }

        // Check condition polarity: make ops need "not exists" (unless/negated if);
        // remove ops need "exists" (bare if).
        let method_name = cx.symbol_str(method);
        if !condition_matches_method(parent, method_name, cx) {
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

        // Autocorrect (suppressed for elsif, then-forms, and Dir.mkdir with 2+ args)
        let has_too_many_args = method_name == "mkdir"
            && is_dir_receiver(receiver, cx)
            && cx.list(args).len() >= 2;
        if cx.is_elsif(parent) || cx.is_then(parent) || has_too_many_args {
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

/// Returns `true` when the receiver (OptNodeId) is a top-level
/// file-utility constant: `FileUtils`, `Dir`, or `File` (or their
/// `::`-prefixed fully-qualified forms).
///
/// Murphy normalises `::X` to `Const { scope: None, name: X }`, same as
/// bare `X`, so both `FileUtils` and `::FileUtils` match. Nested scopes
/// like `Foo::FileUtils` are rejected — they may define different methods.
fn is_valid_operation_receiver(receiver: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    let NodeKind::Const { name, scope } = *cx.kind(recv_id) else {
        return false;
    };
    // scope must be None (top-level or cbase `::X`). Nested `Foo::FileUtils`
    // is rejected because the inner class may not have the expected methods.
    if scope.get().is_some() {
        return false;
    }
    OPERATION_RECEIVERS.contains(&cx.symbol_str(name))
}

/// Returns `true` when the receiver is the `Dir` constant.
fn is_dir_receiver(receiver: OptNodeId, cx: &Cx<'_>) -> bool {
    receiver.get().is_some_and(|recv| {
        if let NodeKind::Const { name, scope } = *cx.kind(recv) {
            scope.get().is_none() && cx.symbol_str(name) == "Dir"
        } else {
            false
        }
    })
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

/// Returns `true` when the condition polarity matches the file operation type:
/// - Make ops (`mkdir`): must be `unless` or negated `if` (= "not exists")
/// - Remove ops (`remove`, `delete`, etc.): must be bare `if` (= "exists")
/// Force methods use whichever polarity their non-force counterpart uses.
/// Returns `true` when the condition polarity matches the file operation type:
/// - Make ops (`mkdir`): body must execute when file does NOT exist
/// - Remove ops (`remove`, `delete`, etc.): body must execute when file DOES exist
///
/// Handles `unless !condition` (double-negative = body executes when condition true).
fn condition_matches_method(if_node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    let keyword_src = cx.raw_source(cx.if_keyword_loc(if_node));
    let is_negated = is_negated_condition(if_node, cx);

    // Determine whether the body executes when the condition is TRUE.
    // if/elsif cond     → body when cond true
    // if/elsif !cond    → body when cond FALSE
    // unless cond       → body when cond FALSE
    // unless !cond      → body when cond TRUE (double negative)
    let body_executes_when_exists = match (keyword_src, is_negated) {
        ("if", false) | ("elsif", false) => true,
        ("if", true) | ("elsif", true) => false,
        ("unless", false) => false,
        ("unless", true) => true,
        _ => return false,
    };

    let is_make = MAKE_METHODS.contains(&method_name) || MAKE_FORCE_METHODS.contains(&method_name);
    let is_remove = REMOVE_METHODS.contains(&method_name)
        || RECURSIVE_REMOVE_METHODS.contains(&method_name)
        || REMOVE_FORCE_METHODS.contains(&method_name);

    if is_make {
        // `mkdir` should fire when file does NOT exist
        !body_executes_when_exists
    } else if is_remove {
        // `remove` should fire when file DOES exist
        body_executes_when_exists
    } else {
        true
    }
}

/// Returns `true` when the condition of the `If` node is negated — i.e. the
/// top-level operator chain is `!` with an odd number of `!` calls (so `!x`
/// is negated, `!!x` is not, `!!!x` is, etc.).
fn is_negated_condition(if_node: NodeId, cx: &Cx<'_>) -> bool {
    let mut cond = cx.if_condition(if_node);
    let mut count = 0u32;
    loop {
        let Some(cond_id) = cond.get() else {
            return count % 2 == 1;
        };
        let NodeKind::Send {
            receiver,
            method,
            args,
            ..
        } = *cx.kind(cond_id)
        else {
            return count % 2 == 1;
        };
        if cx.symbol_str(method) != "!" {
            return count % 2 == 1;
        }
        count += 1;
        let args_list = cx.list(args);
        if !args_list.is_empty() {
            cond = OptNodeId::some(args_list[0]);
        } else if let Some(recv) = receiver.get() {
            cond = OptNodeId::some(recv);
        } else {
            return count % 2 == 1;
        }
    }
}

/// Searches the `if`/`unless`/`elsif` condition for a file existence check
/// (`FileTest.exist?`, etc.). Only accepts existence checks that are the
/// condition itself or directly negated. Rejects checks nested inside other
/// method call arguments.
fn find_existence_check(if_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let cond = cx.if_condition(if_node);
    let Some(cond_id) = cond.get() else {
        return None;
    };

    // Check if the condition node itself is an existence check.
    if let Some((node, _)) = as_existence_check(cond_id, cx) {
        return Some(node);
    }

    // Walk descendants. Only accept existence checks that are NOT inside
    // another method call's argument list.
    for &desc in cx.descendants(cond_id).iter() {
        if desc == cond_id {
            continue;
        }
        if let Some((node, _)) = as_existence_check(desc, cx) {
            // Check if node is an argument to some enclosing Send/Csend
            // that is not the negation operator `!`.
            if let Some(parent) = cx.parent(node).get() {
                match *cx.kind(parent) {
                    NodeKind::Send { args, method, .. } | NodeKind::Csend { args, method, .. } => {
                        let method_str = cx.symbol_str(method);
                        // Allow `!exist?` (negation) and `exist?` itself.
                        if method_str != "!" {
                            let in_args = cx.list(args).iter().any(|&a| a == node);
                            if in_args {
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
            }
            return Some(node);
        }
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
    let r = cx.range(node);
    let src = cx.raw_source(r);

    // When the receiver is `Dir` or `File` (not `FileUtils`), replace the whole
    // receiver.method expression with `FileUtils.<replacement>` so the autocorrect
    // produces a working call (e.g. `Dir.mkdir(path)` → `FileUtils.mkdir_p(path)`).
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else { return };
    let needs_receiver_replace = receiver.get().is_some_and(|recv| {
        if let NodeKind::Const { name, scope } = *cx.kind(recv) {
            if scope.get().is_none() {
                let n = cx.symbol_str(name);
                return n == "Dir" || n == "File";
            }
        }
        false
    });

    if needs_receiver_replace {
        // Find the receiver+dot+method portion and replace with FileUtils.replacement.
        // The method name starts after the last `(` or ` ` or `.`.
        let method_start = if let Some(dot) = src.rfind('.') {
            r.start + dot as u32 + 1
        } else if let Some(open_paren) = src.find('(') {
            r.start
        } else if let Some(space) = src.find(' ') {
            r.start
        } else {
            r.start
        };
        let method_end = method_start + current_name.len() as u32;
        let edit_range = Range { start: r.start, end: method_end };
        cx.emit_edit(edit_range, &format!("FileUtils.{replacement}"));
    } else {
        // Standard: just replace the method name.
        let method_range = if let Some(dot) = src.rfind('.') {
            Range {
                start: r.start + (dot + 1) as u32,
                end: r.start + (dot + 1 + current_name.len()) as u32,
            }
        } else if let Some(open_paren) = src.find('(') {
            Range {
                start: r.start,
                end: r.start + open_paren as u32,
            }
        } else if let Some(space) = src.find(' ') {
            Range {
                start: r.start,
                end: r.start + space as u32,
            }
        } else {
            Range {
                start: r.start,
                end: r.end,
            }
        };
        cx.emit_edit(method_range, replacement);
    }
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
    fn accepts_make_with_if_exists() {
        // mkdir guarded by `if exist?` — wrong polarity for make ops,
        // mkdir_p would create if not exists, changing behaviour.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.mkdir(path) if FileTest.exist?(path)
        "#});
    }

    #[test]
    fn accepts_make_with_double_negated_if() {
        // `if !!exist?` = `if exist?` (double negation cancels) —
        // wrong polarity for make ops.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.makedirs(path) if !!FileTest.exist?(path)
        "#});
    }

    #[test]
    fn flags_make_with_triple_negated_if() {
        // `if !!!exist?` = `if !exist?` (triple = single negation) —
        // correct polarity for make ops. makedirs is already a force method,
        // so only the existence-check offense fires.
        test::<NonAtomicFileOperation>().expect_offense(indoc! {r#"
            FileUtils.makedirs(path) if !!!FileTest.exist?(path)
                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `FileTest.exist?`.
        "#});
    }

    #[test]
    fn accepts_remove_with_unless_exists() {
        // remove guarded by `unless exist?` — wrong polarity for remove ops.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.remove(path) unless FileTest.exist?(path)
        "#});
    }

    #[test]
    fn accepts_makedirs_with_if_exists() {
        // force make method but wrong polarity.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.makedirs(path) if FileTest.exist?(path)
        "#});
    }

    #[test]
    fn accepts_rm_f_with_unless_exists() {
        // force remove method but wrong polarity.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.rm_f(path) unless FileTest.exist?(path)
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
    fn accepts_negated_complex_conditional() {
        // !(exist? || disabled) is still complex — should not trigger.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            unless !(FileTest.exist?(path) || disabled)
              FileUtils.makedirs(path)
            end
        "#});
    }

    #[test]
    fn accepts_exist_check_as_method_argument() {
        // allowed?(File.exist?(path)) — the existence check is a sub-expression,
        // not the actual condition. Should not trigger.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if allowed?(File.exist?(path))
              FileUtils.remove(path)
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

    #[test]
    fn accepts_non_file_utility_constant_receiver() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            Storage.remove(path) if File.exist?(path)
        "#});
    }

    #[test]
    fn accepts_nested_constant_receiver() {
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            MyApp::FileUtils.remove(path) if File.exist?(path)
        "#});
    }

    #[test]
    fn accepts_exist_check_in_body_not_condition() {
        // Existence check inside the operation's arguments (body side) should
        // not trigger the cop — only the if/unless condition counts.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            if enabled
              FileUtils.remove(path, verbose: File.exist?(other))
            end
        "#});
    }

    #[test]
    fn accepts_make_with_unless_negated() {
        // `unless !exist?` means "execute when file exists" — wrong for make ops.
        test::<NonAtomicFileOperation>().expect_no_offenses(indoc! {r#"
            FileUtils.mkdir(path) unless !FileTest.exist?(path)
        "#});
    }

    #[test]
    fn flags_remove_with_unless_negated() {
        // `unless !exist?` means "execute when file exists" — correct for remove ops.
        use murphy_plugin_api::test_support::run_cop;
        let offenses = run_cop::<NonAtomicFileOperation>(
            "FileUtils.remove(path) unless !FileTest.exist?(path)\n",
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

    #[test]
    fn no_corrections_for_then_form() {
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let run = run_cop_with_edits::<NonAtomicFileOperation>(
            "if FileTest.exist?(path) then FileUtils.remove(path) end\n",
        );
        assert!(!run.offenses.is_empty(), "should have offense");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits for then-form");
    }

    #[test]
    fn corrects_dir_mkdir_to_fileutils_mkdir_p() {
        test::<NonAtomicFileOperation>().expect_correction(
            indoc! {r#"
                Dir.mkdir(path) unless Dir.exist?(path)
                ^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.mkdir_p`.
                                ^^^^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `Dir.exist?`.
            "#},
            "FileUtils.mkdir_p(path)\n",
        );
    }

    #[test]
    fn corrects_file_delete_to_fileutils_rm_f() {
        test::<NonAtomicFileOperation>().expect_correction(
            indoc! {r#"
                File.delete(path) if File.exist?(path)
                ^^^^^^^^^^^^^^^^^ Use atomic file operation method `FileUtils.rm_f`.
                                  ^^^^^^^^^^^^^^^^^^^^ Remove unnecessary existence check `File.exist?`.
            "#},
            "FileUtils.rm_f(path)\n",
        );
    }

    #[test]
    fn no_corrections_for_dir_mkdir_with_mode() {
        // Dir.mkdir with 2 args: all autocorrect suppressed because
        // FileUtils.mkdir_p expects keyword mode: not positional integer,
        // and removing the guard would make the code unsafe.
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let run = run_cop_with_edits::<NonAtomicFileOperation>(
            "Dir.mkdir(path, 0o755) unless Dir.exist?(path)\n",
        );
        assert!(!run.offenses.is_empty(), "should have offenses");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits for Dir.mkdir with mode arg");
    }
}
