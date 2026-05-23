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
    by_hash: HashMap<u64, u32>,
    by_handle: Vec<Arc<PatternIr>>,
}

impl PatternIrRegistry {
    pub(crate) fn global() -> &'static Self {
        static REGISTRY: OnceLock<PatternIrRegistry> = OnceLock::new();
        REGISTRY.get_or_init(PatternIrRegistry::default)
    }

    pub(crate) fn intern(&self, src: &str) -> Result<u32, ParseError> {
        let hash = hash_pattern(src);
        if let Some(&handle) = self
            .inner
            .read()
            .expect("pattern registry lock poisoned")
            .by_hash
            .get(&hash)
        {
            return Ok(handle);
        }

        let ir = Arc::new(murphy_pattern::compile(src)?);
        let mut inner = self.inner.write().expect("pattern registry lock poisoned");
        if let Some(&handle) = inner.by_hash.get(&hash) {
            return Ok(handle);
        }

        let handle = inner.by_handle.len() as u32;
        inner.by_handle.push(ir);
        inner.by_hash.insert(hash, handle);
        Ok(handle)
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
            .cloned()
    }
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
    fn get_returns_the_shared_ir_for_a_known_handle() {
        let registry = PatternIrRegistry::default();
        let handle = registry.intern("_").unwrap();

        let first = registry.get(handle).unwrap();
        let second = registry.get(handle).unwrap();

        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert!(registry.get(u32::MAX).is_none());
    }
}
