//! The `Cop` trait — a cop's compile-time metadata.

use crate::RubyVersion;
use crate::options::CopOptions;
use crate::severity::Severity;

/// A cop, as authored against the plugin API: **metadata only**.
///
/// Every field is an associated `const` so `register_cops!`
/// (murphy-9cr.21) can assemble the static registration table at
/// const-eval time. Runtime dispatch lives on the `NodeCop` trait.
/// This continues the const-based, stateless-cop design of ADR 0035.
pub trait Cop: Send + Sync + 'static {
    /// Option struct backing this cop's config table. [`NoOptions`] for
    /// cops with no configuration beyond `enabled` / `severity`.
    ///
    /// [`NoOptions`]: crate::NoOptions
    type Options: CopOptions;

    /// The cop identifier, e.g. `"Plugin/MyCop"`. Must match the name in
    /// `murphy.toml` and offense JSON.
    const NAME: &'static str;

    /// One-line human-readable description. Empty by default.
    const DESCRIPTION: &'static str = "";

    /// Default severity when the user does not override it. `None` leaves
    /// Murphy's built-in fallback.
    const DEFAULT_SEVERITY: Option<Severity> = None;

    /// Default enablement. `None` keeps Murphy's built-in default.
    const DEFAULT_ENABLED: Option<bool> = None;

    /// Whether normal lint execution should treat this cop as safe.
    /// `None` keeps Murphy's default, currently safe.
    const SAFE: Option<bool> = None;

    /// Whether `murphy lint -a` may apply this cop's autocorrections.
    /// `None` keeps Murphy's default, currently safe.
    const SAFE_AUTOCORRECT: Option<bool> = None;

    /// Minimum `AllCops.TargetRubyVersion` required for this cop to run.
    /// `None` means no version gating.
    const MINIMUM_TARGET_RUBY_VERSION: Option<RubyVersion> = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cop_metadata_consts_are_readable() {
        struct Stub;
        impl Cop for Stub {
            type Options = crate::NoOptions;
            const NAME: &'static str = "Plugin/Stub";
            const DEFAULT_SEVERITY: Option<crate::Severity> = Some(crate::Severity::Warning);
        }
        assert_eq!(<Stub as Cop>::NAME, "Plugin/Stub");
        assert_eq!(<Stub as Cop>::DESCRIPTION, ""); // default
        assert_eq!(
            <Stub as Cop>::DEFAULT_SEVERITY,
            Some(crate::Severity::Warning)
        );
        assert_eq!(<Stub as Cop>::DEFAULT_ENABLED, None); // default
        assert_eq!(<Stub as Cop>::SAFE, None); // default
        assert_eq!(<Stub as Cop>::SAFE_AUTOCORRECT, None); // default
        assert_eq!(<Stub as Cop>::MINIMUM_TARGET_RUBY_VERSION, None); // default
    }
}
