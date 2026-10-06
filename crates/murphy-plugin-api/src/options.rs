//! The `CopOptions` trait: a cop's typed view of its config table.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;

use crate::abi::OptionSpec;
use crate::config_error::ConfigError;

/// Thread-local decode cache for cop options (murphy-l7i0.1).
///
/// `options_json` bytes are constant per (cop, run) while cops decode them
/// on every matching node. Keyed by `(TypeId, buffer address, length)` with
/// the bytes snapshotted for content verification, so a reused address with
/// different content can never return a stale value.
struct OptionsCacheEntry {
    bytes: Vec<u8>,
    value: Box<dyn Any>,
}

thread_local! {
    static OPTIONS_CACHE: RefCell<HashMap<(TypeId, *const u8, usize), OptionsCacheEntry>> =
        RefCell::new(HashMap::new());
}

/// Decode cop options with a per-thread cache (murphy-l7i0.1).
///
/// The first call for distinct `(T, bytes)` parses via
/// [`CopOptions::from_config_json`]; repeats after that pay only a map
/// lookup, a byte compare, and a `Clone` — no serde. Requires `T: Clone`
/// (emitted automatically by `#[derive(CopOptions)]`; hand-written option
/// structs need `#[derive(Clone)]` or equivalent).
pub fn from_config_json_cached<T: CopOptions + Clone>(bytes: &[u8]) -> Result<T, ConfigError> {
    let key = (TypeId::of::<T>(), bytes.as_ptr(), bytes.len());
    OPTIONS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.get(&key)
            && entry.bytes.as_slice() == bytes
            && let Some(value) = entry.value.downcast_ref::<T>()
        {
            return Ok(value.clone());
        }
        let value = T::from_config_json(bytes)?;
        cache.insert(
            key,
            OptionsCacheEntry {
                bytes: bytes.to_vec(),
                value: Box::new(value.clone()),
            },
        );
        Ok(value)
    })
}

/// A cop's option struct, backing its `[cops.rules."Name"]` table.
///
/// `Default` lets the runtime hand a cop an `Options` value even with no
/// user config. `SCHEMA` is an associated `const` so it is readable from
/// `static` / `const fn` contexts (what `register_cops!` — murphy-9cr.21
/// — needs). `#[derive(CopOptions)]` (murphy-9cr.21) overrides
/// `from_config_json` (and `to_config_json`) with field-by-field
/// (de)coding.
pub trait CopOptions: Default + Sized + 'static {
    /// Static schema, one entry per option. Empty for [`NoOptions`].
    const SCHEMA: &'static [OptionSpec] = &[];

    /// Decode an `Options` value from the cop's config table (a JSON
    /// object). The default ignores the input and returns [`Default`],
    /// correct for cops that take no configuration.
    fn from_config_json(_bytes: &[u8]) -> Result<Self, ConfigError> {
        Ok(Self::default())
    }

    /// Encode this `Options` value back to a JSON object matching the
    /// `[cops.rules."Name"]` wire format that `from_config_json` would
    /// accept. The default returns the empty object `"{}"` — correct
    /// for cops that take no configuration, and a safe fallback for
    /// hand-implemented options that opt not to round-trip. The
    /// `#[derive(CopOptions)]` impl walks the fields one-for-one so a
    /// roundtrip through `to_config_json` ↦ `from_config_json` recovers
    /// the original value.
    ///
    /// The test harness uses this to hand a typed `Options` value to a
    /// cop without making test code construct raw JSON.
    fn to_config_json(&self) -> String {
        String::from("{}")
    }
}

/// String-backed enum option metadata used by `#[derive(CopOptions)]`.
///
/// `#[derive(CopOptionEnum)]` implements this trait for enums whose variants
/// carry `#[option(value = "...")]` wire values.
pub trait CopOptionEnum: Copy + Sized + 'static {
    /// Allowed wire values as plain strings.
    const VALUES: &'static [&'static str];

    /// Allowed wire values encoded as a JSON array for [`OptionSpec`].
    const VALUES_JSON: &'static str;

    /// Convert a user-provided wire value into the typed enum.
    fn from_str(value: &str) -> Option<Self>;

    /// Return this variant's wire value.
    fn as_str(self) -> &'static str;
}

/// Marker for cops that declare no options.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOptions;

impl CopOptions for NoOptions {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Intentionally asserts the schema is empty; the const-known `is_empty`
    // is the point of the test (`const_is_empty` under `-D warnings`).
    #[allow(clippy::const_is_empty)]
    fn no_options_has_empty_schema_and_ignores_input() {
        assert!(<NoOptions as CopOptions>::SCHEMA.is_empty());
        assert!(<NoOptions as CopOptions>::from_config_json(b"not json").is_ok());
    }
}
