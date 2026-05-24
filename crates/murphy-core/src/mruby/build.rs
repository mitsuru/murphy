//! Build-mode abstraction for mruby state bootstrap.
//!
//! Default builds call `mrb_open` from `mruby3-sys`.
//! Custom builds may provide an alternate constructor in a shared runtime.

use mruby3_sys::mrb_state;

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
use libloading::{Library, Symbol};
#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
use std::collections::HashSet;
#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
use std::sync::{Mutex, OnceLock};

/// Options for opening an mruby runtime in a single cop run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MrubyStateOptions {
    /// Optional instruction-step budget for custom runtime callbacks.
    pub instruction_budget: Option<u64>,
}

/// Default runtime constructor (current production behavior).
fn open_state_default(_options: MrubyStateOptions) -> *mut mrb_state {
    let _ = _options.instruction_budget;
    // SAFETY: `mruby3_sys::mrb_open` is the documented constructor for mruby.
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
#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
static CUSTOM_OPENED_STATES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

#[cfg(not(all(mruby_custom_build, not(target_os = "windows"))))]
pub(crate) fn open_state(options: MrubyStateOptions) -> *mut mrb_state {
    open_state_default(options)
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
pub(crate) fn open_state(options: MrubyStateOptions) -> *mut mrb_state {
    if let Some(runtime) = custom_runtime() {
        let state = unsafe { (runtime.open_state)(options.instruction_budget.unwrap_or(0)) };
        if !state.is_null() {
            custom_opened_states()
                .lock()
                .expect("custom opened state tracker lock poisoned")
                .insert(state as usize);
            return state;
        }
    }

    open_state_default(options)
}

#[cfg(not(all(mruby_custom_build, not(target_os = "windows"))))]
pub(crate) fn close_state(state: *mut mrb_state) {
    // SAFETY: state is owned by this module and closed exactly once in Drop flow.
    unsafe { mruby3_sys::mrb_close(state) }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
pub(crate) fn close_state(state: *mut mrb_state) {
    if let Some(runtime) = custom_runtime() {
        let was_custom = custom_opened_states()
            .lock()
            .expect("custom opened state tracker lock poisoned")
            .remove(&(state as usize));

        if was_custom && let Some(close_state) = runtime.close_state {
            // SAFETY: state was opened by custom runtime and not yet closed.
            unsafe { (close_state)(state) }
            return;
        }
    }

    // SAFETY: if state was not custom opened or custom close is absent, fallback.
    unsafe { mruby3_sys::mrb_close(state) }
}

#[cfg(all(mruby_custom_build, not(target_os = "windows")))]
fn custom_opened_states() -> &'static Mutex<HashSet<usize>> {
    CUSTOM_OPENED_STATES.get_or_init(|| Mutex::new(HashSet::new()))
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
    let library = unsafe { Library::new(path) }
        .map_err(|err| format!("failed to load custom runtime {path}: {err}"))?;
    let library = Box::new(library);
    let library = Box::leak(library);

    // SAFETY: symbol lookup is unsafe; validate symbol signatures before use.
    let open_state: MrubyOpenStateFn = {
        let symbol: Symbol<'_, MrubyOpenStateFn> = unsafe {
            library
                .get(b"murphy_mruby_open_state\0")
                .map_err(|err| format!("missing `murphy_mruby_open_state`: {err}"))?
        };
        *symbol
    };

    // SAFETY: symbol lookup is optional; fallback to default close if absent.
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
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

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

    #[test]
    fn open_and_close_custom_runtime_with_fallback() {
        let runtime_src = r#"
            use std::sync::atomic::{AtomicU64, Ordering, AtomicUsize};

            #[repr(C)]
            pub struct mrb_state {
                _private: u8,
            }

            static OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);
            static CLOSE_CALLS: AtomicUsize = AtomicUsize::new(0);
            static LAST_BUDGET: AtomicU64 = AtomicU64::new(u64::MAX);

            #[no_mangle]
            pub extern "C" fn murphy_mruby_open_state(budget: u64) -> *mut mrb_state {
                OPEN_CALLS.fetch_add(1, Ordering::SeqCst);
                LAST_BUDGET.store(budget, Ordering::SeqCst);

                if budget == 0 {
                    return std::ptr::null_mut();
                }

                Box::into_raw(Box::new(mrb_state { _private: 0 }))
            }

            #[no_mangle]
            pub extern "C" fn murphy_mruby_close_state(state: *mut mrb_state) {
                if !state.is_null() {
                    unsafe { drop(Box::from_raw(state)); }
                }
                CLOSE_CALLS.fetch_add(1, Ordering::SeqCst);
            }

            #[no_mangle]
            pub extern "C" fn custom_open_calls() -> u64 {
                OPEN_CALLS.load(Ordering::SeqCst) as u64
            }

            #[no_mangle]
            pub extern "C" fn custom_close_calls() -> u64 {
                CLOSE_CALLS.load(Ordering::SeqCst) as u64
            }

            #[no_mangle]
            pub extern "C" fn custom_last_budget() -> u64 {
                LAST_BUDGET.load(Ordering::SeqCst)
            }
        "#;

        let tmp = TempDir::new().expect("temp dir");
        let src = tmp.path().join("runtime.rs");
        let lib = temp_runtime_lib_path(&tmp);

        std::fs::write(&src, runtime_src).expect("write runtime source");
        compile_custom_runtime(&src, &lib);

        let previous = std::env::var("MURPHY_MRUBY_CUSTOM_BUILD_PATH").ok();
        unsafe { std::env::set_var("MURPHY_MRUBY_CUSTOM_BUILD_PATH", &lib) };
        let symbols = load_custom_test_symbols(&lib);

        let state_custom = open_state(MrubyStateOptions {
            instruction_budget: Some(12),
        });
        assert!(!state_custom.is_null());
        close_state(state_custom);
        assert_eq!(symbols.last_budget(), 12);

        let state_default = open_state(MrubyStateOptions {
            instruction_budget: Some(0),
        });
        assert!(!state_default.is_null());
        close_state(state_default);
        assert_eq!(symbols.last_budget(), 0);

        assert_eq!(symbols.open_calls(), 2);
        assert_eq!(symbols.close_calls(), 1);

        if let Some(value) = previous {
            unsafe { std::env::set_var("MURPHY_MRUBY_CUSTOM_BUILD_PATH", value) };
        } else {
            unsafe { std::env::remove_var("MURPHY_MRUBY_CUSTOM_BUILD_PATH") };
        }
    }

    struct RuntimeTestSymbols {
        open_calls: unsafe extern "C" fn() -> u64,
        close_calls: unsafe extern "C" fn() -> u64,
        last_budget: unsafe extern "C" fn() -> u64,
    }

    impl RuntimeTestSymbols {
        fn open_calls(&self) -> u64 {
            unsafe { (self.open_calls)() }
        }

        fn close_calls(&self) -> u64 {
            unsafe { (self.close_calls)() }
        }

        fn last_budget(&self) -> u64 {
            unsafe { (self.last_budget)() }
        }
    }

    fn load_custom_test_symbols(path: &PathBuf) -> RuntimeTestSymbols {
        use libloading::Library;
        let lib = unsafe { Library::new(path) }.expect("load runtime lib");

        let open_calls: unsafe extern "C" fn() -> u64 = unsafe {
            *lib.get::<unsafe extern "C" fn() -> u64>(b"custom_open_calls\0")
                .expect("open calls")
        };
        let close_calls: unsafe extern "C" fn() -> u64 = unsafe {
            *lib.get::<unsafe extern "C" fn() -> u64>(b"custom_close_calls\0")
                .expect("close calls")
        };
        let last_budget: unsafe extern "C" fn() -> u64 = unsafe {
            *lib.get::<unsafe extern "C" fn() -> u64>(b"custom_last_budget\0")
                .expect("last budget")
        };

        RuntimeTestSymbols {
            open_calls,
            close_calls,
            last_budget,
        }
    }

    fn compile_custom_runtime(src: &PathBuf, lib: &PathBuf) {
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
        let output = Command::new(rustc)
            .arg("--crate-type")
            .arg("cdylib")
            .arg("-C")
            .arg("panic=abort")
            .arg(src)
            .arg("-o")
            .arg(lib)
            .output()
            .expect("compile custom runtime");

        if !output.status.success() {
            panic!(
                "failed to compile runtime: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    fn temp_runtime_lib_path(tmp: &TempDir) -> PathBuf {
        let mut lib_name = "libmurphy_custom_runtime".to_string();
        lib_name.push_str(lib_extension());
        tmp.path().join(lib_name)
    }

    fn lib_extension() -> &'static str {
        if cfg!(target_os = "macos") {
            ".dylib"
        } else {
            ".so"
        }
    }
}
