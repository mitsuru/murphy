//! Implementation of the `#[cop]` and `#[on_node]` attribute proc-macros
//! (murphy-9cr.8).
//!
//! This module contains the `proc_macro2::TokenStream`-level logic; the
//! `proc_macro::TokenStream` shims in `lib.rs` call into these functions.
//!
//! ## Layer 1 (murphy-9cr.8.1) — skeleton
//!
//! - `cop`: identity — passes the `item` through unchanged.  The full
//!   lowering (trait impls, `KINDS`, dispatch table) is added in layer 2.
//! - `on_node`: stub that always emits
//!   `compile_error!("#[on_node] must be used inside a #[cop] impl block")`.
//!   When `#[cop]` is correctly applied, it consumes `#[on_node]` attributes
//!   before they ever reach this proc-macro, so the error is only triggered
//!   on misuse.

use proc_macro2::TokenStream;

/// `#[cop(...)]` — layer-1 identity skeleton.
///
/// Accepts any `item` (args are silently ignored for now) and returns it
/// unchanged.  Subsequent layers will parse `args`, validate `item`, and
/// generate `Cop` / `NodeCop` impls.
pub fn cop(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// `#[on_node(...)]` — stub that fires only on misuse.
///
/// When `#[cop]` processes an impl block it will consume all `#[on_node]`
/// attributes directly, so this proc-macro entry point is only reached when
/// `#[on_node]` appears outside a `#[cop]` impl.  We always emit a
/// `compile_error!` to surface that misuse clearly.
pub fn on_node(_args: TokenStream, _item: TokenStream) -> TokenStream {
    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[on_node] must be used inside a #[cop] impl block",
    )
    .to_compile_error()
}
