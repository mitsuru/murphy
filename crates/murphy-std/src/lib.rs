//! Murphy standard cop pack — Murphy / Lint / Style / Layout (ADR 0018).
//!
//! This crate is the "built-in" pack: `murphy-cli` links it statically and
//! registers its cops through the same single-surface plugin ABI (ADR 0038)
//! that external `.so` packs use. The Murphy-internal dependency boundary
//! is **`murphy-plugin-api` only** — every standard cop reaches Murphy
//! through that one surface, with no shortcut through `murphy-core`. The
//! boundary is enforced as a compile-time test in
//! `tests/dep_boundary.rs` and is the implementation of §5 of
//! `docs/plans/2026-05-22-plugin-reboot-design.md`.
//!
//! v1 (murphy-9cr.23 §12a) ships an empty pack: the crate exists so the
//! boundary gate has something to assert and so subsequent §12b–§12d work
//! has a home for cops, the static registration entry point, and the
//! disabled registry. Each individual cop migration lands in its own
//! commit under §12d (and beyond, in epic murphy-au8).
