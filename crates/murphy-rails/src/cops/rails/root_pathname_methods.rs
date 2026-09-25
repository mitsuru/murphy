//! `Rails/RootPathnameMethods` — use `Rails.root` IO methods instead of `File`/`Dir`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RootPathnameMethods
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `Dir`/`IO`/`File`/`FileTest`/`FileUtils`
//!   calls (full upstream method tables) whose first argument is a bare
//!   `Rails.root`/`Rails.public_path` or a `Rails.root.join(...)` chain.
//!   `Dir[]`/`Dir.glob` rewrite to `Rails.root.glob("a/b")` (multi-arg joins
//!   are slash-joined; sym/int/float args use their value; anything else
//!   becomes `#{source}` interpolation, forcing double quotes); all other
//!   methods rewrite to `path.method[(args)]` (`*`-splicing array args,
//!   parenthesizing space-separated paths). `open` with a `send` parent is
//!   skipped like upstream (block/csend/lvasgn parents still flag). The
//!   Ruby < 2.5 narrowing (no `Dir[]`/`Dir.glob`) is mirrored; unset target
//!   Ruby resolves to newest and flags. Two documented narrowings: a bare
//!   `Rails.root` under `Dir[]`/`Dir.glob` is skipped (upstream crashes on
//!   the missing selector), and glob quoting always defaults to single
//!   quotes unless interpolation forces double (Murphy cannot read
//!   `Style/StringLiterals`; upstream default is single quotes, so default
//!   configs agree). Upstream ships `Enabled: pending`; Murphy maps that to
//!   `default_enabled = false`. Autocorrect is unsafe upstream
//!   (`SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, RubyVersion, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RootPathnameMethods;

#[cop(
    name = "Rails/RootPathnameMethods",
    description = "Use `Rails.root` IO methods instead of passing it to `File`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RootPathnameMethods {
    // Upstream `RESTRICT_ON_SEND` is the union of the tables below; the
    // attribute list would be ~100 entries, so dispatch on all sends and
    // gate on table membership in the body (same candidate set).
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const DIR_GLOB_METHODS: &[&str] = &["[]", "glob"];

const DIR_NON_GLOB_METHODS: &[&str] = &[
    "children",
    "delete",
    "each_child",
    "empty?",
    "entries",
    "exist?",
    "mkdir",
    "open",
    "rmdir",
    "unlink",
];

const FILE_METHODS: &[&str] = &[
    "atime",
    "basename",
    "binread",
    "binwrite",
    "birthtime",
    "blockdev?",
    "chardev?",
    "chmod",
    "chown",
    "ctime",
    "delete",
    "directory?",
    "dirname",
    "empty?",
    "executable?",
    "executable_real?",
    "exist?",
    "expand_path",
    "extname",
    "file?",
    "fnmatch",
    "fnmatch?",
    "ftype",
    "grpowned?",
    "join",
    "lchmod",
    "lchown",
    "lstat",
    "mtime",
    "open",
    "owned?",
    "pipe?",
    "read",
    "readable?",
    "readable_real?",
    "readlines",
    "readlink",
    "realdirpath",
    "realpath",
    "rename",
    "setgid?",
    "setuid?",
    "size",
    "size?",
    "socket?",
    "split",
    "stat",
    "sticky?",
    "symlink?",
    "sysopen",
    "truncate",
    "unlink",
    "utime",
    "world_readable?",
    "world_writable?",
    "writable?",
    "writable_real?",
    "write",
    "zero?",
];

const FILE_TEST_METHODS: &[&str] = &[
    "blockdev?",
    "chardev?",
    "directory?",
    "empty?",
    "executable?",
    "executable_real?",
    "exist?",
    "file?",
    "grpowned?",
    "owned?",
    "pipe?",
    "readable?",
    "readable_real?",
    "setgid?",
    "setuid?",
    "size",
    "size?",
    "socket?",
    "sticky?",
    "symlink?",
    "world_readable?",
    "world_writable?",
    "writable?",
    "writable_real?",
    "zero?",
];

const FILE_UTILS_METHODS: &[&str] = &["chmod", "chown", "mkdir", "mkpath", "rmdir", "rmtree"];

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    // Upstream receivers are bare/`::` consts only; `const_name` returns the
    // full path, so `Foo::File` never equals `File`.
    let recv_name = match cx.const_name(recv).as_deref() {
        Some(n) => n.to_owned(),
        None => return,
    };
    if !method_supported(cx, &recv_name, &method) {
        return;
    }
    // Upstream `return if node.method?(:open) && node.parent&.send_type?`.
    if method == "open"
        && let Some(parent) = cx.parent(node).get()
        && matches!(*cx.kind(parent), NodeKind::Send { .. })
    {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    let Some(&path) = args.first() else {
        return;
    };
    let Some(rails_root) = rails_root_pathname(cx, path) else {
        return;
    };
    let rails_src = cx.raw_source(cx.range(rails_root)).to_owned();
    let rest = &args[1..];
    let replacement = if recv_name == "Dir" && DIR_GLOB_METHODS.contains(&method.as_str()) {
        let Some(glob) = build_glob_replacement(cx, path) else {
            return;
        };
        glob
    } else {
        build_path_replacement(cx, path, &method, rest)
    };
    // The arena send range covers an attached block; upstream reports and
    // corrects the `File`/`Dir` send alone.
    let send_range = send_only_range(cx, node);
    cx.emit_offense(
        send_range,
        &format!("`{rails_src}` is a `Pathname`, so you can use `{replacement}`."),
        None,
    );
    cx.emit_edit(send_range, &replacement);
}


/// Send-only range: the arena send range covers an attached block, while
/// upstream's offense and correction span the receiver send alone.
/// Mirrors `Rails/FreezeTime`'s recomputation.
fn send_only_range(cx: &Cx<'_>, node: NodeId) -> murphy_plugin_api::Range {
    let range = cx.range(node);
    if cx.block_node(node).get().is_none() {
        return range;
    }
    if cx.is_parenthesized(node) {
        if let Some(paren_end) = matching_close_paren(cx, node) {
            return murphy_plugin_api::Range {
                start: range.start,
                end: paren_end,
            };
        }
        return range;
    }
    match cx.call_arguments(node).last() {
        Some(&last) => murphy_plugin_api::Range {
            start: range.start,
            end: cx.range(last).end,
        },
        None => murphy_plugin_api::Range {
            start: range.start,
            end: cx.loc(node).name.end,
        },
    }
}

/// Closing paren matching the call's open paren. Depth-counted so nested
/// calls (`File.read(Rails.root.join('a'))`) resolve to the outer `)`.
fn matching_close_paren(cx: &Cx<'_>, node: NodeId) -> Option<u32> {
    let toks = cx.tokens_in(cx.range(node));
    let sel_end = cx.selector(node).end;
    let open = toks
        .iter()
        .position(|t| t.kind == SourceTokenKind::LeftParen && t.range.start >= sel_end)?;
    let mut depth = 0i32;
    for tok in &toks[open..] {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    None
}

/// Upstream `pathname_method`: table membership per receiver, with the
/// Ruby >= 2.5 gate controlling `Dir[]`/`Dir.glob`.
fn method_supported(cx: &Cx<'_>, recv: &str, method: &str) -> bool {
    match recv {
        "Dir" => {
            let modern = cx
                .target_ruby_version()
                .is_none_or(|v| v >= RubyVersion::new(2, 5));
            if modern {
                DIR_GLOB_METHODS.contains(&method) || DIR_NON_GLOB_METHODS.contains(&method)
            } else {
                DIR_NON_GLOB_METHODS.contains(&method)
            }
        }
        "IO" | "File" => FILE_METHODS.contains(&method),
        "FileTest" => FILE_TEST_METHODS.contains(&method),
        "FileUtils" => FILE_UTILS_METHODS.contains(&method),
        _ => false,
    }
}

/// Upstream `rails_root_pathname?`: a bare zero-arg `Rails.root` /
/// `Rails.public_path`, or a `join` call on one. Returns the inner root node
/// whose source seeds the message.
fn rails_root_pathname(cx: &Cx<'_>, path: NodeId) -> Option<NodeId> {
    if is_rails_root(cx, path) {
        return Some(path);
    }
    if !matches!(*cx.kind(path), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(path) != Some("join") {
        return None;
    }
    let recv = cx.call_receiver(path).get()?;
    if is_rails_root(cx, recv) {
        Some(recv)
    } else {
        None
    }
}

/// Zero-arg `Rails.root` / `Rails.public_path` (`::Rails` included).
fn is_rails_root(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    let method = cx.method_name(id);
    if method != Some("root") && method != Some("public_path") {
        return false;
    }
    if !cx.call_arguments(id).is_empty() {
        return false;
    }
    cx.call_receiver(id)
        .get()
        .is_some_and(|r| cx.const_name(r).as_deref() == Some("Rails"))
}

/// Upstream `build_path_glob_replacement`. Returns `None` for a bare root
/// (no join args to glob over) — upstream crashes on that shape, so Murphy
/// skips it instead.
fn build_glob_replacement(cx: &Cx<'_>, path: NodeId) -> Option<String> {
    if !matches!(*cx.kind(path), NodeKind::Send { .. })
        || cx.method_name(path) != Some("join")
    {
        return None;
    }
    let rails = cx.call_receiver(path).get()?;
    let start = cx.range(path).start;
    let selector_end = cx.loc(rails).name.end;
    let receiver_src = cx.source().get(start as usize..selector_end as usize)?;
    let join_args = cx.call_arguments(path).to_vec();
    let argument = if join_args.len() == 1 {
        cx.raw_source(cx.range(join_args[0])).to_owned()
    } else {
        join_arguments(cx, &join_args)
    };
    Some(format!("{receiver_src}.glob({argument})"))
}

/// Upstream `build_path_replacement`.
fn build_path_replacement(cx: &Cx<'_>, path: NodeId, method: &str, args: &[NodeId]) -> String {
    let mut path_rep = cx.raw_source(cx.range(path)).to_owned();
    // Upstream wraps space-separated (non-parenthesized) paths in parens.
    if !cx.call_arguments(path).is_empty() && !cx.is_parenthesized(path)
        && let Some(pos) = path_rep.find(' ')
    {
        path_rep.replace_range(pos..pos + 1, "(");
        path_rep.push(')');
    }
    let mut replacement = format!("{path_rep}.{method}");
    if !args.is_empty() {
        let formatted: Vec<String> = args
            .iter()
            .map(|&a| {
                let src = cx.raw_source(cx.range(a)).to_owned();
                if matches!(*cx.kind(a), NodeKind::Array(_)) {
                    format!("*{src}")
                } else {
                    src
                }
            })
            .collect();
        replacement.push_str(&format!("({})", formatted.join(", ")));
    }
    replacement
}

/// Upstream `join_arguments`: literal values are inlined, anything else
/// becomes `#{source}` interpolation (forcing double quotes).
fn join_arguments(cx: &Cx<'_>, args: &[NodeId]) -> String {
    let mut use_interpolation = false;
    let mut parts = Vec::with_capacity(args.len());
    for &arg in args {
        match literal_value(cx, arg) {
            Some(v) => parts.push(v),
            None => {
                use_interpolation = true;
                parts.push(format!("#{{{}}}", cx.raw_source(cx.range(arg))));
            }
        }
    }
    let joined = parts.join("/");
    // Upstream also consults `Style/StringLiterals` and nested
    // interpolation; Murphy cannot read sibling-cop config, so single
    // quotes win unless interpolation (or a dstr arg) forces double —
    // matching upstream under default config.
    let force_double = use_interpolation
        || args
            .iter()
            .any(|&a| matches!(*cx.kind(a), NodeKind::Dstr(_)));
    let quote = if force_double { '"' } else { '\'' };
    format!("{quote}{joined}{quote}")
}

/// Upstream `arg.respond_to?(:value)` inlined for the literal shapes Murphy
/// models with direct value access.
fn literal_value(cx: &Cx<'_>, arg: NodeId) -> Option<String> {
    match *cx.kind(arg) {
        NodeKind::Str(id) => Some(cx.string_str(id).to_owned()),
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Int(n) => Some(n.to_string()),
        NodeKind::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RootPathnameMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_file_read() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                File.read(Rails.root.join('db', 'schema.rb'))
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.join('db', 'schema.rb').read`.
            "#},
            "Rails.root.join('db', 'schema.rb').read\n",
        );
    }

    #[test]
    fn flags_file_write_with_extra_arg() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                File.write(Rails.root.join('db', 'schema.rb'), content)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.join('db', 'schema.rb').write(content)`.
            "#},
            "Rails.root.join('db', 'schema.rb').write(content)\n",
        );
    }

    #[test]
    fn flags_dir_glob() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                Dir.glob(Rails.root.join('db', 'schema.rb'))
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.glob('db/schema.rb')`.
            "#},
            "Rails.root.glob('db/schema.rb')\n",
        );
    }

    #[test]
    fn flags_dir_brackets() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                Dir[Rails.root.join('db', 'schema.rb')]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.glob('db/schema.rb')`.
            "#},
            "Rails.root.glob('db/schema.rb')\n",
        );
    }

    #[test]
    fn flags_bare_root() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                File.read(Rails.root)
                ^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.read`.
            "#},
            "Rails.root.read\n",
        );
    }

    #[test]
    fn flags_file_test_and_utils() {
        test::<RootPathnameMethods>().expect_offense(
            indoc! {r#"
                FileTest.exist?(Rails.root.join('db', 'schema.rb'))
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.join('db', 'schema.rb').exist?`.
            "#},
        );
        test::<RootPathnameMethods>().expect_offense(
            indoc! {r#"
                FileUtils.mkdir(Rails.root.join('db'))
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.join('db').mkdir`.
            "#},
        );
    }

    #[test]
    fn globs_interpolated_arg_with_double_quotes() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                Dir.glob(Rails.root.join('db', x))
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.glob("db/#{x}")`.
            "#},
            "Rails.root.glob(\"db/#{x}\")\n",
        );
    }

    #[test]
    fn keeps_block_with_send_only_offense() {
        test::<RootPathnameMethods>().expect_correction(
            indoc! {r#"
                File.read(Rails.root.join('a')) { |f| puts f }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `Rails.root` is a `Pathname`, so you can use `Rails.root.join('a').read`.
            "#},
            "Rails.root.join('a').read { |f| puts f }\n",
        );
    }

    #[test]
    fn allows_open_with_send_parent() {
        test::<RootPathnameMethods>()
            .expect_no_offenses("File.open(Rails.root.join('db', 'schema.rb')).read\n");
    }

    #[test]
    fn allows_unrelated_receiver() {
        test::<RootPathnameMethods>()
            .expect_no_offenses("File.read(Rails.root.join('db', 'schema.rb').to_s + '.bak')\n");
    }

    #[test]
    fn allows_plain_path() {
        test::<RootPathnameMethods>().expect_no_offenses("File.read('db/schema.rb')\n");
    }

    #[test]
    fn allows_unsupported_method() {
        test::<RootPathnameMethods>()
            .expect_no_offenses("File.nonexistent(Rails.root.join('db'))\n");
    }
}
murphy_plugin_api::submit_cop!(RootPathnameMethods);
