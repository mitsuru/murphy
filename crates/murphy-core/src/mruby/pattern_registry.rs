use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use murphy_pattern::{ParseError, PatternIr};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(crate) struct PatternIrRegistry {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    by_hash: HashMap<u64, Vec<u32>>,
    by_handle: Vec<PatternIrEntry>,
}

struct PatternIrEntry {
    src: String,
    ir: Arc<PatternIr>,
}

impl PatternIrRegistry {
    pub(crate) fn global() -> &'static Self {
        static REGISTRY: OnceLock<PatternIrRegistry> = OnceLock::new();
        REGISTRY.get_or_init(PatternIrRegistry::default)
    }

    pub(crate) fn intern(&self, src: &str) -> Result<u32, ParseError> {
        let hash = hash_pattern(src);
        self.intern_with_hash(src, hash)
    }

    fn intern_with_hash(&self, src: &str, hash: u64) -> Result<u32, ParseError> {
        {
            let inner = self.inner.read().expect("pattern registry lock poisoned");
            if let Some(handle) = inner
                .by_hash
                .get(&hash)
                .and_then(|bucket| find_matching_source(&inner, bucket, src))
            {
                return Ok(handle);
            }
        }

        let ir = Arc::new(murphy_pattern::compile(src)?);
        let mut inner = self.inner.write().expect("pattern registry lock poisoned");
        if let Some(handle) = inner
            .by_hash
            .get(&hash)
            .and_then(|bucket| find_matching_source(&inner, bucket, src))
        {
            return Ok(handle);
        }

        let handle = inner.by_handle.len() as u32;
        inner.by_handle.push(PatternIrEntry {
            src: src.to_owned(),
            ir,
        });
        inner.by_hash.entry(hash).or_default().push(handle);
        Ok(handle)
    }

    #[cfg(test)]
    fn intern_with_hash_for_test(&self, src: &str, hash: u64) -> Result<u32, ParseError> {
        self.intern_with_hash(src, hash)
    }

    // Used by the upcoming `Murphy.match` primitive; tested now with the
    // registry so handle lifetime semantics are fixed before match dispatch.
    #[allow(dead_code)]
    pub(crate) fn get(&self, handle: u32) -> Option<Arc<PatternIr>> {
        self.inner
            .read()
            .expect("pattern registry lock poisoned")
            .by_handle
            .get(handle as usize)
            .map(|entry| Arc::clone(&entry.ir))
    }
}

fn find_matching_source(inner: &Inner, bucket: &[u32], src: &str) -> Option<u32> {
    bucket.iter().copied().find(|&handle| {
        inner
            .by_handle
            .get(handle as usize)
            .is_some_and(|entry| entry.src == src)
    })
}

fn hash_pattern(src: &str) -> u64 {
    let digest = Sha256::digest(src.as_bytes());
    u64::from_le_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 digest is long enough"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_reuses_the_same_handle_for_the_same_pattern_source() {
        let registry = PatternIrRegistry::default();

        let first = registry.intern("(send nil? :puts $...)").unwrap();
        let second = registry.intern("(send nil? :puts $...)").unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn intern_distinguishes_different_sources_with_the_same_hash_bucket() {
        let registry = PatternIrRegistry::default();

        let first = registry.intern_with_hash_for_test("_", 42).unwrap();
        let second = registry
            .intern_with_hash_for_test("(send nil? :puts)", 42)
            .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn get_returns_the_shared_ir_for_a_known_handle() {
        let registry = PatternIrRegistry::default();
        let handle = registry.intern("_").unwrap();

        let first = registry.get(handle).unwrap();
        let second = registry.get(handle).unwrap();

        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert!(registry.get(u32::MAX).is_none());
    }
}
