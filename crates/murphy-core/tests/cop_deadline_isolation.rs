//! Integration test for per-cop deadline + exception isolation
//! (Phase 3 Task 5; ADR 0003 Mechanism A; ADR 0009 composition rules).
//!
//! Exercises the public surface a downstream consumer sees:
//! `murphy_core::{AstContext, run_mruby_cop_isolated, Offense, Severity}`.
//!
//! Task 5 wraps the Task-4 synchronous `run_mruby_cop` in:
//!
//!   * a per-cop OS thread + wall-clock watchdog (`recv_timeout`),
//!   * abandon-on-timeout (the never-joined child thread is stuck inside
//!     `mrb_load_string`, so its stack-local `MrubyState`/`CopRun` `Drop`
//!     never runs → NO `mrb_close`; the child-thread-owned `Arc<AstContext>`
//!     clone keeps the AST alive — ADR 0003 Mechanism A / ADR 0009 rules 1 & 4),
//!   * one `error offense` for that cop×file on timeout, run continues,
//!   * Ruby-exception-caught (incl. an IN-VISITOR `raise`, I-3) → one
//!     `error offense`, run continues (design §6).
//!
//! The `error offense` is the ADR 0006 frozen `Offense` shape — `Severity::
//! Error`, the cop's own `cop_name`, a message naming the timeout/exception,
//! NO `autocorrect` field (the JSON contract is unchanged by Task 5).

use std::sync::Arc;
use std::time::{Duration, Instant};

use murphy_core::{
    AstContext, COP_DEADLINE, Offense, Severity, run_mruby_cop_isolated,
    run_mruby_cop_isolated_with_deadline,
};

/// A short hardcoded test deadline (the production deadline is longer; see
/// `sdk::COP_DEADLINE`). 200 ms is ample headroom for the well-behaved cops
/// here (they finish in well under a millisecond) while keeping the
/// runaway-cop timeout assertion fast.
const TEST_DEADLINE: Duration = Duration::from_millis(200);

/// Well-behaved cop: flag bare `puts` (mirrors the real NoReceiverPuts).
const GOOD_PUTS_RB: &str = r#"
class GoodPutsCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: "no bare puts")
  end
end
"#;

fn run(cop_src: &str, cop_name: &str, file: &str, source: &str) -> Vec<Offense> {
    let ctx: Arc<AstContext> = AstContext::new(source.as_bytes().to_vec());
    run_mruby_cop_isolated_with_deadline(&ctx, cop_src, cop_name, file, TEST_DEADLINE)
}

/// (a) A cop that `while true; end`s at cop-file load → exactly one Error
/// offense for it; the call returns within ~deadline+ε (NOT infinite); and a
/// sibling well-behaved cop on the SAME source still produces its offense.
#[test]
fn runaway_cop_times_out_to_one_error_offense_bounded_wall_time() {
    // `cops/loops.rb`-style: zero yield points, no per-iteration native
    // callback — only thread-abandon (Mechanism A) bounds this.
    const LOOPS_RB: &str = "while true; end";

    let start = Instant::now();
    let offenses = run(LOOPS_RB, "Murphy/Loops", "loops.rb", "puts \"hi\"\n");
    let elapsed = start.elapsed();

    // BOUNDED WALL TIME: the host was NOT held hostage by the infinite loop.
    // Generous bound (deadline × 8) covers cold mruby init + thread spawn
    // without ever approaching "infinite" — the discriminating assertion vs a
    // cooperative scheme that would hang forever here.
    assert!(
        elapsed < TEST_DEADLINE * 8,
        "host must not hang on the runaway cop (elapsed {elapsed:?}, \
         deadline {TEST_DEADLINE:?}) — it bounded, did not hang"
    );

    // Exactly one Error offense for that cop×file.
    assert_eq!(
        offenses.len(),
        1,
        "runaway cop → exactly one error offense (got {offenses:?})"
    );
    let o = &offenses[0];
    assert_eq!(o.cop_name, "Murphy/Loops");
    assert_eq!(o.file, "loops.rb");
    assert_eq!(o.severity, Severity::Error);
    assert!(
        o.message.to_lowercase().contains("deadline"),
        "the timeout error offense must name the deadline: {}",
        o.message
    );

    // ADR 0006 frozen JSON shape: NO autocorrect even for the error offense.
    let j = serde_json::to_string(o).unwrap();
    assert!(
        !j.contains("autocorrect"),
        "ADR 0006: no autocorrect in the error-offense JSON: {j}"
    );
}

/// One runaway cop must NOT poison a sibling well-behaved cop on the same
/// source: each cop gets its own isolated thread+state+deadline.
#[test]
fn runaway_cop_does_not_poison_a_sibling_good_cop_on_same_source() {
    let src = "puts \"hi\"\n";
    let ctx: Arc<AstContext> = AstContext::new(src.as_bytes().to_vec());

    let runaway = run_mruby_cop_isolated_with_deadline(
        &ctx,
        "while true; end",
        "Murphy/Loops",
        "f.rb",
        TEST_DEADLINE,
    );
    let good = run_mruby_cop_isolated_with_deadline(
        &ctx,
        GOOD_PUTS_RB,
        "Murphy/Good",
        "f.rb",
        TEST_DEADLINE,
    );

    assert_eq!(runaway.len(), 1);
    assert_eq!(runaway[0].severity, Severity::Error);

    assert_eq!(
        good.len(),
        1,
        "the good cop on the same source must still produce its offense \
         (one runaway cop does not poison its sibling) — got {good:?}"
    );
    assert_eq!(good[0].cop_name, "Murphy/Good");
    assert_eq!(good[0].severity, Severity::Warning);
}

/// (b) I-3: a cop whose `on_call_node` RAISES (the IN-VISITOR case, not only a
/// top-level `raise` at file load) → exactly one Error offense for it; a
/// sibling unaffected.
#[test]
fn in_visitor_raise_is_isolated_to_one_error_offense() {
    // Real native work first (the walk reaches `on_call_node`), THEN raise
    // from inside the visitor — Task-4's eval did not check `(*mrb).exc`, so
    // this would otherwise be a silent no-op (I-3).
    const BOOM_IN_VISITOR_RB: &str = r#"
class BoomInVisitorCop < Murphy::Cop
  def on_call_node(node)
    raise "cop blew up inside on_call_node"
  end
end
"#;
    let src = "puts \"hi\"\nFoo.bar(1)\n";
    let ctx: Arc<AstContext> = AstContext::new(src.as_bytes().to_vec());

    let boom = run_mruby_cop_isolated_with_deadline(
        &ctx,
        BOOM_IN_VISITOR_RB,
        "Murphy/Boom",
        "boom.rb",
        TEST_DEADLINE,
    );
    assert_eq!(
        boom.len(),
        1,
        "in-visitor raise → exactly one error offense (got {boom:?})"
    );
    assert_eq!(boom[0].cop_name, "Murphy/Boom");
    assert_eq!(boom[0].file, "boom.rb");
    assert_eq!(boom[0].severity, Severity::Error);
    assert!(
        boom[0].message.to_lowercase().contains("exception")
            || boom[0].message.to_lowercase().contains("raise"),
        "the exception error offense must name the exception: {}",
        boom[0].message
    );

    // Sibling unaffected (exception isolated).
    let good = run_mruby_cop_isolated_with_deadline(
        &ctx,
        GOOD_PUTS_RB,
        "Murphy/Good",
        "boom.rb",
        TEST_DEADLINE,
    );
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].severity, Severity::Warning);
}

/// A top-level `raise` at cop-file load is ALSO isolated (the original
/// `cops/boom.rb` case from the plan).
#[test]
fn top_level_raise_at_load_is_isolated_to_one_error_offense() {
    const BOOM_RB: &str = r#"raise "cop file blew up at load""#;
    let offenses = run(BOOM_RB, "Murphy/Boom", "boom.rb", "puts 1\n");
    assert_eq!(offenses.len(), 1, "got {offenses:?}");
    assert_eq!(offenses[0].severity, Severity::Error);
    assert_eq!(offenses[0].cop_name, "Murphy/Boom");
}

/// A well-behaved cop with ample deadline headroom completes normally,
/// producing its real offenses (NOT an error offense). This is the
/// determinism-with-headroom case (ADR 0009 rule 6): a cop that finishes
/// well inside the deadline always resolves the same way.
#[test]
fn well_behaved_cop_with_headroom_completes_normally() {
    let offenses = run(
        GOOD_PUTS_RB,
        "Murphy/Good",
        "g.rb",
        "puts \"a\"\nputs(b)\nx.puts\n",
    );
    // `puts "a"` and `puts(b)` are bare puts → 2 offenses; `x.puts` has a
    // receiver → not flagged. NONE are Error offenses.
    assert_eq!(offenses.len(), 2, "got {offenses:?}");
    assert!(
        offenses.iter().all(|o| o.severity == Severity::Warning),
        "a well-behaved cop with headroom produces only its real offenses, \
         never an error offense: {offenses:?}"
    );
}

/// M-3: a single `.rb` defining TWO cops → BOTH run, BOTH offenses present
/// (the multi-cop-per-file contract guard Task 7 relies on; Task-4's
/// docstring claims it but no test exercised it).
#[test]
fn multi_cop_per_file_both_cops_run() {
    const TWO_COPS_RB: &str = r#"
class FirstCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: "first cop says no puts")
  end
end

class SecondCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: "second cop also says no puts")
  end
end
"#;
    let offenses = run(TWO_COPS_RB, "Murphy/Two", "two.rb", "puts \"hi\"\n");
    assert_eq!(
        offenses.len(),
        2,
        "one .rb defining two cops → both run, both offenses present \
         (got {offenses:?})"
    );
    let msgs: Vec<&str> = offenses.iter().map(|o| o.message.as_str()).collect();
    assert!(
        msgs.contains(&"first cop says no puts"),
        "FirstCop must have run: {msgs:?}"
    );
    assert!(
        msgs.contains(&"second cop also says no puts"),
        "SecondCop must have run: {msgs:?}"
    );
    assert!(
        offenses.iter().all(|o| o.severity == Severity::Warning),
        "both cops completed normally, no error offense: {offenses:?}"
    );
}

/// The public, hardcoded-deadline entry point [`run_mruby_cop_isolated`]
/// (the API Task 7 wires) works end-to-end with the sane [`COP_DEADLINE`]:
/// a well-behaved cop completes far inside it (no error offense), and a
/// runaway cop still terminates well within `COP_DEADLINE` (bounded — NOT
/// the full `COP_DEADLINE` wall, since mruby blocks immediately, but
/// definitively not infinite).
#[test]
fn public_hardcoded_deadline_api_works_end_to_end() {
    assert!(
        COP_DEADLINE >= Duration::from_millis(500),
        "the hardcoded deadline must be a sane, generous value (ADR 0003), \
         got {COP_DEADLINE:?}"
    );

    let ctx: Arc<AstContext> = AstContext::new(b"puts \"hi\"\n".to_vec());

    // Well-behaved: completes normally under the hardcoded deadline.
    let good = run_mruby_cop_isolated(&ctx, GOOD_PUTS_RB, "Murphy/Good", "p.rb");
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].severity, Severity::Warning);

    // Runaway: one error offense, bounded by the hardcoded deadline (must
    // not hang; allow COP_DEADLINE + generous slack for spawn/init).
    let start = Instant::now();
    let runaway = run_mruby_cop_isolated(&ctx, "while true; end", "Murphy/Loops", "p.rb");
    let elapsed = start.elapsed();
    assert_eq!(runaway.len(), 1);
    assert_eq!(runaway[0].severity, Severity::Error);
    assert!(
        elapsed < COP_DEADLINE + Duration::from_secs(2),
        "runaway under the hardcoded deadline must still be bounded \
         (elapsed {elapsed:?}, deadline {COP_DEADLINE:?})"
    );
}
