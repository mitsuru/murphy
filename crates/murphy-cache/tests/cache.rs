//! Integration tests for `murphy-cache` (murphy-9cr.26-c).

use murphy_ast::{AstBuilder, NodeKind, OptNodeId, Range, content_hash};
use murphy_cache::Cache;

fn small_ast(source: &str) -> murphy_ast::Ast {
    let mut b = AstBuilder::new(source, "t.rb");
    let int = b.push(
        NodeKind::Int(1),
        Range {
            start: 4,
            end: source.len() as u32,
        },
    );
    let name = b.intern_symbol("x");
    let asgn = b.push(
        NodeKind::Lvasgn {
            name,
            value: OptNodeId::some(int),
        },
        Range {
            start: 0,
            end: source.len() as u32,
        },
    );
    let list = b.push_list(&[asgn]);
    let root = b.push(
        NodeKind::Begin(list),
        Range {
            start: 0,
            end: source.len() as u32,
        },
    );
    b.finish(root)
}

fn unique_tempdir() -> std::path::PathBuf {
    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = base.join(format!("murphy-cache-test-{stamp}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn open_in_creates_root_directory() {
    let root = unique_tempdir().join("nested/deeper");
    assert!(!root.exists());
    let _cache = Cache::open_in(root.clone(), 1);
    assert!(root.is_dir(), "Cache::open_in must mkdir -p the root");
}

#[test]
fn put_then_lookup_returns_same_ast() {
    let root = unique_tempdir();
    let cache = Cache::open_in(root, 1);
    let ast = small_ast("x = 1");
    let hash = content_hash(ast.source().as_bytes());

    cache.put(&hash, &ast);
    let restored = cache.lookup(&hash).expect("must hit");
    assert_eq!(ast, restored);
}

#[test]
fn lookup_with_unknown_hash_returns_none() {
    let root = unique_tempdir();
    let cache = Cache::open_in(root, 1);
    let unknown = [0u8; 32];
    assert!(cache.lookup(&unknown).is_none());
}

#[test]
fn lookup_on_corrupt_file_returns_none_not_panic() {
    let root = unique_tempdir();
    let cache = Cache::open_in(root, 1);
    let ast = small_ast("y = 2");
    let hash = content_hash(ast.source().as_bytes());

    cache.put(&hash, &ast);
    // Corrupt every cache file under the root.
    for file in walk(cache.root()) {
        let mut bytes = std::fs::read(&file).unwrap();
        if !bytes.is_empty() {
            bytes[0] ^= 0xFF; // smash the magic
            std::fs::write(&file, bytes).unwrap();
        }
    }
    assert!(cache.lookup(&hash).is_none());
}

#[test]
fn lookup_with_different_layer_version_misses() {
    // Caches keyed under v=1 must not be returned by a Cache opened at v=2.
    let root = unique_tempdir();
    let cache_v1 = Cache::open_in(root.clone(), 1);
    let ast = small_ast("z = 3");
    let hash = content_hash(ast.source().as_bytes());
    cache_v1.put(&hash, &ast);
    drop(cache_v1);

    let cache_v2 = Cache::open_in(root, 2);
    assert!(
        cache_v2.lookup(&hash).is_none(),
        "different layer_version must miss"
    );
}

#[test]
fn version_key_differs_for_different_layer_versions() {
    let v1 = Cache::open_in(unique_tempdir(), 1);
    let v2 = Cache::open_in(unique_tempdir(), 2);
    assert_ne!(v1.version_key(), v2.version_key());
}

#[test]
fn open_returns_none_when_disabled_by_env() {
    // SAFETY: tests run in parallel inside the same process. We use a
    // dedicated, non-clashing variable name and restore it before exit so
    // the rest of the suite is unaffected.
    let key = "MURPHY_NO_CACHE";
    let prior = std::env::var_os(key);
    // SAFETY: documented above.
    unsafe { std::env::set_var(key, "1") };
    let cache = Cache::open(1);
    match prior {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }
    assert!(
        cache.is_none(),
        "MURPHY_NO_CACHE must disable cache via Cache::open"
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
