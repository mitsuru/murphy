//! Integration tests for `ResultCache` (A5, murphy-fmw.1.1).

use murphy_cache::{ResultCache, derive_result_version_key};

fn unique_tempdir() -> std::path::PathBuf {
    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    base.join(format!(
        "murphy-result-cache-test-{stamp}-{}",
        std::process::id()
    ))
}

fn extra(v: u8) -> [u8; 32] {
    [v; 32]
}

#[test]
fn put_then_lookup_returns_same_bytes() {
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root, 1, &extra(7));
    let hash = [9u8; 32];
    let payload = br#"[{"file":"a.rb"}]"#;
    assert!(cache.lookup(&hash, "a.rb").is_none());
    cache.put(&hash, "a.rb", payload);
    assert_eq!(
        cache.lookup(&hash, "a.rb").as_deref(),
        Some(payload.as_slice())
    );
}

#[test]
fn lookup_with_unknown_hash_returns_none() {
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root, 1, &extra(1));
    assert!(cache.lookup(&[0u8; 32], "a.rb").is_none());
}

#[test]
fn different_extra_fingerprint_misses() {
    let root = unique_tempdir();
    let a = ResultCache::open_in(root.clone(), 1, &extra(1));
    let hash = [5u8; 32];
    a.put(&hash, "a.rb", b"[]");
    drop(a);
    let b = ResultCache::open_in(root, 1, &extra(2));
    assert!(
        b.lookup(&hash, "a.rb").is_none(),
        "different cop/config fingerprint must miss"
    );
}

#[test]
fn different_layer_version_misses() {
    let root = unique_tempdir();
    let a = ResultCache::open_in(root.clone(), 1, &extra(3));
    let hash = [6u8; 32];
    a.put(&hash, "a.rb", b"[1]");
    drop(a);
    let b = ResultCache::open_in(root, 2, &extra(3));
    assert!(b.lookup(&hash, "a.rb").is_none());
}

#[test]
fn empty_payload_is_rejected() {
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root, 1, &extra(4));
    let hash = [7u8; 32];
    cache.put(&hash, "a.rb", b"");
    assert!(cache.lookup(&hash, "a.rb").is_none());
}

#[test]
fn oversize_payload_is_rejected() {
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root, 1, &extra(5));
    let hash = [8u8; 32];
    let big = vec![b'x'; murphy_cache::MAX_RESULT_BYTES + 1];
    cache.put(&hash, "a.rb", &big);
    assert!(cache.lookup(&hash, "a.rb").is_none());
}

#[test]
fn corrupt_file_returns_bytes_for_caller_to_reject() {
    // `ResultCache` is bytes-opaque: garbage bytes are returned so the
    // caller (JSON deserialize) can reject them as a miss. The cache
    // itself never panics on content.
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root.clone(), 1, &extra(6));
    let hash = [10u8; 32];
    cache.put(&hash, "a.rb", b"not json{{{");
    let got = cache.lookup(&hash, "a.rb").expect("bytes round-trip");
    assert!(serde_json::from_slice::<serde_json::Value>(&got).is_err());
    let _ = root;
}

#[test]
fn open_returns_none_when_disabled_by_env() {
    let key = "MURPHY_NO_CACHE";
    let prior = std::env::var_os(key);
    // SAFETY: dedicated key, restored before exit.
    unsafe { std::env::set_var(key, "1") };
    let cache = ResultCache::open(&extra(1), 1);
    match prior {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }
    assert!(cache.is_none());
}

#[test]
fn derive_result_version_key_binds_both_inputs() {
    let a = derive_result_version_key(1, &extra(1));
    let b = derive_result_version_key(2, &extra(1));
    let c = derive_result_version_key(1, &extra(2));
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(a, derive_result_version_key(1, &extra(1)));
}

#[test]
fn same_content_different_paths_do_not_share() {
    // Identical content at different paths (e.g. pack Exclude scopes)
    // must not share: per-path keys.
    let root = unique_tempdir();
    let cache = ResultCache::open_in(root, 1, &extra(8));
    let hash = [11u8; 32];
    cache.put(&hash, "spec/requests/a.rb", b"[]");
    assert!(
        cache.lookup(&hash, "spec/models/a.rb").is_none(),
        "different paths must not share entries"
    );
    assert!(cache.lookup(&hash, "spec/requests/a.rb").is_some());
}
