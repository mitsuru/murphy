//! `Style/FileRead` — favor `File.(bin)read` convenience methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FileRead
//! upstream_version_checked: 1.86.2
//! version_added: "1.24"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects three patterns:
//!     1. `File.open(f).read` — chained send (no-arg read after open)
//!     2. `File.open(f, &:read)` — block-pass on open
//!     3. `File.open(f) { |v| v.read }` — explicit single-arg block
//!   Binary variants with 'rb' mode → `File.binread`.
//!   Mode must be one of: r, rt, rb, r+, r+t, r+b (or absent, which defaults to 'r').
//!   Autocorrect replaces the range from the `open` selector start to the end of
//!   the outer expression with `read(filename)` or `binread(filename)`, keeping
//!   the `File.` prefix intact.
//!   Scope check: `File` must be at top scope (nil or cbase parent const).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad - text mode
//! File.open(filename).read
//! File.open(filename, &:read)
//! File.open(filename) { |f| f.read }
//! File.open(filename, 'r').read
//!
//! # bad - binary mode
//! File.open(filename, 'rb').read
//! File.open(filename, 'rb') { |f| f.read }
//!
//! # good
//! File.read(filename)
//! File.binread(filename)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces from the `open` selector to the end of the read node:
//! `File.open(f).read` → `File.read(f)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const READ_FILE_START_TO_FINISH_MODES: &[&str] = &["r", "rt", "rb", "r+", "r+t", "r+b"];

/// Stateless unit struct.
#[derive(Default)]
pub struct FileRead;

#[cop(
    name = "Style/FileRead",
    description = "Favor `File.(bin)read` convenience methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl FileRead {
    /// Pattern 1: `File.open(f).read` — the read call is a parent send.
    /// Pattern 2: `File.open(f, &:read)` — block-pass on the open call itself.
    #[on_node(kind = "send", methods = ["open"])]
    fn check_open(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_file_class(node, cx) {
            return;
        }

        let args = cx.call_arguments(node);

        // Pattern 2: last arg is `&:read` block-pass.
        if let Some(&last_arg) = args.last()
            && let NodeKind::BlockPass(inner_opt) = *cx.kind(last_arg)
            && let Some(sym_node) = inner_opt.get()
            && let NodeKind::Sym(sym) = *cx.kind(sym_node)
            && cx.symbol_str(sym) == "read"
        {
            let mode = extract_mode_from_open_args(args, cx);
            if mode.is_some_and(|m| !READ_FILE_START_TO_FINISH_MODES.contains(&m)) {
                return;
            }
            let read_method = if mode_is_binary(mode) { "binread" } else { "read" };
            if let Some(&filename_node) = args.first() {
                let filename_src = cx.raw_source(cx.range(filename_node)).to_string();
                let offense_range = Range {
                    start: cx.node(node).loc.name.start,
                    end: cx.range(node).end,
                };
                let msg = format!("Use `File.{read_method}`.");
                cx.emit_offense(offense_range, &msg, None);
                cx.emit_edit(offense_range, &format!("{read_method}({filename_src})"));
            }
            return;
        }

        // Pattern 1: `File.open(f).read` — check if parent is a `.read` call.
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        if let NodeKind::Send { method, .. } = *cx.kind(parent)
            && cx.symbol_str(method) == "read"
            && cx.call_arguments(parent).is_empty()
        {
            let mode = extract_mode_from_open_args(args, cx);
            if mode.is_some_and(|m| !READ_FILE_START_TO_FINISH_MODES.contains(&m)) {
                return;
            }
            let read_method = if mode_is_binary(mode) { "binread" } else { "read" };
            if let Some(&filename_node) = args.first() {
                let filename_src = cx.raw_source(cx.range(filename_node)).to_string();
                let offense_range = Range {
                    start: cx.node(node).loc.name.start,
                    end: cx.range(parent).end,
                };
                let msg = format!("Use `File.{read_method}`.");
                cx.emit_offense(offense_range, &msg, None);
                cx.emit_edit(offense_range, &format!("{read_method}({filename_src})"));
            }
        }
    }

    /// Pattern 3: `File.open(f) { |v| v.read }` — explicit block.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args: block_args, body } = *cx.kind(node) else {
            return;
        };

        if cx.method_name(call) != Some("open") || !is_file_class(call, cx) {
            return;
        }

        // body must be a single `v.read` send with no args.
        let Some(body_id) = body.get() else {
            return;
        };
        let NodeKind::Send { method: body_method, .. } = *cx.kind(body_id) else {
            return;
        };
        if cx.symbol_str(body_method) != "read" || !cx.call_arguments(body_id).is_empty() {
            return;
        }

        // The receiver of the body send must be the block parameter variable.
        let Some(body_recv) = cx.call_receiver(body_id).get() else {
            return;
        };
        let NodeKind::Lvar(body_recv_sym) = *cx.kind(body_recv) else {
            return;
        };

        // Block must have exactly one parameter that matches the receiver.
        let NodeKind::Args(arg_list_id) = *cx.kind(block_args) else {
            return;
        };
        let arg_list = cx.list(arg_list_id);
        if arg_list.len() != 1 {
            return;
        }
        let NodeKind::Arg(param_sym) = *cx.kind(arg_list[0]) else {
            return;
        };
        if param_sym != body_recv_sym {
            return;
        }

        // Mode check.
        let open_args = cx.call_arguments(call);
        let mode = extract_mode_from_open_args(open_args, cx);
        if mode.is_some_and(|m| !READ_FILE_START_TO_FINISH_MODES.contains(&m)) {
            return;
        }
        let read_method = if mode_is_binary(mode) { "binread" } else { "read" };
        if let Some(&filename_node) = open_args.first() {
            let filename_src = cx.raw_source(cx.range(filename_node)).to_string();
            let offense_range = Range {
                start: cx.node(call).loc.name.start,
                end: cx.range(node).end,
            };
            let msg = format!("Use `File.{read_method}`.");
            cx.emit_offense(offense_range, &msg, None);
            cx.emit_edit(offense_range, &format!("{read_method}({filename_src})"));
        }
    }
}

/// Returns true if the receiver of the send node is `File` (nil-scoped or cbase-scoped).
fn is_file_class(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_global_const(recv, "File")
}

/// Extracts the mode string from `File.open` call arguments.
/// Mode is the second arg (index 1) if it's a plain string (not a block-pass).
fn extract_mode_from_open_args<'a>(args: &[NodeId], cx: &'a Cx<'_>) -> Option<&'a str> {
    let candidate = args.get(1)?;
    if matches!(cx.kind(*candidate), NodeKind::BlockPass(_)) {
        return None;
    }
    let NodeKind::Str(string_id) = *cx.kind(*candidate) else {
        return None;
    };
    Some(cx.string_str(string_id))
}

/// Returns true if the mode string ends with 'b' (binary).
fn mode_is_binary(mode: Option<&str>) -> bool {
    mode.is_some_and(|m| m.ends_with('b'))
}

#[cfg(test)]
mod tests {
    use super::FileRead;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense + autocorrect cases ---

    #[test]
    fn flags_file_open_dot_read() {
        // carets = 19 (from 'o' in 'open' to end of 'read')
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename).read
                     ^^^^^^^^^^^^^^^^^^^ Use `File.read`.
            "#},
            "File.read(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_with_r_mode_dot_read() {
        // carets = 24
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename, 'r').read
                     ^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.read`.
            "#},
            "File.read(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_with_rb_mode_dot_read() {
        // carets = 25
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename, 'rb').read
                     ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binread`.
            "#},
            "File.binread(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_block_pass_read() {
        // carets = 22
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename, &:read)
                     ^^^^^^^^^^^^^^^^^^^^^^ Use `File.read`.
            "#},
            "File.read(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_rb_block_pass_read() {
        // carets = 28
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename, 'rb', &:read)
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binread`.
            "#},
            "File.binread(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_block_read() {
        // carets = 29
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename) { |f| f.read }
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.read`.
            "#},
            "File.read(filename)\n",
        );
    }

    #[test]
    fn flags_file_open_rb_block_read() {
        // carets = 35
        test::<FileRead>().expect_correction(
            indoc! {r#"
                File.open(filename, 'rb') { |f| f.read }
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `File.binread`.
            "#},
            "File.binread(filename)\n",
        );
    }

    // --- allowed cases ---

    #[test]
    fn accepts_file_read() {
        test::<FileRead>().expect_no_offenses("File.read(filename)\n");
    }

    #[test]
    fn accepts_file_binread() {
        test::<FileRead>().expect_no_offenses("File.binread(filename)\n");
    }

    #[test]
    fn accepts_write_mode() {
        test::<FileRead>().expect_no_offenses("File.open(filename, 'w').write('data')\n");
    }

    #[test]
    fn accepts_non_file_open() {
        test::<FileRead>().expect_no_offenses("io.open(filename).read\n");
    }
}
murphy_plugin_api::submit_cop!(FileRead);
