//! Flat string interner shared by [`Symbol`] (identifiers) and
//! [`StringId`] (string-literal contents). See design §4.

use std::collections::HashMap;

use crate::node::Range;

/// A finished, serializable interner: a flat byte blob plus per-entry
/// offsets. Index it with the `u32` inside a [`Symbol`] or [`StringId`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Interner {
    pub(crate) blob: Vec<u8>,
    pub(crate) offsets: Vec<Range>,
}

impl Interner {
    /// Resolve an entry index to its string.
    pub fn resolve(&self, index: u32) -> &str {
        let r = self.offsets[index as usize];
        // Only valid UTF-8 is ever interned (see `InternBuilder::intern`);
        // `from_bytes` validates on the deserialization path.
        std::str::from_utf8(&self.blob[r.start as usize..r.end as usize])
            .expect("interner blob holds valid UTF-8")
    }

    /// Number of interned entries.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// `true` iff nothing has been interned.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
}

/// Build-time interner with deduplication. The `dedup` map is dropped by
/// [`InternBuilder::finish`]; only the flat [`Interner`] survives.
#[derive(Debug, Default)]
pub struct InternBuilder {
    interner: Interner,
    dedup: HashMap<String, u32>,
}

impl InternBuilder {
    /// Intern a string, returning its entry index. Repeated strings return
    /// the same index.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.dedup.get(s) {
            return idx;
        }
        let start = self.interner.blob.len() as u32;
        self.interner.blob.extend_from_slice(s.as_bytes());
        let end = self.interner.blob.len() as u32;
        let idx = self.interner.offsets.len() as u32;
        self.interner.offsets.push(Range { start, end });
        self.dedup.insert(s.to_owned(), idx);
        idx
    }

    /// Consume the builder, returning the flat interner.
    pub fn finish(self) -> Interner {
        self.interner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplicates() {
        let mut b = InternBuilder::default();
        let a1 = b.intern("call");
        let a2 = b.intern("call");
        let other = b.intern("new");
        assert_eq!(a1, a2, "same string interns to the same index");
        assert_ne!(a1, other);
        let interner = b.finish();
        assert_eq!(interner.len(), 2);
        assert_eq!(interner.resolve(a1), "call");
        assert_eq!(interner.resolve(other), "new");
    }

    #[test]
    fn empty_interner() {
        let interner = InternBuilder::default().finish();
        assert!(interner.is_empty());
    }
}
