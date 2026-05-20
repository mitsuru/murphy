//! Build-mode abstraction for mruby state bootstrap.
//!
//! This is a Phase 7 seam: default builds call plain `mrb_open` from
//! `mruby3-sys`, while custom builds can later provide an alternative open path
//! that installs an instruction-step hook before returning `mrb_state`.

use mruby3_sys::mrb_state;

/// Options for opening an mruby runtime in a single cop run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MrubyStateOptions {
    /// Optional instruction-step budget (for future custom mruby builds).
    pub instruction_budget: Option<u64>,
}

impl Default for MrubyStateOptions {
    fn default() -> Self {
        Self {
            instruction_budget: None,
        }
    }
}

/// Internal config passed from `mruby_core` to the build-specific open path.
pub(crate) fn open_state(options: MrubyStateOptions) -> *mut mrb_state {
    if cfg!(mruby_custom_build) {
        // Custom build not yet wired; this branch preserves current behavior while
        // retaining an in-VM budget hook seam for Phase 7.
        return open_state_default(options);
    }

    open_state_default(options)
}

/// Default runtime constructor (current production behavior).
fn open_state_default(_options: MrubyStateOptions) -> *mut mrb_state {
    let _ = _options.instruction_budget;
    // SAFETY: `mruby3_sys::mrb_open` is documented constructor for a
    // thread-confined mruby VM state and returns either null or a valid handle.
    unsafe { mruby3_sys::mrb_open() }
}
