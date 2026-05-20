//! Integration test for the `Murphy::Cop` mruby SDK base (Phase 4 Task 2).
//!
//! This exercises the public surface a downstream consumer sees:
//! `murphy_core::{parse_for_mruby (AstContext::new), run_mruby_cop, Offense}`.
//! It loads a `.rb` user cop written in the design §4 style, runs it over a
//! really-parsed source via the Task-3 live native primitives, and asserts the
//! emitted `Offense`s carry the ADR 0013 Phase-4 `autocorrect` field when a
//! `fix` block is present — `edits` contains the REAL `[start, end, replacement]`
//! values marshalled from Ruby into Rust (ADR 0013).
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

/// Same cop, but ALSO writing a `fix` block. Phase 4 (ADR 0013): the fix
/// MUST appear in the serialized offense as `autocorrect:{edits:[{range,replacement}]}`
/// with the REAL `[start, end, replacement]` values — no longer captured-only.
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

/// Phase-4 deliberate inversion of the Phase-3 soft-(a) invariant (ADR 0013).
///
/// Phase 3 asserted: fix-cop and no-fix-cop emit BYTE-IDENTICAL JSON (autocorrect absent).
/// Phase 4 (this test) asserts the OPPOSITE: a fix block produces `autocorrect:{edits:[...]}`
/// with the REAL `[start, end, replacement]` values from `fix.replace`; a no-fix cop still
/// has `autocorrect` ABSENT. This is the deliberate inversion point documented in ADR 0013.
#[test]
fn fix_block_produces_real_edit_values_in_offense_autocorrect() {
    // ADR 0013: deliberate inversion of Phase-3 soft-(a) "captured-only" invariant.
    // `puts "x"\n` — selector `puts` = bytes [0, 4) (ADR 0001).
    // fix.replace(node.message_loc, "logger.info") → start=0, end=4, replacement="logger.info"
    let src = "puts \"x\"\n";
    let no_fix = run(NO_PUTS_RB, "Murphy/NoPuts", "t.rb", src);
    let with_fix = run(NO_PUTS_FIX_RB, "Murphy/NoPuts", "t.rb", src);

    assert_eq!(no_fix.len(), 1);
    assert_eq!(with_fix.len(), 1);

    // No-fix cop: autocorrect ABSENT (unchanged invariant).
    let j_no_fix: serde_json::Value = serde_json::to_value(&no_fix[0]).unwrap();
    assert!(
        j_no_fix.as_object().unwrap().get("autocorrect").is_none(),
        "no-fix cop: autocorrect key must be ABSENT from JSON: {j_no_fix}"
    );

    // Fix cop: autocorrect PRESENT with REAL values (Phase 4 — ADR 0013 inversion).
    let j_with_fix: serde_json::Value = serde_json::to_value(&with_fix[0]).unwrap();
    let autocorrect = j_with_fix
        .as_object()
        .unwrap()
        .get("autocorrect")
        .expect("fix cop: autocorrect key MUST be present (ADR 0013 Phase 4)");
    let edits = autocorrect["edits"]
        .as_array()
        .expect("autocorrect.edits must be an array");
    assert_eq!(edits.len(), 1, "one fix.replace → one edit");
    // Exact real values from fix.replace(node.message_loc, "logger.info"):
    //   puts "x"\n: `puts` selector = bytes [0, 4).
    assert_eq!(
        edits[0]["range"]["start_offset"], 0,
        "edit start must be the real `puts` selector start offset"
    );
    assert_eq!(
        edits[0]["range"]["end_offset"], 4,
        "edit end must be the real `puts` selector end offset"
    );
    assert_eq!(
        edits[0]["replacement"].as_str().unwrap(),
        "logger.info",
        "edit replacement must be the real replacement text from fix.replace"
    );
}

#[test]
fn mruby_node_message_loc_is_typed_range() {
    const COP: &str = r#"
class Murphy
  def self.node_msg_range(_handle)
    raise "stringly node_msg_range must not be used"
  end
end

class TypedRangeCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?

    loc = node.message_loc
    return unless loc.is_a?(Murphy::Range)

    add_offense(loc, message: "typed range") do |fix|
      fix.replace(loc, "logger.info")
    end
  end
end
"#;

    let source = "puts \"x\"\n";
    let ctx: Arc<AstContext> = AstContext::new(source.as_bytes().to_vec());
    let offenses = run_mruby_cop(&ctx, COP, "Murphy/TypedRange", "t.rb");

    assert_eq!(offenses.len(), 1);
    assert_eq!(
        offenses[0].range,
        Range {
            start_offset: 0,
            end_offset: 4
        }
    );
    let edit = &offenses[0]
        .autocorrect
        .as_ref()
        .expect("fix block should reuse the typed message_loc range")
        .edits[0];
    assert_eq!(edit.range.start_offset, 0);
    assert_eq!(edit.range.end_offset, 4);
    assert_eq!(edit.replacement, "logger.info");
}

/// PIN B: an invalid edit (inverted range) is silently dropped; the offense still emits.
/// A cop that produces one valid edit AND one invalid edit → autocorrect present with 1 edit.
#[test]
fn invalid_range_edit_is_silently_dropped_valid_edit_survives() {
    // fix.replace at [0,4] is valid; fix.replace at [4,0] is inverted (start > end) → dropped.
    const COP: &str = r#"
class InvRangeCop < Murphy::Cop
  def on_call_node(n)
    return unless n.name == :puts && n.receiver_nil?
    add_offense(n.message_loc, message: "m") do |fix|
      fix.replace(n.message_loc, "ok")  # valid: [0, 4]
      # Inverted range (start > end) — must be silently dropped (PIN B).
      fix.replace(Murphy::Range.new(4, 0), "bad")
    end
  end
end
"#;
    let offenses = run(COP, "Murphy/InvRange", "t.rb", "puts \"x\"\n");
    assert_eq!(
        offenses.len(),
        1,
        "offense is still emitted despite dropped invalid edit"
    );
    let j: serde_json::Value = serde_json::to_value(&offenses[0]).unwrap();
    let edits = j["autocorrect"]["edits"]
        .as_array()
        .expect("autocorrect present: 1 valid edit survived");
    assert_eq!(
        edits.len(),
        1,
        "exactly the valid edit; inverted-range edit was silently dropped"
    );
    assert_eq!(edits[0]["range"]["start_offset"], 0);
    assert_eq!(edits[0]["range"]["end_offset"], 4);
    assert_eq!(edits[0]["replacement"].as_str().unwrap(), "ok");
}

/// PIN B: if ALL edits are invalid, autocorrect is ABSENT (not `edits:[]`).
#[test]
fn all_edits_invalid_autocorrect_absent() {
    // Both edits have inverted ranges → both dropped → Vec<Edit> empty → no autocorrect.
    const COP: &str = r#"
class AllBadCop < Murphy::Cop
  def on_call_node(n)
    return unless n.name == :puts && n.receiver_nil?
    add_offense(n.message_loc, message: "m") do |fix|
      fix.replace(Murphy::Range.new(4, 0), "bad1")
      fix.replace(Murphy::Range.new(9, 2), "bad2")
    end
  end
end
"#;
    let offenses = run(COP, "Murphy/AllBad", "t.rb", "puts \"x\"\n");
    assert_eq!(offenses.len(), 1, "offense is still emitted");
    let j: serde_json::Value = serde_json::to_value(&offenses[0]).unwrap();
    assert!(
        j.as_object().unwrap().get("autocorrect").is_none(),
        "all edits invalid → autocorrect ABSENT (not edits:[]): {j}"
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
