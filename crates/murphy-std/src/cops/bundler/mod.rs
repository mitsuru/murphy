//! `Bundler/*` cop namespace (ADR 0018) — Gemfile/gems.rb cops with a RuboCop
//! counterpart. Gated to bundler files by per-cop `Include` in `config/default.yml`.
// automod::dir! は cop 移植完了後に撤退予定 — 撤退時は明示的な pub mod リストに戻す。
automod::dir!("src/cops/bundler");
