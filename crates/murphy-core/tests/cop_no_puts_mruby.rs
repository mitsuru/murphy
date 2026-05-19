//! Integration test for the `Murphy::Cop` mruby SDK base (Phase 3 Task 4).
//!
//! This exercises the public surface a downstream consumer sees:
//! `murphy_core::{parse_for_mruby (AstContext::new), run_mruby_cop, Offense}`.
//! It loads a `.rb` user cop written in the design §4 style, runs it over a
//! really-parsed source via the Task-3 live native primitives, and asserts the
//! emitted `Offense`s are the ADR 0006 frozen JSON shape — `autocorrect` is
//! ABSENT even when the cop writes a `fix` block (Scope Fence 1, soft-(a):
//! the fix is captured-stored-only, never applied, never serialized).
//!
//! ADR 0001: offense ranges are **byte** offsets; the positive cases pin the
//! exact `puts` selector range hand-computed from the source.

use std::sync::Arc;

use murphy_core::{AstContext, Offense, Range, Severity, run_mruby_cop};

/// The design §4 cop, verbatim-in-spirit. Task-3's `node_name` returns a
/// String and `receiver` is exposed as `receiver_nil?`, so the prelude's
/// `Node` coerces `name` to a Symbol and exposes `receiver_nil?` — the cop
/// reads close to design §4 (`node.name == :puts`, no explicit receiver),
/// `add_offense(node.message_loc, message:)`, with NO `fix` block.
const NO_PUTS_RB: &str = r#"
class NoPutsCop < Murphy::Cop
  MSG = "Use a logger instead of puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: MSG)
  end
end
"#;

/// Same cop, but ALSO writing a `fix` block. Soft-(a): the fix MUST be
/// captured-stored-only — the emitted offense must be BYTE-IDENTICAL (when
/// serialized) to the no-fix variant. `autocorrect` is never serialized.
const NO_PUTS_FIX_RB: &str = r#"
class NoPutsFixCop < Murphy::Cop
  MSG = "Use a logger instead of puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: MSG) do |fix|
      fix.replace(node.message_loc, "logger.info")
    end
  end
end
"#;

fn run(cop_src: &str, cop_name: &str, file: &str, source: &str) -> Vec<Offense> {
    let ctx: Arc<AstContext> = AstContext::new(source.as_bytes().to_vec());
    run_mruby_cop(&ctx, cop_src, cop_name, file)
}

#[test]
fn no_puts_cop_emits_one_offense_adr0006_shape_no_autocorrect() {
    // `puts "x"\n` — selector `puts` = bytes [0, 4) (ADR 0001).
    let offenses = run(NO_PUTS_RB, "Murphy/NoPuts", "t.rb", "puts \"x\"\n");
    assert_eq!(offenses.len(), 1, "exactly one offense for one bare `puts`");

    let o = &offenses[0];
    assert_eq!(o.file, "t.rb");
    assert_eq!(o.cop_name, "Murphy/NoPuts");
    assert_eq!(
        o.range,
        Range {
            start_offset: 0,
            end_offset: 4
        },
        "byte range of the `puts` selector token"
    );
    assert_eq!(o.severity, Severity::Warning, "default severity");
    assert_eq!(o.message, "Use a logger instead of puts");

    // ADR 0006 frozen JSON shape: `autocorrect` MUST be absent (soft-(a)).
    let j: serde_json::Value = serde_json::to_value(o).unwrap();
    assert!(
        j.get("autocorrect").is_none(),
        "ADR 0006: no `autocorrect` field in the serialized contract, got {j}"
    );
    // serde_json::Value orders keys via BTreeMap (alphabetical), so assert the
    // key SET (not declaration order — that is pinned by the existing
    // `offense::tests::offense_serializes_to_contract` contract test). The
    // load-bearing claim here is: exactly the ADR 0006 frozen 5 keys, and
    // crucially NO `autocorrect` (soft-(a)).
    let mut keys: Vec<&str> = j.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["cop_name", "file", "message", "range", "severity"],
        "Offense JSON is exactly the ADR 0006 frozen 5-field shape, no autocorrect"
    );
}

#[test]
fn fix_block_is_captured_only_offense_byte_identical_to_no_fix() {
    let src = "puts \"x\"\n";
    let no_fix = run(NO_PUTS_RB, "Murphy/NoPuts", "t.rb", src);
    let with_fix = run(NO_PUTS_FIX_RB, "Murphy/NoPuts", "t.rb", src);

    assert_eq!(no_fix.len(), 1);
    assert_eq!(with_fix.len(), 1);

    // Soft-(a): the `fix.replace(...)` is captured-stored-only. The emitted
    // offense — and its serialized JSON — is byte-identical to the no-fix
    // variant. The fix never reaches the contract.
    let j_no_fix = serde_json::to_string(&no_fix[0]).unwrap();
    let j_with_fix = serde_json::to_string(&with_fix[0]).unwrap();
    assert_eq!(
        j_no_fix, j_with_fix,
        "fix is captured-only: serialized offense must be byte-identical"
    );
    assert!(
        !j_with_fix.contains("autocorrect") && !j_with_fix.contains("logger.info"),
        "the captured fix MUST NOT leak into the serialized offense: {j_with_fix}"
    );
}

#[test]
fn explicit_receiver_puts_is_not_flagged() {
    // `obj.puts` HAS a receiver → the cop's `receiver_nil?` gate rejects it.
    let offenses = run(NO_PUTS_RB, "Murphy/NoPuts", "t.rb", "obj.puts\n");
    assert!(
        offenses.is_empty(),
        "obj.puts has an explicit receiver — 0 offenses, got {offenses:?}"
    );
}

#[test]
fn severity_kwarg_defaults_to_warning_and_is_overridable() {
    // A cop that passes `severity: :error` must produce an Error offense;
    // omitting it defaults to Warning (asserted above).
    const ERR_RB: &str = r#"
class ErrPutsCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: "boom", severity: :error)
  end
end
"#;
    let offenses = run(ERR_RB, "Murphy/ErrPuts", "t.rb", "puts 1\n");
    assert_eq!(offenses.len(), 1);
    assert_eq!(offenses[0].severity, Severity::Error);
}
