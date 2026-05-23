//! End-to-end test (placeholder) for native + `.rb` user cops co-occurring
//! under the CLI.
//!
//! v1 (post-murphy-9cr.22) does NOT load `.rb` user cops at the CLI
//! level — the legacy `load_mruby_cops` plumbing was removed when the
//! pre-reboot plugin ABI was retired (design §6.2). The C-backend
//! matcher in murphy-9cr.24 reintroduces `.rb` user cops through a
//! different surface, and this test will be re-authored against that
//! surface there.
//!
//! The file is preserved (compilable under
//! `--features mruby-user-cops`) so .24 has a regression target to fill
//! in, but no active assertion runs in .22. Asserting on the legacy
//! behavior would lock in a contract that no longer exists.

#![cfg(feature = "mruby-user-cops")]

#[test]
fn placeholder_for_24_c_backend_user_cop_e2e() {
    // Intentionally empty: a sentinel so the file's `cfg`-gating and
    // the test runner discover-pass remain wired. Once murphy-9cr.24
    // lands, this is replaced by the real e2e tests.
}
