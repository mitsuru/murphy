//! `Lint/*` cop namespace (ADR 0018) — correctness lints that have a RuboCop counterpart.
//! New cop files are picked up by `automod::dir!` below.
// automod::dir! は cop 移植完了後に撤退予定 — 撤退時は明示的な pub mod リストに戻す。
automod::dir!("src/cops/lint");
