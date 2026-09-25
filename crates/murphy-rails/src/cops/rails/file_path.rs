//! `Rails/FilePath` — use `Rails.root.join` for file path joining.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FilePath
//! upstream_version_checked: 2.35.0
//! version_added: "0.47"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Send-dispatch port of rubocop-rails 2.35.0 with EnforcedStyle
//!   (slashes default, arguments alternative). File.join with a Rails.root
//!   argument, Rails.root.join multi-arg (slashes style) and slash-arg
//!   (arguments style) shapes are implemented with autocorrect. Dstr
//!   `"#{Rails.root}/..."` is implemented; the
//!   `"#{Rails.root.join(...)}/ext"` extension-append shape and the
//!   colon-guard (`"#{x}:#{y}"`) edge are not (documented narrowings).
//!   Leading-slash and double-slash args are ignored like upstream.
//! ```
//!
//! Identifies usages of file path joining process to use
//! `Rails.root.join` clause for uniformity.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FilePath;

#[derive(CopOptions)]
pub struct FilePathOptions {
    #[option(
        name = "EnforcedStyle",
        default = "slashes",
        description = "Whether to enforce slash-separated paths or separate arguments."
    )]
    pub enforced_style: FilePathStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum FilePathStyle {
    #[option(value = "slashes")]
    Slashes,
    #[option(value = "arguments")]
    Arguments,
}

#[cop(
    name = "Rails/FilePath",
    description = "Use `Rails.root.join` for file path joining.",
    default_severity = "warning",
    default_enabled = true,
    options = FilePathOptions,
)]
impl FilePath {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        check_dstr(node, cx);
    }
}

fn check_send(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "join" {
        return;
    }
    // `File.join(...)` with a Rails.root argument.
    if is_file_const_receiver(cx, node) {
        check_file_join(node, cx);
        return;
    }
    // `Rails.root.join(...)` style checks.
    if !is_rails_root_join(cx, node) {
        return;
    }
    let opts = cx.options_or_default::<FilePathOptions>();
    match opts.enforced_style {
        FilePathStyle::Slashes => check_slashes_style(node, cx),
        FilePathStyle::Arguments => check_arguments_style(node, cx),
    }
}

/// True when receiver is `File` / `::File` const.
fn is_file_const_receiver(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.call_receiver(node)
        .get()
        .is_some_and(|r| cx.const_name(r).as_deref() == Some("File"))
}

/// True when receiver is a `Rails.root` (or `::Rails.root`) call.
fn is_rails_root(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("root") {
        return false;
    }
    if !cx.call_arguments(id).is_empty() {
        return false;
    }
    cx.call_receiver(id)
        .get()
        .is_some_and(|r| cx.const_name(r).as_deref() == Some("Rails"))
}

/// True when `node` is `join` on a `Rails.root` receiver.
fn is_rails_root_join(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.call_receiver(node).get().is_some_and(|r| is_rails_root(cx, r))
}

/// `File.join(Rails.root, ...)` → `Rails.root.join(...).to_s`.
fn check_file_join(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if !args.iter().any(|&a| is_rails_root(cx, a)) {
        return;
    }
    // No variable/const args; no `//` string args.
    for &a in args {
        match *cx.kind(a) {
            NodeKind::Lvar(_)
            | NodeKind::Ivar(_)
            | NodeKind::Cvar(_)
            | NodeKind::Gvar(_)
            | NodeKind::Const { .. }
                if !is_rails_root(cx, a) =>
            {
                // Rails.root itself is a Send, not Const — consts bail.
                // But `File` const args? Any const bails like upstream.
                return;
            }
            NodeKind::Str(id) if cx.string_str(id).contains("//") => {
                return;
            }
            _ => {}
        }
        // Rails.root args are Send — allowed. Const args bail.
        if matches!(*cx.kind(a), NodeKind::Const { .. }) {
            return;
        }
    }
    let msg = "Prefer `Rails.root.join('path/to').to_s`.";
    cx.emit_offense(cx.range(node), msg, None);
    // Autocorrect: replace `File` receiver with `Rails.root`, drop the
    // Rails.root first-arg (with comma), strip leading `/` from strings,
    // splat arrays, append `.to_s`.
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    cx.emit_edit(cx.range(recv), "Rails.root");
    // Remove the Rails.root argument (first such arg) with its comma.
    if let Some(&rr) = args.iter().find(|&&a| is_rails_root(cx, a)) {
        let rr_range = cx.range(rr);
        // Extend right to swallow the following comma + spaces.
        let src = cx.source();
        let mut end = rr_range.end as usize;
        let bytes = src.as_bytes();
        while end < src.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
            end += 1;
        }
        if end < src.len() && bytes[end] == b',' {
            end += 1;
            while end < src.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
                end += 1;
            }
        }
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: rr_range.start,
                end: end as u32,
            },
            "",
        );
    }
    for &a in args {
        if is_rails_root(cx, a) {
            continue;
        }
        match *cx.kind(a) {
            NodeKind::Str(id) => {
                let v = cx.string_str(id).to_owned();
                let stripped = v.strip_prefix('/').unwrap_or(&v);
                cx.emit_edit(cx.range(a), &format!("\"{stripped}\""));
            }
            NodeKind::Array(_) => {
                let src = cx.raw_source(cx.range(a)).to_owned();
                cx.emit_edit(cx.range(a), &format!("*{src}"));
            }
            _ => {}
        }
    }
    cx.emit_edit(
        murphy_plugin_api::Range {
            start: cx.range(node).end,
            end: cx.range(node).end,
        },
        ".to_s",
    );
}

/// Slashes style: `Rails.root.join('a', 'b')` → `Rails.root.join('a/b')`.
fn check_slashes_style(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.len() <= 1 {
        return;
    }
    let mut values = Vec::new();
    for &a in args {
        let NodeKind::Str(id) = *cx.kind(a) else {
            return;
        };
        let v = cx.string_str(id).to_owned();
        if v.starts_with('/') || v.contains("//") {
            return;
        }
        values.push(v);
    }
    cx.emit_offense(cx.range(node), "Prefer `Rails.root.join('path/to')`.", None);
    let joined = values.join("/");
    cx.emit_edit(cx.range(args[0]), &format!("\"{joined}\""));
    // Delete remaining args with preceding comma.
    for &a in &args[1..] {
        let r = cx.range(a);
        let src = cx.source();
        let mut start = r.start as usize;
        let bytes = src.as_bytes();
        // Walk left over spaces then comma.
        while start > 0 && (bytes[start - 1] == b' ' || bytes[start - 1] == b'\t') {
            start -= 1;
        }
        if start > 0 && bytes[start - 1] == b',' {
            start -= 1;
            while start > 0 && (bytes[start - 1] == b' ' || bytes[start - 1] == b'\t') {
                start -= 1;
            }
        }
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: start as u32,
                end: r.end,
            },
            "",
        );
    }
}

/// Arguments style: `Rails.root.join('a/b')` → `Rails.root.join('a', 'b')`.
fn check_arguments_style(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if !args.iter().any(|&a| {
        matches!(*cx.kind(a), NodeKind::Str(_))
            && cx
                .string_str(match *cx.kind(a) {
                    NodeKind::Str(id) => id,
                    _ => unreachable!(),
                })
                .contains('/')
    }) {
        return;
    }
    for &a in args {
        if let NodeKind::Str(id) = *cx.kind(a) {
            let v = cx.string_str(id).to_owned();
            if v.starts_with('/') || v.contains("//") {
                return;
            }
        }
    }
    cx.emit_offense(cx.range(node), "Prefer `Rails.root.join('path', 'to')`.", None);
    for &a in args {
        let NodeKind::Str(id) = *cx.kind(a) else {
            continue;
        };
        let v = cx.string_str(id).to_owned();
        if !v.contains('/') {
            continue;
        }
        // Split on first `/`: keep head in place, insert `, "tail"` after.
        let idx = v.find('/').unwrap();
        let (head, tail) = v.split_at(idx);
        let tail = tail.strip_prefix('/').unwrap_or(tail);
        cx.emit_edit(cx.range(a), &format!("\'{head}\'"));
        cx.emit_edit(
            murphy_plugin_api::Range {
                start: cx.range(a).end,
                end: cx.range(a).end,
            },
            &format!(", \"{tail}\""),
        );
    }
}

/// `"#{Rails.root}/path"` → `Rails.root.join("path")`.
fn check_dstr(node: NodeId, cx: &Cx<'_>) {
    let parts = cx.list(cx.dstr_parts(node));
    // Find a Begin wrapping Rails.root (bare) followed by a Str starting with `/`.
    for (i, &part) in parts.iter().enumerate() {
        // Part must be a Begin whose single child is Rails.root or Rails.root.join.
        let inner = begin_single_child(cx, part);
        let Some(inner_id) = inner else {
            continue;
        };
        let next = parts.get(i + 1).copied();
        let Some(next_id) = next else {
            continue;
        };
        let NodeKind::Str(sid) = *cx.kind(next_id) else {
            continue;
        };
        let s = cx.string_str(sid).to_owned();
        if !s.starts_with('/') {
            continue;
        }
        // Colon guard: `"#{x}:#{y}"` shapes are not paths.
        if parts[i + 1..].iter().any(|&p| {
            matches!(*cx.kind(p), NodeKind::Str(_))
                && cx
                    .string_str(match *cx.kind(p) {
                        NodeKind::Str(id) => id,
                        _ => unreachable!(),
                    })
                    .starts_with(':')
        }) {
            continue;
        }
        if is_rails_root(cx, inner_id) {
            let mut path_val = String::new();
            for &p in &parts[i + 1..] {
                match *cx.kind(p) {
                    NodeKind::Str(id) => path_val.push_str(cx.string_str(id)),
                    _ => path_val.push_str(cx.raw_source(cx.range(p))),
                }
            }
            let path_val = path_val.strip_prefix('/').unwrap_or(&path_val).to_owned();
            cx.emit_offense(cx.range(node), "Prefer `Rails.root.join(\'path/to\')`.", None);
            cx.emit_edit(cx.range(inner_id), &format!("Rails.root.join(\"{path_val}\")"));
            for &p in &parts[i + 1..] {
                cx.emit_edit(cx.range(p), "");
            }
            return;
        }
        // `"#{Rails.root.join('a')}/b"` extension shape — not implemented.
    }
}

/// Returns the single child of a Begin node, if `id` is a single-child Begin.
fn begin_single_child(cx: &Cx<'_>, id: NodeId) -> Option<NodeId> {
    match *cx.kind(id) {
        NodeKind::Begin(list) => {
            let items = cx.list(list);
            if items.len() == 1 {
                Some(items[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Dstr parts accessor: Dstr holds a NodeList directly.
trait DstrParts {
    fn dstr_parts(&self, id: NodeId) -> murphy_plugin_api::NodeList;
}

impl DstrParts for Cx<'_> {
    fn dstr_parts(&self, id: NodeId) -> murphy_plugin_api::NodeList {
        match *self.kind(id) {
            NodeKind::Dstr(list) => list,
            _ => murphy_plugin_api::NodeList::EMPTY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FilePath, FilePathOptions, FilePathStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_root_join_multi_arg_slashes() {
        test::<FilePath>().expect_offense(indoc! {r#"
            Rails.root.join('app', 'models')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to')`.
        "#});
    }

    #[test]
    fn autocorrects_root_join_multi_arg() {
        test::<FilePath>().expect_correction(
            indoc! {r#"
                Rails.root.join('app', 'models')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to')`.
            "#},
            "Rails.root.join(\"app/models\")\n",
        );
    }

    #[test]
    fn accepts_root_join_slash_path_slashes() {
        test::<FilePath>().expect_no_offenses("Rails.root.join('app/models')\n");
    }

    #[test]
    fn flags_slash_path_arguments_style() {
        let opts = FilePathOptions {
            enforced_style: FilePathStyle::Arguments,
        };
        test::<FilePath>().with_options(&opts).expect_offense(indoc! {r#"
            Rails.root.join('app/models')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path', 'to')`.
        "#});
    }

    #[test]
    fn autocorrects_slash_path_arguments_style() {
        let opts = FilePathOptions {
            enforced_style: FilePathStyle::Arguments,
        };
        test::<FilePath>().with_options(&opts).expect_correction(
            indoc! {r#"
                Rails.root.join('app/models')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path', 'to')`.
            "#},
            "Rails.root.join(\'app\', \"models\")\n",
        );
    }

    #[test]
    fn flags_file_join_with_rails_root() {
        test::<FilePath>().expect_offense(indoc! {r#"
            File.join(Rails.root, 'app/models')
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to').to_s`.
        "#});
    }

    #[test]
    fn autocorrects_file_join() {
        test::<FilePath>().expect_correction(
            indoc! {r#"
                File.join(Rails.root, 'app/models')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to').to_s`.
            "#},
            "Rails.root.join(\"app/models\").to_s\n",
        );
    }

    #[test]
    fn flags_dstr_rails_root_slash() {
        test::<FilePath>().expect_offense(concat!(
            "\"#{Rails.root}/app/models\"\n",
            "^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to')`.\n",
        ));
    }

    #[test]
    fn autocorrects_dstr() {
        test::<FilePath>().expect_correction(
            concat!(
                "\"#{Rails.root}/app/models\"\n",
                "^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `Rails.root.join('path/to')`.\n",
            ),
            "\"#{Rails.root.join(\"app/models\")}\"\n",
        );
    }

    #[test]
    fn does_not_flag_leading_slash_arg() {
        test::<FilePath>().expect_no_offenses("Rails.root.join('/app', 'models')\n");
    }
}
murphy_plugin_api::submit_cop!(FilePath);
