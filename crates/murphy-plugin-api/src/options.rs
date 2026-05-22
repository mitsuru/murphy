//! The `CopOptions` trait: a cop's typed view of its config table.

use crate::abi::OptionSpec;
use crate::config_error::ConfigError;

/// A cop's option struct, backing its `[cops.rules."Name"]` table.
///
/// `Default` lets the runtime hand a cop an `Options` value even with no
/// user config. `SCHEMA` is an associated `const` so it is readable from
/// `static` / `const fn` contexts (what `register_cops!` — murphy-9cr.21
/// — needs). `#[derive(CopOptions)]` (murphy-9cr.21) overrides
/// `from_config_json` with field-by-field decoding.
pub trait CopOptions: Default + Sized + 'static {
    /// Static schema, one entry per option. Empty for [`NoOptions`].
    const SCHEMA: &'static [OptionSpec] = &[];

    /// Decode an `Options` value from the cop's config table (a JSON
    /// object). The default ignores the input and returns [`Default`],
    /// correct for cops that take no configuration.
    fn from_config_json(_bytes: &[u8]) -> Result<Self, ConfigError> {
        Ok(Self::default())
    }
}

/// Marker for cops that declare no options.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOptions;

impl CopOptions for NoOptions {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_options_has_empty_schema_and_ignores_input() {
        assert!(<NoOptions as CopOptions>::SCHEMA.is_empty());
        assert!(<NoOptions as CopOptions>::from_config_json(b"not json").is_ok());
    }
}
