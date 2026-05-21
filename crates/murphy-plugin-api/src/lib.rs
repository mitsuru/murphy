//! Safe, plugin-author-facing surface over the Murphy native plugin ABI.
//!
//! This crate is what third-party native plugins import. It re-exports the
//! raw `#[repr(C)]` ABI types from `murphy-core` together with two safe
//! traits — [`Cop`] and [`CopOptions`] — that authors implement on plain
//! Rust structs, and a [`kinds`] module of node-kind string constants.
//!
//! The proc-macro driven impls (`register_cops!`, `#[derive(CopOptions)]`,
//! `#[murphy::cop]`) belong to a separate `murphy-plugin-macros` crate
//! (murphy-9cr.6 / .7) and consume the traits and constants defined here.

pub use murphy_core::{
    CopOptionMetadata, MURPHY_CALL_ARGUMENT_KIND_OTHER, MURPHY_CALL_ARGUMENT_KIND_STRING,
    MURPHY_CALL_ARGUMENT_KIND_SYMBOL, MURPHY_CALL_RECEIVER_FLOAT, MURPHY_CALL_RECEIVER_INTEGER,
    MURPHY_CALL_RECEIVER_NONE, MURPHY_CALL_RECEIVER_OTHER, MURPHY_PLUGIN_ABI_VERSION,
    MURPHY_SEVERITY_ERROR, MURPHY_SEVERITY_UNSET, MURPHY_SEVERITY_WARNING, MURPHY_TRISTATE_FALSE,
    MURPHY_TRISTATE_TRUE, MURPHY_TRISTATE_UNSET, MurphyCallContext, MurphyCallDispatchV1,
    MurphyCopOptionV1, MurphyEmitOffense, MurphyFileContext, MurphyNodeContext,
    MurphyNodeDispatchV1, MurphyPluginAutocorrect, MurphyPluginCallArgument, MurphyPluginCopV1,
    MurphyPluginEdit, MurphyPluginOffense, MurphyPluginV1, MurphyRange, MurphyRunCallDispatch,
    MurphyRunFile, MurphyRunNodeDispatch, MurphySlice, Severity,
};

pub mod kinds;

/// A cop, as authored against the plugin API.
///
/// The `register_cops!` macro (murphy-9cr.6) turns implementations of this
/// trait into the static `MurphyPluginCopV1` table that Murphy's loader
/// consumes. Authors usually do **not** implement [`Cop`] by hand — they
/// derive it through `#[murphy::cop]` (murphy-9cr.8) — but the trait is
/// part of the stable surface and may be implemented directly when the
/// macros are not enough.
///
/// `name` is required; everything else has a sensible default.
///
/// # Example
///
/// ```
/// use murphy_plugin_api::{Cop, NoOptions, Severity};
///
/// struct NoTabs;
///
/// impl Cop for NoTabs {
///     type Options = NoOptions;
///     fn name(&self) -> &'static str { "Plugin/NoTabs" }
///     fn description(&self) -> &'static str { "Forbids tab indentation." }
///     fn default_severity(&self) -> Option<Severity> { Some(Severity::Warning) }
/// }
/// ```
pub trait Cop: Send + Sync + 'static {
    /// Option struct backing this cop's `[cops.rules."Name"]` table.
    ///
    /// Defaults to [`NoOptions`] for cops that take no configuration beyond
    /// `enabled` and `severity`.
    type Options: CopOptions;

    /// The cop identifier, e.g. `"Plugin/MyCop"`. Must match the runtime
    /// name visible in `murphy.toml` and in offense JSON.
    fn name(&self) -> &'static str;

    /// One-line human-readable description. Surfaced by future `murphy
    /// plugins list` diagnostics and editor hover. Empty by default.
    fn description(&self) -> &'static str {
        ""
    }

    /// Default severity used when the user does not override it in
    /// `murphy.toml`. `None` means "let Murphy decide" (typically warning).
    fn default_severity(&self) -> Option<Severity> {
        None
    }

    /// Default enablement. `None` keeps Murphy's built-in default (enabled
    /// for `Style`/`Lint`/`Murphy`, opt-in for niche cops).
    fn default_enabled(&self) -> Option<bool> {
        None
    }
}

/// Plugin-side counterpart of [`CopOptionMetadata`].
///
/// Plugin authors usually derive this trait via `#[derive(CopOptions)]`
/// (murphy-9cr.7). Direct implementation is supported but requires
/// hand-maintaining the schema slice and the JSON decoder.
///
/// `Default` is required so the runtime can hand the cop an `Options`
/// instance even when the user supplied no `[cops.rules."X"]` table.
pub trait CopOptions: Default + Sized + 'static {
    /// Static schema describing each option. The validation gate
    /// (murphy-9cr.9) reads this to diff against the user's config and to
    /// report unknown / deprecated keys.
    fn schema() -> &'static [MurphyCopOptionV1];
}

/// Marker type for cops that declare no options.
///
/// Implements [`CopOptions`] with an empty schema, so writing
/// `type Options = NoOptions;` lets a cop opt out of configuration entirely.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOptions;

impl CopOptions for NoOptions {
    fn schema() -> &'static [MurphyCopOptionV1] {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_options_has_empty_schema() {
        assert!(<NoOptions as CopOptions>::schema().is_empty());
    }

    #[test]
    fn kinds_count_matches_all_slice_length() {
        assert_eq!(kinds::ALL.len(), kinds::COUNT);
    }

    #[test]
    fn kinds_all_has_no_duplicates() {
        // ALL is sorted by Rust enum variant name (which is how the source
        // module is organised); the invariant that actually matters at
        // runtime is that wire names do not repeat.
        let mut seen = std::collections::BTreeSet::new();
        for kind in kinds::ALL {
            assert!(seen.insert(*kind), "duplicate node kind: {kind}");
        }
    }

    #[test]
    fn kinds_consts_resolve_to_known_examples() {
        // Spot-check that the SCREAMING_SNAKE_CASE identifier mapping holds.
        assert_eq!(kinds::CALL_NODE, "call");
        assert_eq!(kinds::ALIAS_GLOBAL_VARIABLE_NODE, "alias_global_variable");
        assert_eq!(kinds::IF_NODE, "if");
        assert_eq!(kinds::UNLESS_NODE, "unless");
        assert_eq!(kinds::CLASS_NODE, "class");
    }
}
