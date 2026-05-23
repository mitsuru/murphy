//! Single-symbol ABI loader for the post-reboot plugin surface (ADR 0038).
//!
//! A plugin `.so` exports exactly one symbol — [`MurphyPluginRegister`] —
//! that the host calls to fill a [`PluginRegistration`]. The loader
//! validates the registration (ABI version + per-cop struct size), then
//! keeps the `dlopen`'d library alive for the duration the cop pointers
//! are referenced.
//!
//! Strict scope (murphy-9cr.22 acceptance criterion):
//! - `dlopen` the `.so`, resolve `murphy_plugin_register`.
//! - Invoke it, capture a [`PluginRegistration`].
//! - Reject `abi_version != MURPHY_PLUGIN_ABI_VERSION`.
//! - Reject any [`PluginCopV1`] whose `size != size_of::<PluginCopV1>()`.
//! - Reject a `cops_len > 0` with a null `cops_ptr`.
//!
//! Optional validations (cop name uniqueness, schema sanity, etc.) live
//! in murphy-9cr.9 (option schema validation gate); they are deliberately
//! not folded in here.

use murphy_plugin_api::{MURPHY_PLUGIN_ABI_VERSION, MurphyPluginRegister, PluginCopV1};

/// The exported symbol name a plugin `.so` must define.
pub const REGISTER_SYMBOL: &[u8] = b"murphy_plugin_register";

/// A loader-level failure. Distinct from the post-load runtime errors a
/// cop might raise — those flow through the dispatch host (the i32
/// return).
#[derive(Debug)]
pub enum LoaderError {
    /// The shared library could not be opened (path missing, ELF malformed,
    /// permissions, etc.). The wrapped message is the dynamic linker's.
    Open(String),
    /// The single required symbol is not exported.
    MissingSymbol(String),
    /// The registration function returned a non-zero status (a trapped
    /// panic in `register_cops!` or an explicit failure).
    RegisterFailed(i32),
    /// The plugin advertised a different ABI version than this host.
    AbiVersionMismatch {
        /// What the host expects.
        expected: u32,
        /// What the plugin reported.
        got: u32,
    },
    /// A `PluginCopV1.size` did not match `size_of::<PluginCopV1>()`. The
    /// pack and host were compiled against divergent struct layouts and
    /// cannot interoperate safely.
    StructSizeMismatch {
        /// Position of the offending cop in the registration table.
        cop_index: usize,
        /// What the host's `PluginCopV1` measures.
        expected: usize,
        /// What the plugin wrote into the `size` field.
        got: usize,
    },
    /// `cops_len > 0` but `cops_ptr` is null — the table is unreachable.
    NullCopsPointer { cops_len: usize },
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderError::Open(msg) => write!(f, "failed to open plugin: {msg}"),
            LoaderError::MissingSymbol(name) => {
                write!(f, "plugin is missing the required symbol `{name}`")
            }
            LoaderError::RegisterFailed(rc) => {
                write!(f, "plugin registration returned non-zero status {rc}")
            }
            LoaderError::AbiVersionMismatch { expected, got } => {
                write!(
                    f,
                    "plugin ABI version mismatch: got {got}, host expects {expected}"
                )
            }
            LoaderError::StructSizeMismatch {
                cop_index,
                expected,
                got,
            } => {
                write!(
                    f,
                    "plugin cop {cop_index} reports PluginCopV1.size = {got}, \
                     host's size_of::<PluginCopV1>() = {expected}"
                )
            }
            LoaderError::NullCopsPointer { cops_len } => {
                write!(
                    f,
                    "plugin registration has cops_len = {cops_len} but a null cops_ptr"
                )
            }
        }
    }
}

impl std::error::Error for LoaderError {}

/// Validate a [`PluginRegistration`] in isolation (no `dlopen`) and return
/// a slice view over the cop table on success. Factored out so unit tests
/// can drive every rejection branch without a real `.so`.
///
/// # Safety
/// `cops_ptr` must point to `cops_len` consecutive `PluginCopV1` values
/// for `'static`, or `cops_len` must be 0. In real use the registration
/// is filled by `register_cops!` against static cop tables, so this is a
/// natural fit; tests construct fakes that satisfy it.
pub unsafe fn validate_registration(
    reg: &murphy_plugin_api::PluginRegistration,
) -> Result<&'static [PluginCopV1], LoaderError> {
    if reg.abi_version != MURPHY_PLUGIN_ABI_VERSION {
        return Err(LoaderError::AbiVersionMismatch {
            expected: MURPHY_PLUGIN_ABI_VERSION,
            got: reg.abi_version,
        });
    }
    if reg.cops_len > 0 && reg.cops_ptr.is_null() {
        return Err(LoaderError::NullCopsPointer {
            cops_len: reg.cops_len,
        });
    }
    let cops: &'static [PluginCopV1] = if reg.cops_len == 0 {
        &[]
    } else {
        // Safety: contract above.
        unsafe { std::slice::from_raw_parts(reg.cops_ptr, reg.cops_len) }
    };
    let expected_size = std::mem::size_of::<PluginCopV1>();
    for (cop_index, cop) in cops.iter().enumerate() {
        if cop.size != expected_size {
            return Err(LoaderError::StructSizeMismatch {
                cop_index,
                expected: expected_size,
                got: cop.size,
            });
        }
    }
    Ok(cops)
}

/// A loaded plugin pack, holding the live `Library` handle and a borrowed
/// view of the cop table the registration declared.
///
/// The `_library` field owns the `dlopen` handle: dropping the pack
/// `dlclose`s the library, which invalidates the cop pointers. Callers
/// must hold a `LoadedPluginPack` for as long as any borrowed
/// `&PluginCopV1` from it is in use.
#[cfg(not(target_os = "windows"))]
pub struct LoadedPluginPack {
    /// Original path, kept for diagnostics.
    pub path: std::path::PathBuf,
    /// Borrowed cop table — pointers are stable for the library's lifetime.
    pub cops: &'static [PluginCopV1],
    _library: libloading::Library,
}

/// Load a plugin pack from `path`. See module docs for the validation set
/// performed.
#[cfg(not(target_os = "windows"))]
pub fn load_plugin_pack(path: &std::path::Path) -> Result<LoadedPluginPack, LoaderError> {
    use libloading::{Library, Symbol};

    // Safety: dlopen of an attacker-controlled path is intentional (ADR 0004
    // accepts user-cop trust); the caller has gated this on the project's
    // configured cops path.
    let library = unsafe { Library::new(path) }.map_err(|e| LoaderError::Open(e.to_string()))?;

    let mut reg = murphy_plugin_api::PluginRegistration {
        abi_version: 0,
        cops_ptr: std::ptr::null(),
        cops_len: 0,
    };

    let rc = {
        let symbol: Symbol<'_, MurphyPluginRegister> = unsafe {
            library.get(REGISTER_SYMBOL).map_err(|_| {
                LoaderError::MissingSymbol(String::from_utf8_lossy(REGISTER_SYMBOL).into_owned())
            })?
        };
        // Safety: the symbol is `MurphyPluginRegister`-typed; the plugin
        // contract (ADR 0038) forbids the thunk from retaining the
        // pointer past return or unwinding.
        unsafe { symbol(&mut reg) }
    };
    if rc != 0 {
        return Err(LoaderError::RegisterFailed(rc));
    }

    // Safety: see `validate_registration`'s contract — the registration's
    // `cops_ptr` was filled by `register_cops!` (which uses a `&'static`
    // cop table) and lives as long as `library`.
    let cops = unsafe { validate_registration(&reg)? };
    Ok(LoadedPluginPack {
        path: path.to_path_buf(),
        cops,
        _library: library,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::{
        CxRaw, NodeKindTag, PluginCopV1, PluginRegistration, RawSlice, SEVERITY_UNSET,
    };

    /// A no-op dispatch thunk to fill `PluginCopV1.dispatch` in fake cops.
    /// Tests in this module never invoke it; the loader validates static
    /// shape only.
    unsafe extern "C" fn noop_dispatch(_node: murphy_ast::NodeId, _cx: *const CxRaw) -> i32 {
        0
    }

    static FAKE_KINDS: &[NodeKindTag] = &[NodeKindTag(1)];

    fn fake_cop(size_override: Option<usize>) -> PluginCopV1 {
        PluginCopV1 {
            size: size_override.unwrap_or(std::mem::size_of::<PluginCopV1>()),
            name: RawSlice::from_str("Fake/Cop"),
            description: RawSlice::from_str(""),
            default_severity: SEVERITY_UNSET,
            default_enabled: 255,
            options_ptr: std::ptr::null(),
            options_len: 0,
            kinds_ptr: FAKE_KINDS.as_ptr(),
            kinds_len: FAKE_KINDS.len(),
            dispatch: noop_dispatch,
        }
    }

    #[test]
    fn validate_registration_accepts_correct_registration() {
        let cops = [fake_cop(None)];
        let reg = PluginRegistration {
            abi_version: MURPHY_PLUGIN_ABI_VERSION,
            cops_ptr: cops.as_ptr(),
            cops_len: cops.len(),
        };
        let got = unsafe { validate_registration(&reg) }.expect("should validate");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].size, std::mem::size_of::<PluginCopV1>());
    }

    #[test]
    fn validate_registration_rejects_wrong_abi_version() {
        let reg = PluginRegistration {
            abi_version: 99,
            cops_ptr: std::ptr::null(),
            cops_len: 0,
        };
        let err = unsafe { validate_registration(&reg) }.unwrap_err();
        match err {
            LoaderError::AbiVersionMismatch { expected, got } => {
                assert_eq!(expected, MURPHY_PLUGIN_ABI_VERSION);
                assert_eq!(got, 99);
            }
            other => panic!("expected AbiVersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn validate_registration_rejects_struct_size_mismatch() {
        let cops = [fake_cop(Some(7))];
        let reg = PluginRegistration {
            abi_version: MURPHY_PLUGIN_ABI_VERSION,
            cops_ptr: cops.as_ptr(),
            cops_len: cops.len(),
        };
        let err = unsafe { validate_registration(&reg) }.unwrap_err();
        match err {
            LoaderError::StructSizeMismatch {
                cop_index,
                expected,
                got,
            } => {
                assert_eq!(cop_index, 0);
                assert_eq!(expected, std::mem::size_of::<PluginCopV1>());
                assert_eq!(got, 7);
            }
            other => panic!("expected StructSizeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn validate_registration_rejects_null_cops_ptr_with_nonzero_len() {
        let reg = PluginRegistration {
            abi_version: MURPHY_PLUGIN_ABI_VERSION,
            cops_ptr: std::ptr::null(),
            cops_len: 3,
        };
        let err = unsafe { validate_registration(&reg) }.unwrap_err();
        match err {
            LoaderError::NullCopsPointer { cops_len } => assert_eq!(cops_len, 3),
            other => panic!("expected NullCopsPointer, got {other:?}"),
        }
    }

    #[test]
    fn validate_registration_accepts_zero_cops() {
        let reg = PluginRegistration {
            abi_version: MURPHY_PLUGIN_ABI_VERSION,
            cops_ptr: std::ptr::null(),
            cops_len: 0,
        };
        let got = unsafe { validate_registration(&reg) }.expect("zero cops should be allowed");
        assert!(got.is_empty());
    }
}
