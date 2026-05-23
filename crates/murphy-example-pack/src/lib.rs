//! murphy-example-pack — demo cop pack.
//!
//! SUPERSEDED by murphy-9cr.23. The legacy lib body (3 cops on the
//! pre-reboot `MurphyPluginV1` ABI) was deleted in murphy-9cr.22; the
//! pack will be re-registered against `murphy-plugin-api` (ADR 0038)
//! in follow-up issues.
//!
//! The cdylib still builds with an empty cop table; the plugin loader
//! accepts an empty registration — see the loader's
//! `validate_registration_accepts_zero_cops` unit test
//! (`murphy-core::plugin_loader`).
