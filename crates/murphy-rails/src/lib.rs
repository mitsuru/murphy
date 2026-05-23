//! murphy-rails — Rails-focused cop pack.
//!
//! SUPERSEDED by murphy-9cr.23 (the legacy lib body was deleted in
//! murphy-9cr.22 when the pre-reboot plugin ABI — `MurphyPluginV1` /
//! `MurphyCallContext` / `MurphyEmitOffense` — was retired). The 138
//! cops will be re-registered against `murphy-plugin-api` (ADR 0038)
//! using `register_cops!` (from `murphy-plugin-macros`, murphy-9cr.21)
//! in murphy-9cr.23 and follow-ups.
//!
//! The cdylib still builds (Cargo requires a `lib.rs`); it simply
//! exports no cops. The murphy CLI's plugin loader accepts an empty
//! registration — see the loader's `validate_registration_accepts_zero_cops`
//! unit test (`murphy-core::plugin_loader`).
