//! `Naming/FileName` — require source file names to use snake_case.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/FileName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of rubocop 1.87.0 `file_name.rb` end-to-end. The basename
//!   of the source file (`cx.file_path()` → last path component) must match
//!   `SNAKE_CASE = /^[\d[[:lower:]]_.?!]+$/` after RuboCop's exact basename
//!   normalization (delete one leading dot, strip only the last extension,
//!   replace the first `+` with `_`). `IgnoreExecutableScripts` (default
//!   true) suppresses the bad-filename offense when the source begins with a
//!   `#!` shebang. The offense is RuboCop's `add_global_offense`, rendered at
//!   line 1 column 1 (a single-column range at byte 0, clamped on empty
//!   source). Verified across snake_case/CamelCase/dash/space/multi-dot/
//!   leading-dot/`+`/digit/`?`/`!` basenames and the shebang skip.
//!
//!   Opt-in machinery (murphy-ugzq):
//!     * ExpectMatchingDefinition (default false) — when the filename is
//!       good, require the file to define a matching class/module/Struct
//!       (`class`/`module` with a const name, `Foo = Class.new` /
//!       `Module.new` with or without a block, `Foo = Struct.new` with or
//!       without a block; `Data.define` is NOT a definition, matching
//!       upstream). Message: "`basename` should define a class or module
//!       called `Namespace`.". A bad filename still reports the snake_case /
//!       regex message and skips the definition check; a shebang does NOT
//!       suppress a missing-definition offense (both verified).
//!     * CheckDefinitionPathHierarchy (default true) /
//!       CheckDefinitionPathHierarchyRoots (default
//!       ["lib","spec","test","src"]) — `to_namespace` finds the rightmost
//!       root component and maps the remainder via `to_module_name`
//!       (first-dot strip, `_` split, Ruby `capitalize` per word). The full
//!       path namespace is checked first, then the basename-only namespace
//!       (verified: `lib/foo/bar.rb` needs `Foo::Bar`, `Bar` alone fails;
//!       `CheckDefinitionPathHierarchy: false` accepts `Bar` alone).
//!     * AllowedAcronyms — `match_acronym?` (`expected.gsub(
//!       acronym.capitalize, acronym) == name`) applied to the target name
//!       and to every scope/ancestor segment (verified: `api_client.rb`
//!       defining `APIClient` passes with defaults, fails with `[]`).
//!     * Regex — custom pattern overriding SNAKE_CASE, matched (search,
//!       not full-match) against the normalized basename; the bad-filename
//!       message becomes "`basename` should match `pattern`." (verified).
//!       An uncompilable pattern is treated as non-matching (offense)
//!       rather than panicking, per murphy's invalid-pattern convention.
//!       Ruby-vs-Rust regex syntax differences (look-around, etc.) are the
//!       only known divergence.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop, regex::Regex};

#[derive(Default)]
pub struct FileName;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "ExpectMatchingDefinition",
        default = false,
        description = "Require each file to define a class/module matching its filename."
    )]
    pub expect_matching_definition: bool,
    #[option(
        name = "CheckDefinitionPathHierarchy",
        default = true,
        description = "Require the namespace hierarchy to match the subdirectory path."
    )]
    pub check_definition_path_hierarchy: bool,
    #[option(
        name = "CheckDefinitionPathHierarchyRoots",
        default = ["lib", "spec", "test", "src"],
        description = "Root directories for definition-path-hierarchy matching."
    )]
    pub check_definition_path_hierarchy_roots: Vec<String>,
    #[option(
        name = "AllowedAcronyms",
        default = ["CLI", "DSL", "ACL", "API", "ASCII", "CPU", "CSS", "DNS", "EOF", "GUID", "HTML", "HTTP", "HTTPS", "ID", "IP", "JSON", "LHS", "QPS", "RAM", "RHS", "RPC", "SLA", "SMTP", "SQL", "SSH", "TCP", "TLS", "TTL", "UDP", "UI", "UID", "UUID", "URI", "URL", "UTF8", "VM", "XML", "XMPP", "XSRF", "XSS"],
        description = "Acronyms tolerated when matching definitions to filenames."
    )]
    pub allowed_acronyms: Vec<String>,
    #[option(
        name = "Regex",
        description = "Custom filename pattern overriding snake_case (matched against the normalized basename)."
    )]
    pub regex: Option<String>,
    #[option(
        name = "IgnoreExecutableScripts",
        default = true,
        description = "Don't report offending filenames for executable scripts (i.e. source files with a shebang in the first line)."
    )]
    pub ignore_executable_scripts: bool,
}

#[cop(
    name = "Naming/FileName",
    description = "Use snake_case for source file names.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl FileName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let file_path = cx.file_path();
        let basename = basename(file_path);
        // The ABI returns an empty `file_path` when the host cannot expose one
        // (e.g. stdin). RuboCop never lints a pathless source, so guard against
        // emitting a nonsense offense on an empty basename.
        if basename.is_empty() {
            return;
        }

        let regex_opt = opts.regex.as_deref();
        if filename_good(basename, regex_opt) {
            // Good filename: only the opt-in definition check can still fail.
            // A shebang does NOT suppress it (verified against rubocop 1.87.0).
            if !opts.expect_matching_definition {
                return;
            }
            if opts.check_definition_path_hierarchy {
                let ns_full =
                    to_namespace(file_path, &opts.check_definition_path_hierarchy_roots);
                if !matching_definition(&ns_full, cx, &opts.allowed_acronyms) {
                    let ns = ns_full.join("::");
                    cx.emit_offense(
                        global_offense_range(cx),
                        &format!(
                            "`{basename}` should define a class or module called `{ns}`."
                        ),
                        None,
                    );
                    return;
                }
            }
            let ns_base = to_namespace(basename, &opts.check_definition_path_hierarchy_roots);
            if !matching_definition(&ns_base, cx, &opts.allowed_acronyms) {
                let ns = ns_base.join("::");
                cx.emit_offense(
                    global_offense_range(cx),
                    &format!("`{basename}` should define a class or module called `{ns}`."),
                    None,
                );
            }
            return;
        }

        // Bad filename: `IgnoreExecutableScripts` (default true) suppresses
        // the offense for a leading shebang.
        if opts.ignore_executable_scripts && cx.source().starts_with("#!") {
            return;
        }

        let message = match regex_opt {
            Some(pat) => format!("`{basename}` should match `{pat}`."),
            None => format!(
                "The name of this source file (`{basename}`) should use snake_case."
            ),
        };
        cx.emit_offense(global_offense_range(cx), &message, None);
    }
}

/// Last path component of `path`, mirroring Ruby's `File.basename` for the
/// shapes a lint target takes (an empty path yields an empty basename).
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether `basename` satisfies RuboCop's filename rule.
///
/// Mirrors RuboCop's `filename_good?` exactly, in order:
///   1. delete one leading `.` (`delete_prefix('.')`);
///   2. strip only the last extension (`sub(/\.[^.]+$/, '')`);
///   3. replace the first `+` with `_` (`sub('+', '_')`);
///   4. match `regex || SNAKE_CASE` (search, not full-match).
fn filename_good(basename: &str, regex: Option<&str>) -> bool {
    let stripped = basename.strip_prefix('.').unwrap_or(basename);
    let no_ext = strip_last_extension(stripped);
    let normalized = replace_first_plus(no_ext);
    if let Some(pat) = regex {
        match Regex::new(pat) {
            Ok(re) => re.is_match(&normalized),
            // Uncompilable pattern: report (non-matching) rather than panic,
            // per murphy's invalid-pattern convention.
            Err(_) => false,
        }
    } else {
        matches_snake_case(&normalized)
    }
}

/// `sub(/\.[^.]+$/, '')` — remove only the final extension. The extension is
/// the last `.` followed by ≥1 non-`.` characters at end of string. A
/// trailing `.` (no chars after) is NOT an extension and is left in place.
fn strip_last_extension(name: &str) -> &str {
    match name.rfind('.') {
        // `.` must be followed by at least one non-`.` char to count as an
        // extension (`[^.]+$`); a trailing dot leaves the name untouched.
        Some(dot) if dot + 1 < name.len() && !name[dot + 1..].contains('.') => &name[..dot],
        // No dot, or the only chars after the last dot include another dot
        // (impossible since it's the last dot) / it's a trailing dot.
        _ => name,
    }
}

/// `sub('+', '_')` — replace only the FIRST `+`. Returns the input borrowed
/// when there is no `+`, allocating only when a replacement is needed.
fn replace_first_plus(name: &str) -> std::borrow::Cow<'_, str> {
    match name.find('+') {
        Some(idx) => {
            let mut out = String::with_capacity(name.len());
            out.push_str(&name[..idx]);
            out.push('_');
            out.push_str(&name[idx + 1..]);
            std::borrow::Cow::Owned(out)
        }
        None => std::borrow::Cow::Borrowed(name),
    }
}

/// `SNAKE_CASE = /^[\d[[:lower:]]_.?!]+$/` — non-empty, and every char is an
/// ASCII digit, a lowercase letter, `_`, `.`, `?`, or `!`. `[[:lower:]]` is
/// Unicode-aware in Ruby; mirror that with `char::is_lowercase`.
fn matches_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_lowercase() || matches!(c, '_' | '.' | '?' | '!'))
}

/// RuboCop's `add_global_offense` renders at line 1, column 1 (length 0). The
/// caret-based test harness needs a non-empty range to carry a caret, so use
/// a single-column range at byte 0, clamped to the source length so an empty
/// source yields a valid zero-width range instead of an out-of-bounds end.
fn global_offense_range(cx: &Cx<'_>) -> Range {
    let end = 1.min(cx.source().len() as u32);
    Range { start: 0, end }
}

// --- ExpectMatchingDefinition machinery (rubocop 1.87.0 `file_name.rb`). ---

/// `to_namespace`: split `path` into components (Ruby `Pathname#each_filename`
/// keeps `.` segments, drops empties), find the rightmost
/// `CheckDefinitionPathHierarchyRoots` entry, and map the remainder via
/// `to_module_name`. No root → basename only.
fn to_namespace(path: &str, roots: &[String]) -> Vec<String> {
    let components: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|c| !c.is_empty())
        .collect();
    let Some(last) = components.last() else {
        return Vec::new();
    };
    let mut start_index: Option<usize> = None;
    for (i, c) in components.iter().rev().enumerate() {
        if roots.iter().any(|r| r == *c) {
            start_index = Some(components.len() - i);
            break;
        }
    }
    match start_index {
        None => vec![to_module_name(last)],
        Some(idx) => components[idx..].iter().map(|c| to_module_name(c)).collect(),
    }
}

/// `to_module_name`: `basename.sub(/\..*/, '')` (first dot onward), split on
/// `_`, Ruby `capitalize` each word, join.
fn to_module_name(basename: &str) -> String {
    let stem = match basename.find('.') {
        Some(idx) => &basename[..idx],
        None => basename,
    };
    stem.split('_')
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join("")
}

/// Ruby `String#capitalize`: first char upper, rest lower.
fn capitalize_word(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

/// `match_acronym?`: `allowed.any? { |a| expected.gsub(a.capitalize, a) == name }`.
fn match_acronym(expected: &str, name: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|acronym| {
        let cap = capitalize_word(acronym);
        if cap.is_empty() {
            return false;
        }
        expected.replace(&cap, acronym) == name
    })
}

/// `match?`: `expected.empty? || expected == [:Object]`.
fn namespace_consumed(expected: &[String]) -> bool {
    expected.is_empty() || *expected == ["Object".to_string()]
}

/// All `class`/`module`/`casgn` nodes in the file (root included, since
/// `descendants` excludes it).
fn definition_nodes(cx: &Cx<'_>) -> Vec<NodeId> {
    let root = cx.root();
    let mut nodes = cx.descendants(root);
    nodes.push(root);
    nodes
        .into_iter()
        .filter(|id| {
            matches!(
                *cx.kind(*id),
                NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Casgn { .. }
            )
        })
        .collect()
}

/// Unwrap `Block`/`Numblock`/`Itblock` around a constructor call.
fn constructor_call(value: NodeId, cx: &Cx<'_>) -> NodeId {
    match *cx.kind(value) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send: call, .. } | NodeKind::Itblock { send: call, .. } => call,
        _ => value,
    }
}

/// Whether `value` is `Class.new`/`Module.new`/`Struct.new` (any args, with or
/// without a block wrapper). Mirrors the `casgn` arms of rubocop-ast's
/// `defined_module0` plus `FileName#defined_struct`.
fn is_definition_constructor(value: NodeId, cx: &Cx<'_>, allowed: &[&str]) -> bool {
    let call = constructor_call(value, cx);
    if cx.method_name(call) != Some("new") {
        return false;
    }
    let Some(recv) = cx.call_receiver(call).get() else {
        return false;
    };
    allowed.iter().any(|name| cx.is_global_const(recv, name))
}

/// `find_definition`: `node.defined_module || defined_struct(node)`.
/// `class`/`module` → its const; `casgn` with a `Class`/`Module`/`Struct.new`
/// value (plain or block-wrapped) → its `(scope, name)`.
fn find_definition(node: NodeId, cx: &Cx<'_>) -> Option<(OptNodeId, String)> {
    match *cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => match *cx.kind(name) {
            NodeKind::Const { scope, name: sym } => {
                Some((scope, cx.symbol_str(sym).to_string()))
            }
            _ => None,
        },
        NodeKind::Casgn { scope, name, value } => {
            let v = value.get()?;
            if is_definition_constructor(v, cx, &["Class", "Module", "Struct"]) {
                Some((scope, cx.symbol_str(name).to_string()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `ancestor.defined_module` (Class/Module only — Struct/Data excluded, so a
/// `Foo = Struct.new` ancestor contributes no namespace, matching upstream).
fn ancestor_defined_module(node: NodeId, cx: &Cx<'_>) -> Option<(OptNodeId, String)> {
    match *cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => match *cx.kind(name) {
            NodeKind::Const { scope, name: sym } => {
                Some((scope, cx.symbol_str(sym).to_string()))
            }
            _ => None,
        },
        NodeKind::Casgn { scope, name, value } => {
            let v = value.get()?;
            if is_definition_constructor(v, cx, &["Class", "Module"]) {
                Some((scope, cx.symbol_str(name).to_string()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `find_class_or_module`: pop the target name, then scan every definition.
/// An empty remainder matches any nesting (short-circuit, no ancestor walk).
fn matching_definition(namespace: &[String], cx: &Cx<'_>, allowed: &[String]) -> bool {
    if namespace.is_empty() {
        return false;
    }
    let mut head = namespace.to_vec();
    let target = head.pop().unwrap_or_default();
    for id in definition_nodes(cx) {
        let Some((scope, def_name)) = find_definition(id, cx) else {
            continue;
        };
        if def_name != target && !match_acronym(&target, &def_name, allowed) {
            continue;
        }
        if head.is_empty() {
            return true;
        }
        let mut expected = head.clone();
        if namespace_matches(id, scope, &mut expected, cx, allowed) {
            return true;
        }
    }
    false
}

/// `namespace_matches?`: drain `scope` through the partial matcher, then each
/// `class`/`module`/`casgn` ancestor's full const. An `sclass` ancestor fails
/// immediately. The lambda's return value is ignored upstream — only the
/// `expected` draining (and the early cbase stop) matters.
fn namespace_matches(
    node: NodeId,
    scope: OptNodeId,
    expected: &mut Vec<String>,
    cx: &Cx<'_>,
    allowed: &[String],
) -> bool {
    partial_match_scope(scope, expected, cx, allowed);
    for ancestor in cx.ancestors(node) {
        match *cx.kind(ancestor) {
            NodeKind::Sclass { .. } => return false,
            NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Casgn { .. } => {
                if let Some((a_scope, a_name)) = ancestor_defined_module(ancestor, cx) {
                    partial_match_full(a_scope, &a_name, expected, cx, allowed);
                }
            }
            _ => {}
        }
    }
    namespace_consumed(expected)
}

/// `partial_matcher!` applied to a scope chain (`const` nodes walked outward).
/// A `cbase` segment stops the walk; anything non-`const` stops it too.
fn partial_match_scope(
    mut current: OptNodeId,
    expected: &mut Vec<String>,
    cx: &Cx<'_>,
    allowed: &[String],
) -> bool {
    while let Some(id) = current.get() {
        match *cx.kind(id) {
            NodeKind::Cbase => return namespace_consumed(expected),
            NodeKind::Const { scope, name } => {
                let segment = cx.symbol_str(name);
                if let Some(last) = expected.last()
                    && (segment == last.as_str() || match_acronym(last, segment, allowed))
                {
                    expected.pop();
                }
                current = scope;
            }
            _ => return false,
        }
    }
    false
}

/// `partial_matcher!` applied to a full ancestor const: match its short name
/// first, then drain its scope chain.
fn partial_match_full(
    scope: OptNodeId,
    name: &str,
    expected: &mut Vec<String>,
    cx: &Cx<'_>,
    allowed: &[String],
) -> bool {
    if let Some(last) = expected.last()
        && (name == last.as_str() || match_acronym(last, name, allowed))
    {
        expected.pop();
    }
    partial_match_scope(scope, expected, cx, allowed)
}

#[cfg(test)]
mod tests {
    use super::{FileName, Options, basename, filename_good, to_module_name, to_namespace};
    use murphy_plugin_api::test_support::test;

    // --- pure-function matrix (basename normalization + snake_case rule),
    //     ground truth = rubocop 1.87.0 (verified directly). ---

    #[test]
    fn snake_case_basenames_are_good() {
        assert!(filename_good("good_name.rb", None));
        assert!(filename_good("foo.rb", None));
        assert!(filename_good("snake_case_with_digits123.rb", None));
        // `?`/`!` are allowed by SNAKE_CASE.
        assert!(filename_good("foo?.rb", None));
        assert!(filename_good("foo!.rb", None));
        // all-digits basename.
        assert!(filename_good("123.rb", None));
        // only the LAST extension is stripped; `.` is allowed in SNAKE_CASE,
        // so multi-dot names are good.
        assert!(filename_good("foo.bar.rb", None));
        // one leading dot is deleted, then the extension stripped.
        assert!(filename_good(".hidden.rb", None));
        // first `+` becomes `_`.
        assert!(filename_good("foo+bar.rb", None));
    }

    #[test]
    fn non_snake_case_basenames_are_bad() {
        assert!(!filename_good("badName.rb", None));
        assert!(!filename_good("Foo.rb", None));
        assert!(!filename_good("UPPER.rb", None));
        assert!(!filename_good("with-dash.rb", None));
        assert!(!filename_good("with space.rb", None));
        // `sub('+', '_')` replaces only the FIRST `+`; a second `+` survives
        // and fails SNAKE_CASE.
        assert!(!filename_good("foo+bar+baz.rb", None));
    }

    #[test]
    fn basename_takes_last_path_component() {
        assert_eq!(basename("/tmp/BadName.rb"), "BadName.rb");
        assert_eq!(basename("lib/foo/bar.rb"), "bar.rb");
        assert_eq!(basename("bar.rb"), "bar.rb");
        assert_eq!(basename("a\\b\\Win.rb"), "Win.rb");
    }

    #[test]
    fn to_module_name_strips_from_first_dot() {
        assert_eq!(to_module_name("bar.rb"), "Bar");
        assert_eq!(to_module_name("foo.bar.rb"), "Foo");
        assert_eq!(to_module_name("foo_bar.rb"), "FooBar");
        assert_eq!(to_module_name("api_client.rb"), "ApiClient");
        assert_eq!(to_module_name("foo123.rb"), "Foo123");
    }

    #[test]
    fn to_namespace_uses_rightmost_root() {
        let roots = vec![
            "lib".to_string(),
            "spec".to_string(),
            "test".to_string(),
            "src".to_string(),
        ];
        assert_eq!(to_namespace("foo_bar.rb", &roots), vec!["FooBar"]);
        assert_eq!(
            to_namespace("lib/foo/bar.rb", &roots),
            vec!["Foo".to_string(), "Bar".to_string()]
        );
        // Closest root to the file wins (`/home/user/src/project/lib/...`).
        assert_eq!(
            to_namespace("/home/user/src/project_name/lib/foo.rb", &roots),
            vec!["Foo".to_string()]
        );
    }

    // --- end-to-end through the file-path-aware harness. The source body is
    //     decoupled from the on-disk path, so the Ruby content is arbitrary
    //     while `with_file_path` carries the path under test. ---

    #[test]
    fn flags_camel_case_file_name() {
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_offense(
                "x = 1\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n",
            );
    }

    #[test]
    fn flags_dashed_file_name_with_full_path() {
        // A full path is reduced to its basename before the check.
        test::<FileName>()
            .with_file_path("/tmp/lib/with-dash.rb")
            .expect_offense(
                "x = 1\n\
                 ^ The name of this source file (`with-dash.rb`) should use snake_case.\n",
            );
    }

    #[test]
    fn accepts_snake_case_file_name() {
        test::<FileName>()
            .with_file_path("good_name.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn accepts_multi_dot_file_name() {
        test::<FileName>()
            .with_file_path("foo.bar.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn ignores_executable_script_by_default() {
        // Default IgnoreExecutableScripts: true — a shebang suppresses the
        // bad-name offense.
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_no_offenses("#!/usr/bin/env ruby\nx = 1\n");
    }

    #[test]
    fn checks_executable_script_when_option_disabled() {
        test::<FileName>()
            .with_options(&Options {
                ignore_executable_scripts: false,
                ..Default::default()
            })
            .with_file_path("BadName.rb")
            .expect_offense(
                "#!/usr/bin/env ruby\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n\
                 x = 1\n",
            );
    }

    #[test]
    fn empty_file_path_is_not_flagged() {
        // The host returns an empty `file_path` when it cannot expose one
        // (e.g. stdin); an empty basename must not produce a nonsense offense.
        test::<FileName>()
            .with_file_path("")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn regular_hash_comment_is_not_a_shebang() {
        // A leading `#` that is not `#!` does not count as a shebang.
        test::<FileName>()
            .with_file_path("BadName.rb")
            .expect_offense(
                "# frozen_string_literal: true\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n\
                 x = 1\n",
            );
    }

    // --- ExpectMatchingDefinition (opt-in, default false). ---

    #[test]
    fn definition_check_off_by_default() {
        // `foo_bar.rb` with no matching class: no offense unless opted in.
        test::<FileName>()
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn accepts_matching_class() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("class FooBar; end\n");
    }

    #[test]
    fn flags_missing_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_offense(
                "x = 1\n\
                 ^ `foo_bar.rb` should define a class or module called `FooBar`.\n",
            );
    }

    #[test]
    fn flags_wrong_class_name() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_offense(
                "class Wrong; end\n\
                 ^ `foo_bar.rb` should define a class or module called `FooBar`.\n",
            );
    }

    #[test]
    fn bad_filename_wins_over_definition_check() {
        // A bad filename reports snake_case and skips the definition check.
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("BadName.rb")
            .expect_offense(
                "class Foo; end\n\
                 ^ The name of this source file (`BadName.rb`) should use snake_case.\n",
            );
    }

    #[test]
    fn shebang_does_not_suppress_missing_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_offense(
                "#!/usr/bin/env ruby\n\
                 ^ `foo_bar.rb` should define a class or module called `FooBar`.\n\
                 class Wrong; end\n",
            );
    }

    #[test]
    fn accepts_nested_hierarchy() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("lib/foo/bar.rb")
            .expect_no_offenses("module Foo; class Bar; end; end\n");
    }

    #[test]
    fn flags_hierarchy_mismatch_with_full_namespace() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("lib/foo/bar.rb")
            .expect_offense(
                "class Bar; end\n\
                 ^ `bar.rb` should define a class or module called `Foo::Bar`.\n",
            );
    }

    #[test]
    fn accepts_compact_namespace() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("lib/foo/bar.rb")
            .expect_no_offenses("class Foo::Bar; end\n");
    }

    #[test]
    fn hierarchy_disabled_checks_basename_only() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                check_definition_path_hierarchy: false,
                ..Default::default()
            })
            .with_file_path("lib/foo/bar.rb")
            .expect_no_offenses("class Bar; end\n");
    }

    #[test]
    fn accepts_struct_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("FooBar = Struct.new(:x)\n");
    }

    #[test]
    fn accepts_struct_block_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("FooBar = Struct.new do\n  def x; end\nend\n");
    }

    #[test]
    fn accepts_class_new_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("FooBar = Class.new\n");
    }

    #[test]
    fn accepts_module_new_block_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_no_offenses("FooBar = Module.new do\n  def x; end\nend\n");
    }

    #[test]
    fn rejects_data_define_as_definition() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("foo_bar.rb")
            .expect_offense(
                "FooBar = Data.define(:x)\n\
                 ^ `foo_bar.rb` should define a class or module called `FooBar`.\n",
            );
    }

    #[test]
    fn acronym_tolerance_by_default() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("api_client.rb")
            .expect_no_offenses("class APIClient; end\n");
    }

    #[test]
    fn empty_acronyms_reject_all_caps() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                allowed_acronyms: vec![],
                ..Default::default()
            })
            .with_file_path("api_client.rb")
            .expect_offense(
                "class APIClient; end\n\
                 ^ `api_client.rb` should define a class or module called `ApiClient`.\n",
            );
    }

    #[test]
    fn custom_hierarchy_roots() {
        test::<FileName>()
            .with_options(&Options {
                expect_matching_definition: true,
                check_definition_path_hierarchy_roots: vec!["app".to_string()],
                ..Default::default()
            })
            .with_file_path("app/models/bar.rb")
            .expect_no_offenses("class Models::Bar; end\n");
    }

    // --- Regex (custom pattern overriding SNAKE_CASE). ---

    #[test]
    fn regex_matching_filename_is_accepted() {
        test::<FileName>()
            .with_options(&Options {
                regex: Some("^bad".to_string()),
                ..Default::default()
            })
            .with_file_path("bad.rb")
            .expect_no_offenses("x = 1\n");
    }

    #[test]
    fn regex_mismatch_reports_match_message() {
        test::<FileName>()
            .with_options(&Options {
                regex: Some("^foo".to_string()),
                ..Default::default()
            })
            .with_file_path("bad.rb")
            .expect_offense("x = 1\n^ `bad.rb` should match `^foo`.\n");
    }

    #[test]
    fn regex_good_filename_still_checks_definition() {
        test::<FileName>()
            .with_options(&Options {
                regex: Some("^bad".to_string()),
                expect_matching_definition: true,
                ..Default::default()
            })
            .with_file_path("bad.rb")
            .expect_offense(
                "x = 1\n\
                 ^ `bad.rb` should define a class or module called `Bad`.\n",
            );
    }

    #[test]
    fn invalid_regex_is_non_matching() {
        test::<FileName>()
            .with_options(&Options {
                regex: Some("([".to_string()),
                ..Default::default()
            })
            .with_file_path("good_name.rb")
            .expect_offense("x = 1\n^ `good_name.rb` should match `([`.\n");
    }
}
murphy_plugin_api::submit_cop!(FileName);
