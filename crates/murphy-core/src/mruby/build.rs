//! Build-mode abstraction for mruby state bootstrap.
//!
//! This is a Phase 7 seam: default builds call plain `mrb_open` from
//! `mruby3-sys`, while custom builds can later provide an alternative open path
//! that installs an instruction-step hook before returning `mrb_state`.

use mruby3_sys::mrb_state;

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
use libloading::{Library, Symbol};
#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
use std::sync::OnceLock;

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

/// Default runtime constructor (current production behavior).
fn open_state_default(_options: MrubyStateOptions) -> *mut mrb_state {
    let _ = _options.instruction_budget;
    // SAFETY: `mruby3_sys::mrb_open` is documented constructor for a
    // thread-confined mruby VM state and returns either null or a valid handle.
    unsafe { mruby3_sys::mrb_open() }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
type MrubyOpenStateFn = unsafe extern "C" fn(u64) -> *mut mrb_state;

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
type MrubyCloseStateFn = unsafe extern "C" fn(*mut mrb_state);

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
struct CustomRuntime {
    _library: &'static Library,
    open_state: MrubyOpenStateFn,
    close_state: Option<MrubyCloseStateFn>,
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
static CUSTOM_RUNTIME: OnceLock<Option<&'static CustomRuntime>> = OnceLock::new();

#[cfg(not(all(mruby_custom_build, not(target_os = "windows"))))]
pub(crate) fn open_state(options: MrubyStateOptions) -> *mut mrb_state {
    open_state_default(options)
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
pub(crate) fn open_state(options: MrubyStateOptions) -> *mut mrb_state {
    if let Some(runtime) = custom_runtime() {
        let state = unsafe {
            (runtime.open_state)(options.instruction_budget.unwrap_or(0))
        };
        if !state.is_null() {
            return state;
        }
    }

    open_state_default(options)
}

#[cfg(not(all(mruby_custom_build, not(target_os = "windows"))))]
pub(crate) fn close_state(state: *mut mrb_state) {
    // SAFETY: `state` is the owned handle returned by `open_state` and closed
    // exactly once here in normal-path Drop.
    unsafe { mruby3_sys::mrb_close(state) }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
pub(crate) fn close_state(state: *mut mrb_state) {
    if let Some(runtime) = custom_runtime() {
        if let Some(close_state) = runtime.close_state {
            // SAFETY: `state` is the owned handle this wrapper constructed via
            // `open_state` and this path owns it exactly once in normal Drop.
            unsafe { (close_state)(state); }
            return;
        }
    }

    // SAFETY: fall back to mruby default close if custom close hook is not
    // available.
    unsafe { mruby3_sys::mrb_close(state) }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
fn custom_runtime_path() -> Option<String> {
    parse_custom_runtime_path(std::env::var("MURPHY_MRUBY_CUSTOM_BUILD_PATH").ok()?)
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
fn parse_custom_runtime_path(raw: String) -> Option<String> {
    let path = raw.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_owned())
    }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
fn custom_runtime() -> Option<&'static CustomRuntime> {
    let path = custom_runtime_path()?;
    CUSTOM_RUNTIME
        .get_or_init(|| load_custom_runtime(&path).ok())
        .as_ref()
        .copied()
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
fn load_custom_runtime(path: &str) -> Result<&'static CustomRuntime, String> {
    // SAFETY: `Library::new` uses a filesystem path; safe Rust callers ensure
    // path validity by passing a valid UTF-8 string.
    let library = unsafe { Library::new(path) }
        .map_err(|err| format!("failed to load custom runtime {path}: {err}"))?;
    let library = Box::new(library);
    let library = Box::leak(library);

    // SAFETY: symbol lookup is unsafe because wrong types are UB at call-time;
    // we request explicit `extern "C"` signatures and validate symbol
    // resolution before using the pointers.
    let open_state: MrubyOpenStateFn = {
        let symbol: Symbol<'_, MrubyOpenStateFn> = unsafe {
            library
                .get(b"murphy_mruby_open_state\0")
                .map_err(|err| format!("missing `murphy_mruby_open_state`: {err}"))?
        };
        *symbol
    };

    // SAFETY: closing hook is optional; if absent, `close_state` falls back to
    // the default `mruby3_sys::mrb_close`.
    let close_state: Option<MrubyCloseStateFn> = unsafe {
        library
            .get::<MrubyCloseStateFn>(b"murphy_mruby_close_state\0")
            .ok()
            .map(|symbol| *symbol)
    };

    Ok(Box::leak(Box::new(CustomRuntime {
        _library: library,
        open_state,
        close_state,
    })))
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_custom_runtime_path_trims_whitespace() {
        assert_eq!(
            parse_custom_runtime_path("   /tmp/custom_runtime.so  ".to_string()),
            Some("/tmp/custom_runtime.so".to_string())
        );
        assert_eq!(parse_custom_runtime_path("   ".to_string()), None);
        assert_eq!(parse_custom_runtime_path("".to_string()), None);
    }

    #[test]
    fn load_custom_runtime_with_missing_library_fails() {
        assert!(load_custom_runtime("/tmp/missing_custom_runtime.so").is_err());
    }
}
