//! Build script that drives the LALRPOP parser generator.
//!
//! See beads issue murphy-qpf9 (Phase A+B parser migration). LALRPOP processes
//! every `*.lalrpop` file under `src/`, emitting the generated Rust into
//! `OUT_DIR` (typically `target/<profile>/build/murphy-pattern-*/out/`).
//! The generated module is included from `lib.rs` via `lalrpop_util::lalrpop_mod!`.
//!
//! Round 1 (this commit) wires up the toolchain only; the grammar in
//! `src/parser.lalrpop` is a minimal stub. Rounds 2 and 3 port the existing
//! hand-written parser in `src/parser.rs` over rule-by-rule.

fn main() {
    lalrpop::process_root().expect("LALRPOP grammar generation failed");
}
