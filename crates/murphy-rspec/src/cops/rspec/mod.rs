//! `RSpec/*` cops. Each cop registers itself via `submit_cop!`.
// automod::dir! は cop 移植完了後に撤退予定 — 撤退時は明示的な pub mod リストに戻す。
automod::dir!("src/cops/rspec");
