// Spike 0.3 PoC: runaway-cop deadline mechanism.
//
// Spike 0.2 found the default mruby3-sys build has NO `code_fetch_hook`
// (no MRB_USE_DEBUG_HOOK), and the crate's build.rs offers no supported way to
// enable it without forking. So the v1 mechanism is NOT a cooperative
// instruction hook — it is:
//
//   MECHANISM A: each cop runs on its own OS thread with its own isolated
//   mrb_state; the host waits with a wall-clock deadline; on timeout the
//   worker thread is ABANDONED (detached, never joined), a timeout `error
//   offense` is recorded for that cop, and the host continues. Process exit
//   reaps the leaked thread.
//
// Proves design §6 ("a runaway cop is bounded; host continues; that
// cop×file degrades to an error offense; everything else proceeds") WITHOUT
// the instruction hook the design assumed.
//
// The pathological cop is `while true; end` — ZERO yield points, no native
// callback per iteration. This is the real test: a flag-checking cooperative
// scheme would NOT catch this; only thread-abandon (or the unavailable hook)
// does.
//
// Throwaway spike code. NOT carried into crates/.

#![allow(unsafe_op_in_unsafe_fn)]

use ruby_prism::{parse, Visit};
use std::ffi::CString;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mruby3_sys::{mrb_close, mrb_load_string, mrb_open};

/// Read-only, shared across worker threads. Immutable prism AST data the cops
/// would traverse. Wrapped in Arc so an ABANDONED worker thread keeps it alive
/// — the thread must never dangle-deref freed AST (ADR 0002 drop rule). Arc is
/// cleaner than Box::leak: memory is freed once *all* threads (incl. abandoned
/// ones) are gone, which for a CLI is process exit.
struct AstContext {
    call_names: Vec<String>,
}

struct Collector {
    names: Vec<String>,
}
impl<'pr> Visit<'pr> for Collector {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.names
            .push(String::from_utf8_lossy(node.name().as_slice()).into_owned());
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Debug, PartialEq)]
enum CopOutcome {
    Completed,
    /// Cop exceeded the wall-clock deadline; thread abandoned.
    TimedOut,
}

/// Run one cop's `.rb` under a deadline on its own thread + isolated mrb_state.
/// Returns when the cop finishes OR the deadline elapses (whichever first).
fn run_cop_with_deadline(cop_src: &'static str, ast: Arc<AstContext>, deadline: Duration) -> CopOutcome {
    let (tx, rx) = mpsc::channel::<()>();
    // The worker captures an Arc clone — keeps AstContext alive even if we
    // abandon this thread. `ast` is read-only; immutable shared access is sound.
    let worker_ast = Arc::clone(&ast);
    thread::spawn(move || {
        // Touch the shared AST so the spike genuinely models a cop that holds
        // the context (mirrors the ud-based bridge from Spike 0.2).
        let _ = worker_ast.call_names.len();
        unsafe {
            let mrb = mrb_open();
            assert!(!mrb.is_null());
            let script = CString::new(cop_src).unwrap();
            // For the pathological cop this NEVER returns — the thread blocks
            // here forever inside mruby C. That is the point.
            mrb_load_string(mrb, script.as_ptr());
            mrb_close(mrb);
        }
        // Only reached by a well-behaved cop. If the receiver is gone
        // (host moved on), send fails harmlessly.
        let _ = tx.send(());
    });

    match rx.recv_timeout(deadline) {
        Ok(()) => CopOutcome::Completed,
        Err(mpsc::RecvTimeoutError::Timeout) => CopOutcome::TimedOut, // thread abandoned
        Err(mpsc::RecvTimeoutError::Disconnected) => CopOutcome::Completed,
    }
}

fn main() {
    // Real prism parse so the host genuinely owns an AST the cops share.
    let src = "puts \"hi\"\nlogger.info(x)\n";
    let result = parse(src.as_bytes());
    assert!(result.errors().next().is_none());
    let mut col = Collector { names: Vec::new() };
    col.visit(&result.node());
    let ast = Arc::new(AstContext {
        call_names: col.names.clone(),
    });
    println!("AST shared with cops: {:?}", ast.call_names);

    let deadline = Duration::from_millis(300);

    // Cop 1: pathological — infinite loop, ZERO yield points.
    let runaway = "while true; end";
    // Cop 2: well-behaved — returns immediately.
    let good = "x = 1 + 1";

    let host_start = Instant::now();

    let mut offenses: Vec<String> = Vec::new();

    let r1 = run_cop_with_deadline(runaway, Arc::clone(&ast), deadline);
    if r1 == CopOutcome::TimedOut {
        offenses.push("error: cop `runaway` exceeded 300ms deadline (abandoned)".into());
    }

    // The host MUST still run cop 2 after the runaway — proves "everything
    // else continues" (design §6).
    let r2 = run_cop_with_deadline(good, Arc::clone(&ast), deadline);

    let host_elapsed = host_start.elapsed();

    println!("runaway cop : {:?}", r1);
    println!("good cop    : {:?}", r2);
    println!("offenses    : {:?}", offenses);
    println!("host elapsed: {:?}", host_elapsed);

    // ---- Assertions (design §6) ----
    // (i) runaway is bounded.
    assert_eq!(r1, CopOutcome::TimedOut, "runaway must hit the deadline");
    // (ii) it degrades to exactly one error offense for that cop.
    assert_eq!(offenses.len(), 1, "timed-out cop -> one error offense");
    assert!(offenses[0].contains("error"));
    // (iii) the well-behaved cop after it still completes.
    assert_eq!(r2, CopOutcome::Completed, "good cop must complete");
    // (iv) the host returned promptly (deadline + slack), i.e. it did NOT
    //      block on the infinite loop.
    assert!(
        host_elapsed < deadline * 3,
        "host must not be held hostage by the runaway cop (got {host_elapsed:?})"
    );

    println!(
        "\nALL ASSERTIONS PASSED — runaway cop bounded by wall-clock deadline; \
         host survived and continued; one error offense; AST kept alive via Arc"
    );
}
