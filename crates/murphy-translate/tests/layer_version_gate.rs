//! LAYER_VERSION bump gate (murphy-57i0).
//!
//! Translate-output changes (e.g. a prism node newly mapping to a real
//! `NodeKind` instead of `Unknown`) require a manual
//! `murphy_translate::LAYER_VERSION` bump to invalidate the arena binary
//! cache. Unit tests parse fresh (no on-disk cache), so they stay green
//! while a warm cache serves stale ASTs.
//!
//! This gate snapshots the prism->NodeKind mapping as a corpus of Ruby
//! snippets -> S-expression outputs, tied to `LAYER_VERSION`. If the mapping
//! changes without a version bump, the test fails with an explicit
//! "bump LAYER_VERSION" message. If the version is bumped (or mapping
//! changed), the snapshot must be re-blessed:
//!
//! ```sh
//! BLESS=1 cargo test -p murphy-translate --test layer_version_gate
//! ```
//!
//! Workflow when this test fails:
//! 1. If message says mapping changed without a bump: bump `LAYER_VERSION`
//!    in `crates/murphy-translate/src/lib.rs` (and update the anchor test
//!    `layer_version_is_initialized`), then re-bless.
//! 2. If message says snapshot stale: re-bless with `BLESS=1` and commit the
//!    updated `tests/snapshots/translate_mapping.snap` alongside the bump.
//!
//! The corpus intentionally includes constructs that currently lower to
//! `Unknown` (e.g. `redo`, `__FILE__`, huge integers). A future
//! `Unknown` -> real-`NodeKind` lowering changes their sexp, so the gate
//! fires even for "new support" changes like the `retry` -> `Retry`
//! fix (murphy-l1iy) that originally shipped without its bump.

use murphy_ast::ast_to_sexp;
use std::path::PathBuf;

/// (name, ruby source). All sources are valid Ruby; prism is total so even
/// edge inputs translate without panic. Keep sources single-line where
/// possible (`;` instead of newlines) for snapshot readability; multi-line
/// sources are escaped with `{:?}` in the snapshot.
fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        // --- atoms / literals ---
        ("nil_literal", "nil"),
        ("true_literal", "true"),
        ("false_literal", "false"),
        ("self_expr", "self"),
        ("int_literal", "42"),
        ("float_literal", "3.14"),
        ("string_literal", "\"hello\""),
        ("symbol_literal", ":foo"),
        ("rational_literal", "1r"),
        ("complex_literal", "1i"),
        ("huge_int_unknown", "99999999999999999999999999"),
        // --- variable reads ---
        ("lvar_read", "x = 1; x"),
        ("ivar_read", "@x"),
        ("cvar_read", "@@x"),
        ("gvar_read", "$x"),
        ("backref_read", "$&"),
        ("gvar_tilde", "$~"),
        ("nth_ref", "$1"),
        ("const_read", "Foo"),
        ("const_path", "Foo::Bar"),
        ("cbase", "::Foo"),
        // --- assignments ---
        ("lvasgn", "x = 1"),
        ("ivasgn", "@x = 1"),
        ("cvasgn", "@@x = 1"),
        ("gvasgn", "$x = 1"),
        ("casgn_simple", "Foo = 1"),
        ("casgn_scoped", "A::B = 1"),
        ("masgn", "a, b = 1, 2"),
        ("op_asgn", "x = 1; x += 2"),
        ("or_asgn", "x = 1; x ||= 2"),
        ("and_asgn", "x = 1; x &&= 2"),
        ("index_or_asgn", "h = {}; h[:k] ||= 1"),
        ("const_path_op_asgn", "A::B += 1"),
        ("destructured_param", "def f((a, b)); end"),
        // --- calls / blocks ---
        ("send_simple", "foo(1, 2)"),
        ("csend", "foo&.bar"),
        ("index_call", "a[1, 2]"),
        ("index_asgn", "a[1] = 2"),
        ("block", "foo { |x| x }"),
        ("block_local", "items.each { |item; blacklist| nil }"),
        ("numblock", "foo { _1 + _2 }"),
        ("itblock", "foo { it }"),
        ("lambda_arrow", "->(x) { x }"),
        ("yield_call", "def f; yield 1; end"),
        ("super_args", "def f; super(1); end"),
        ("super_bare", "def f; super; end"),
        // --- collections ---
        ("array", "[1, 2, 3]"),
        ("hash_rocket", "{ :a => 1 }"),
        ("hash_label", "{ a: 1 }"),
        ("splat_array", "[*a]"),
        ("kwsplat_call", "foo(**opts)"),
        ("assoc_splat_hash", "{ a: 1, **opts }"),
        // --- control flow ---
        ("if_else", "if x; 1; else; 2; end"),
        ("unless", "unless x; 1; end"),
        ("while", "while x; 1; end"),
        ("until", "until x; 1; end"),
        ("while_post", "begin; 1; end while x"),
        ("for_loop", "for i in [1, 2]; i; end"),
        ("case_when", "case x; when 1; 2; else; 3; end"),
        ("and_op", "a && b"),
        ("or_op", "a || b"),
        ("not_kw", "not x"),
        ("return_kw", "def f; return 1; end"),
        ("break_kw", "while x; break; end"),
        ("next_kw", "while x; next; end"),
        ("retry_kw", "begin; 1; rescue; retry; end"),
        // Currently Unknown -> gate must fire when it becomes real (cf. retry).
        ("redo_kw", "while x; redo; end"),
        ("defined_kw", "defined?(x)"),
        // --- exceptions ---
        ("rescue_modifier", "1 rescue 2"),
        (
            "rescue_else_ensure",
            "begin; 1; rescue; 2; else; 3; ensure; 4; end",
        ),
        // --- definitions ---
        ("def_simple", "def foo(a); a; end"),
        ("defs_self", "def self.foo; 1; end"),
        ("class_simple", "class Foo; end"),
        ("sclass", "class << self; end"),
        ("module_simple", "module M; end"),
        ("alias_method", "alias foo bar"),
        ("alias_global", "alias $foo $bar"),
        ("undef_kw", "undef foo"),
        ("preexe", "BEGIN { 1 }"),
        ("postexe", "END { 1 }"),
        // --- strings / regexp / ranges ---
        ("dstr", "\"a#{b}c\""),
        ("dsym", ":\"a#{b}\""),
        ("xstr", "`cmd`"),
        ("regexp_plain", "/abc/"),
        ("regexp_interp_flags", "/a#{b}/i"),
        ("range_inclusive", "1..10"),
        ("range_exclusive", "1...10"),
        ("flip_flop", "if a..b; 1; end"),
        // --- pattern matching ---
        ("case_match", "case x; in 1; 2; else; 3; end"),
        ("in_guard_if", "case x; in 1 if y; 2; end"),
        ("in_guard_unless", "case x; in 1 unless y; 2; end"),
        ("array_pattern", "case x; in [1, 2]; 3; end"),
        ("hash_pattern", "case x; in { a: 1 }; 2; end"),
        ("find_pattern", "case x; in [*, 1, *]; 2; end"),
        ("match_alt", "case x; in 1 | 2; 3; end"),
        ("match_rest", "case x; in [1, *rest]; 2; end"),
        ("match_nil_pattern", "case x; in { a: 1, **nil }; 2; end"),
        ("match_pattern_p", "x in [1, 2]"),
        ("match_pattern", "x => [1, 2]"),
        ("match_as", "case x; in [1] => y; 2; end"),
        ("const_pattern", "case x; in Foo[1]; 2; end"),
        ("pin_var", "case x; in ^y; 1; end"),
        ("pin_expr", "case x; in ^(1 + 2); 1; end"),
        // --- regexp match with captures ---
        ("match_with_lvasgn", "/(?<name>foo)/ =~ str"),
        ("parenthesized_pattern", "case x; in (1); 2; end"),
        (
            "shareable_constant",
            "# shareable_constant_value: literal\nBlacklist = []\n",
        ),
        // --- parameters ---
        ("optarg", "def f(a = 1); end"),
        ("restarg", "def f(*r); end"),
        ("kwarg", "def f(k:); end"),
        ("kwoptarg", "def f(k: 1); end"),
        ("kwrestarg", "def f(**o); end"),
        ("blockarg", "def f(&b); end"),
        ("kwnilarg", "def f(**nil); end"),
        ("forward_args_def", "def f(...); end"),
        ("forwarded_args_call", "def f(...); g(...); end"),
        ("shadowarg", "->(a; x) { x }"),
        // --- misc / currently-Unknown probes ---
        ("source_file", "__FILE__"),
        ("source_line", "__LINE__"),
        ("source_encoding", "__ENCODING__"),
        ("match_last_line", "if /re/; 1; end"),
        ("empty_program", ""),
    ]
}

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("translate_mapping.snap")
}

fn render_snapshot() -> String {
    let mut out = String::new();
    out.push_str("# Translate mapping snapshot — DO NOT EDIT BY HAND.\n");
    out.push_str(
        "# Regenerate with: BLESS=1 cargo test -p murphy-translate --test layer_version_gate\n",
    );
    out.push_str("# When prism->NodeKind mapping changes: bump murphy_translate::LAYER_VERSION\n");
    out.push_str("# in crates/murphy-translate/src/lib.rs, then re-bless and commit both.\n");
    out.push_str(&format!(
        "LAYER_VERSION={}\n",
        murphy_translate::LAYER_VERSION
    ));
    for (name, src) in corpus() {
        let ast = murphy_translate::translate(src, format!("{name}.rb"));
        let sexp = ast_to_sexp(&ast);
        out.push_str("---\n");
        out.push_str(&format!("case: {name}\n"));
        out.push_str(&format!("source: {src:?}\n"));
        out.push_str("sexp:\n");
        out.push_str(&sexp);
        out.push('\n');
    }
    out
}

fn parse_snapshot(snap: &str) -> (Option<u32>, std::collections::BTreeMap<String, String>) {
    let mut version = None;
    let mut map = std::collections::BTreeMap::new();
    let mut cur_name: Option<String> = None;
    let mut cur_sexp = String::new();
    let mut in_sexp = false;
    for line in snap.lines() {
        if let Some(v) = line.strip_prefix("LAYER_VERSION=") {
            version = v.trim().parse().ok();
            continue;
        }
        if line == "---" {
            if let Some(name) = cur_name.take() {
                map.insert(name, cur_sexp.clone());
                cur_sexp.clear();
            }
            in_sexp = false;
            continue;
        }
        if let Some(name) = line.strip_prefix("case: ") {
            if let Some(prev) = cur_name.take() {
                map.insert(prev, cur_sexp.clone());
                cur_sexp.clear();
            }
            cur_name = Some(name.trim().to_string());
            in_sexp = false;
            continue;
        }
        if line == "sexp:" {
            in_sexp = true;
            continue;
        }
        if line.starts_with("source: ") || line.starts_with('#') {
            continue;
        }
        if in_sexp {
            cur_sexp.push_str(line);
            cur_sexp.push('\n');
        }
    }
    if let Some(name) = cur_name {
        map.insert(name, cur_sexp);
    }
    // Trim trailing newline per entry for stable comparison (render adds one).
    for v in map.values_mut() {
        if v.ends_with('\n') {
            v.pop();
        }
    }
    (version, map)
}

fn current_map() -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for (name, src) in corpus() {
        let ast = murphy_translate::translate(src, format!("{name}.rb"));
        map.insert(name.to_string(), ast_to_sexp(&ast));
    }
    map
}

#[test]
fn translate_mapping_tied_to_layer_version() {
    let snap_path = snapshot_path();
    let rendered = render_snapshot();
    if std::env::var("BLESS").is_ok() {
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&snap_path, &rendered).unwrap();
        return;
    }
    let expected_raw = std::fs::read_to_string(&snap_path).unwrap_or_else(|_| {
        panic!(
            "missing translate mapping snapshot at {}; run with BLESS=1 to generate it",
            snap_path.display()
        )
    });
    if expected_raw == rendered {
        return;
    }
    let (expected_version, expected_map) = parse_snapshot(&expected_raw);
    let current_version = murphy_translate::LAYER_VERSION;
    let got_map = current_map();

    let mut changed: Vec<String> = Vec::new();
    for (name, want) in &expected_map {
        match got_map.get(name) {
            Some(got) if got != want => changed.push(name.clone()),
            None => changed.push(format!("{name} (removed)")),
            _ => {}
        }
    }
    for name in got_map.keys() {
        if !expected_map.contains_key(name) {
            changed.push(format!("{name} (added)"));
        }
    }
    changed.sort();
    let changed_list = if changed.is_empty() {
        "(no per-case diff; only header/version changed)".to_string()
    } else {
        changed.join(", ")
    };

    match expected_version {
        Some(v) if v == current_version => {
            panic!(
                "translate mapping changed without a LAYER_VERSION bump (still {v}).\n\
                 Changed cases: {changed_list}\n\
                 Bump LAYER_VERSION in crates/murphy-translate/src/lib.rs \
                 (and the layer_version_is_initialized anchor), then re-bless:\n \
                 BLESS=1 cargo test -p murphy-translate --test layer_version_gate"
            );
        }
        Some(v) => {
            panic!(
                "translate mapping snapshot stale (snapshot LAYER_VERSION={v}, current={current_version}).\n\
                 Changed cases: {changed_list}\n\
                 Re-bless and commit the snapshot alongside the bump:\n \
                 BLESS=1 cargo test -p murphy-translate --test layer_version_gate"
            );
        }
        None => {
            panic!(
                "translate mapping snapshot missing LAYER_VERSION header; re-bless:\n \
                 BLESS=1 cargo test -p murphy-translate --test layer_version_gate"
            );
        }
    }
}
