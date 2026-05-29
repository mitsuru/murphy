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

/// Failure context for a name-only `plugins = ["..."]` entry that the
/// resolver could not locate against the search path (ADR 0042).
///
/// Carried structurally rather than as a pre-formatted string so the
/// host-side [`PluginLoadDiagnostic`] can render a consistent
/// `error:` / `cause:` / `hint:` block for both resolve and load
/// failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveFailure {
    /// The `lib{name}.so` / `lib{name}.dylib` the resolver looked for.
    pub filename: String,
    /// The dirs that were probed in order (env, project-local,
    /// user-local). Empty when nothing was configured.
    pub searched_dirs: Vec<std::path::PathBuf>,
}

/// Which side of the plugin pipeline raised the failure: the search-
/// path resolver or the dlopen-and-validate loader.
#[derive(Debug)]
pub enum LoadKind {
    Resolve(ResolveFailure),
    Load(LoaderError),
}

/// User-facing diagnostic for a plugin pack that failed to resolve or
/// load. Renders a rustc-style three-block message (`error:` /
/// `cause:` / `hint:`) via [`std::fmt::Display`].
///
/// Created at the registry boundary where the plugin name (from
/// `plugins:`) and the attempted path (None for resolve failures)
/// are known.
#[derive(Debug)]
pub struct PluginLoadDiagnostic {
    pub plugin_name: String,
    /// `Some` when the resolver succeeded and dlopen/validate failed;
    /// `None` when the resolver itself could not locate the pack.
    pub attempted_path: Option<std::path::PathBuf>,
    pub kind: LoadKind,
}

impl std::fmt::Display for PluginLoadDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.attempted_path {
            Some(path) => writeln!(
                f,
                "error: cannot load plugin `{}` from `{}`",
                self.plugin_name,
                path.display()
            )?,
            None => writeln!(f, "error: cannot load plugin `{}`", self.plugin_name)?,
        }
        let (cause, hint) = self.cause_and_hint();
        writeln!(f, "  cause: {cause}")?;
        // hint may contain literal '\n' for multi-line; indent
        // continuation lines to align under the `hint:  ` label
        // (9 columns: 2 spaces + "hint:  ").
        let mut lines = hint.lines();
        if let Some(first) = lines.next() {
            write!(f, "  hint:  {first}")?;
        }
        for line in lines {
            write!(f, "\n         {line}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PluginLoadDiagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            LoadKind::Load(err) => Some(err),
            // ResolveFailure is structural data, not an Error type —
            // omit from the chain rather than synthesize a wrapper.
            LoadKind::Resolve(_) => None,
        }
    }
}

impl PluginLoadDiagnostic {
    fn cause_and_hint(&self) -> (String, String) {
        match &self.kind {
            LoadKind::Resolve(rf) => {
                let cause = format!("plugin pack `{}` not found in search path", rf.filename);
                let searched = if rf.searched_dirs.is_empty() {
                    "<none>".to_string()
                } else {
                    rf.searched_dirs
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let hint = format!(
                    "searched: {searched}.\n\
                     To pin an explicit path, use the detailed form in `.murphy.yml`:\n\
                     `plugins:\\n  - name: \"{name}\"\\n    path: ...`.\n\
                     See ADR 0042 for the search-path order.",
                    name = self.plugin_name,
                );
                (cause, hint)
            }
            LoadKind::Load(err) => match err {
                LoaderError::Open(msg) => (
                    format!("dlopen failed: {msg}"),
                    format!(
                        "confirm the path exists, or use name-only shorthand\n\
                         `plugins: [{name}]` in `.murphy.yml` to search MURPHY_PLUGIN_PATH /\n\
                         .murphy/plugins/ (ADR 0042).",
                        name = self.plugin_name,
                    ),
                ),
                LoaderError::MissingSymbol(_) => (
                    "required symbol `murphy_plugin_register` not found".to_string(),
                    "the pack must use `murphy_plugin_api::register_cops!`.\n\
                     Packs built against the pre-9cr.13 ABI must be rebuilt (ADR 0038)."
                        .to_string(),
                ),
                LoaderError::RegisterFailed(rc) => (
                    format!("plugin registration returned non-zero status {rc}"),
                    "the pack's initialization trapped a panic or returned failure —\n\
                     inspect the pack's stderr. This is a pack bug."
                        .to_string(),
                ),
                LoaderError::AbiVersionMismatch { expected, got } => (
                    format!("plugin built against ABI version {got}, host expects {expected}"),
                    format!(
                        "rebuild the pack against `murphy-plugin-api = {expected}`\n\
                         (the version this murphy binary embeds).",
                    ),
                ),
                LoaderError::StructSizeMismatch {
                    cop_index,
                    expected,
                    got,
                } => (
                    format!(
                        "cop #{cop_index}'s PluginCopV1 is {got} bytes, host expects {expected}"
                    ),
                    "pack and host disagree on struct layout; rebuild the pack\n\
                     against the same `murphy-plugin-api` revision as this murphy binary."
                        .to_string(),
                ),
                LoaderError::NullCopsPointer { cops_len } => (
                    format!("registration reports {cops_len} cops but a null table pointer"),
                    "this is a pack bug — `register_cops!` should never emit this\n\
                     combination. File an issue against the pack author."
                        .to_string(),
                ),
            },
        }
    }
}

/// Validate a [`PluginRegistration`] in isolation (no `dlopen`) and
/// return the raw `(ptr, len)` of the validated cop table on success.
/// Factored out so unit tests can drive every rejection branch without a
/// real `.so`.
///
/// The return type is deliberately raw, not `&[PluginCopV1]`: the loader
/// itself does not know what owner the pointer is anchored to (the
/// caller does — for `load_plugin_pack`, the [`libloading::Library`]
/// being constructed alongside). Promoting the pointer to a borrow with
/// the wrong lifetime is the soundness hole this signature avoids.
///
/// # Safety
/// `cops_ptr` must point to `cops_len` consecutive `PluginCopV1` values
/// valid for at least as long as the caller intends to use the returned
/// pointer (typically: the lifetime of the owning `LoadedPluginPack` or
/// `&'static`). When `cops_len == 0`, the pointer may be null and is
/// ignored.
pub unsafe fn validate_registration(
    reg: &murphy_plugin_api::PluginRegistration,
) -> Result<(*const PluginCopV1, usize), LoaderError> {
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
    let cops_slice: &[PluginCopV1] = if reg.cops_len == 0 {
        &[]
    } else {
        // Safety: see contract above; the slice borrow is local — used
        // only to walk the table for size checks — and does not escape.
        unsafe { std::slice::from_raw_parts(reg.cops_ptr, reg.cops_len) }
    };
    let expected_size = std::mem::size_of::<PluginCopV1>();
    for (cop_index, cop) in cops_slice.iter().enumerate() {
        if cop.size != expected_size {
            return Err(LoaderError::StructSizeMismatch {
                cop_index,
                expected: expected_size,
                got: cop.size,
            });
        }
    }
    Ok((reg.cops_ptr, reg.cops_len))
}

/// A loaded plugin pack, holding the live `Library` handle and a raw
/// view of the cop table the registration declared.
///
/// The `_library` field owns the `dlopen` handle: dropping the pack
/// `dlclose`s the library, which invalidates the cop pointers. Direct
/// borrowed access is therefore exposed ONLY through [`Self::cops`],
/// whose return lifetime is bound to `&self` — making it impossible to
/// keep a `&PluginCopV1` alive past the pack's drop in safe code.
#[cfg(not(target_os = "windows"))]
pub struct LoadedPluginPack {
    /// Original path, kept for diagnostics.
    pub path: std::path::PathBuf,
    /// Validated `(cops_ptr, cops_len)` from the plugin's registration.
    /// Kept raw — the only safe way to obtain a `&[PluginCopV1]` view
    /// is via [`Self::cops`], which ties the lifetime to `&self`.
    cops_ptr: *const PluginCopV1,
    cops_len: usize,
    _library: libloading::Library,
}

// Safety: a `LoadedPluginPack` is an immutable bundle of raw pointers
// and a `libloading::Library` handle (which is already `Send + Sync`).
// The borrowed cop table view is exposed only through `&self` methods.
#[cfg(not(target_os = "windows"))]
unsafe impl Send for LoadedPluginPack {}
#[cfg(not(target_os = "windows"))]
unsafe impl Sync for LoadedPluginPack {}

#[cfg(not(target_os = "windows"))]
impl LoadedPluginPack {
    /// Borrow the validated cop table. The slice is valid for the
    /// pack's lifetime — `dlclose` runs only when this pack is dropped,
    /// which is impossible while the returned borrow is live.
    pub fn cops(&self) -> &[PluginCopV1] {
        if self.cops_len == 0 {
            &[]
        } else {
            // Safety: `validate_registration` checked the pointer +
            // length; the library handle in `_library` keeps the data
            // mapped for the lifetime of `&self`.
            unsafe { std::slice::from_raw_parts(self.cops_ptr, self.cops_len) }
        }
    }
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
    // cop table) and lives as long as `library`. Storing the raw
    // `(ptr, len)` alongside the library handle keeps that lifetime
    // bound enforced through `LoadedPluginPack::cops`'s borrow.
    let (cops_ptr, cops_len) = unsafe { validate_registration(&reg)? };
    Ok(LoadedPluginPack {
        path: path.to_path_buf(),
        cops_ptr,
        cops_len,
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
            send_methods_ptr: std::ptr::null(),
            send_methods_len: 0,
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
        let (ptr, len) = unsafe { validate_registration(&reg) }.expect("should validate");
        assert_eq!(len, 1);
        assert_eq!(ptr, cops.as_ptr());
        // Safety: `cops` outlives this borrow; validate_registration's
        // raw output is intentionally untyped, so the test re-borrows.
        let view = unsafe { std::slice::from_raw_parts(ptr, len) };
        assert_eq!(view[0].size, std::mem::size_of::<PluginCopV1>());
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
        let (_, len) = unsafe { validate_registration(&reg) }.expect("zero cops should be allowed");
        assert_eq!(len, 0);
    }

    // PluginLoadDiagnostic rendering tests. The inline snapshots capture
    // the exact user-facing text — when a hint copy needs to change,
    // run `UPDATE_EXPECT=1 cargo test -p murphy-core diag_render` to
    // refresh them all in one pass.
    mod diagnostics {
        use super::super::{LoadKind, LoaderError, PluginLoadDiagnostic, ResolveFailure};
        use expect_test::expect;
        use std::path::PathBuf;

        fn loaded(path: &str, kind: LoadKind) -> PluginLoadDiagnostic {
            PluginLoadDiagnostic {
                plugin_name: "murphy-foo".to_string(),
                attempted_path: Some(PathBuf::from(path)),
                kind,
            }
        }

        #[test]
        fn diag_render_resolve() {
            let diag = PluginLoadDiagnostic {
                plugin_name: "murphy-foo".to_string(),
                attempted_path: None,
                kind: LoadKind::Resolve(ResolveFailure {
                    filename: "libmurphy_foo.so".to_string(),
                    searched_dirs: vec![
                        PathBuf::from("/opt/murphy/plugins"),
                        PathBuf::from("/home/u/.murphy/plugins"),
                    ],
                }),
            };
            expect![[r#"
                error: cannot load plugin `murphy-foo`
                  cause: plugin pack `libmurphy_foo.so` not found in search path
                  hint:  searched: /opt/murphy/plugins, /home/u/.murphy/plugins.
                         To pin an explicit path, use the detailed form in `.murphy.yml`:
                         `plugins:\n  - name: "murphy-foo"\n    path: ...`.
                         See ADR 0042 for the search-path order."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_open() {
            let diag = loaded(
                "./vendor/libmurphy_foo.so",
                LoadKind::Load(LoaderError::Open(
                    "libmurphy_foo.so: cannot open shared object file: No such file or directory"
                        .to_string(),
                )),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./vendor/libmurphy_foo.so`
                  cause: dlopen failed: libmurphy_foo.so: cannot open shared object file: No such file or directory
                  hint:  confirm the path exists, or use name-only shorthand
                         `plugins: [murphy-foo]` in `.murphy.yml` to search MURPHY_PLUGIN_PATH /
                         .murphy/plugins/ (ADR 0042)."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_missing_symbol() {
            let diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::MissingSymbol(
                    "murphy_plugin_register".to_string(),
                )),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./libfoo.so`
                  cause: required symbol `murphy_plugin_register` not found
                  hint:  the pack must use `murphy_plugin_api::register_cops!`.
                         Packs built against the pre-9cr.13 ABI must be rebuilt (ADR 0038)."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_register_failed() {
            let diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::RegisterFailed(-1)),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./libfoo.so`
                  cause: plugin registration returned non-zero status -1
                  hint:  the pack's initialization trapped a panic or returned failure —
                         inspect the pack's stderr. This is a pack bug."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_abi_mismatch() {
            let diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::AbiVersionMismatch {
                    expected: 1,
                    got: 2,
                }),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./libfoo.so`
                  cause: plugin built against ABI version 2, host expects 1
                  hint:  rebuild the pack against `murphy-plugin-api = 1`
                         (the version this murphy binary embeds)."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_struct_size_mismatch() {
            let diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::StructSizeMismatch {
                    cop_index: 3,
                    expected: 64,
                    got: 56,
                }),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./libfoo.so`
                  cause: cop #3's PluginCopV1 is 56 bytes, host expects 64
                  hint:  pack and host disagree on struct layout; rebuild the pack
                         against the same `murphy-plugin-api` revision as this murphy binary."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_render_null_cops_ptr() {
            let diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::NullCopsPointer { cops_len: 5 }),
            );
            expect![[r#"
                error: cannot load plugin `murphy-foo` from `./libfoo.so`
                  cause: registration reports 5 cops but a null table pointer
                  hint:  this is a pack bug — `register_cops!` should never emit this
                         combination. File an issue against the pack author."#]]
            .assert_eq(&diag.to_string());
        }

        #[test]
        fn diag_resolve_omits_attempted_path() {
            // Property: a Resolve-kind diagnostic must not render the
            // `from ...` clause on the error line; that path was never
            // reached.
            let diag = PluginLoadDiagnostic {
                plugin_name: "x".to_string(),
                attempted_path: None,
                kind: LoadKind::Resolve(ResolveFailure {
                    filename: "libx.so".to_string(),
                    searched_dirs: vec![],
                }),
            };
            let s = diag.to_string();
            let first_line = s.lines().next().unwrap();
            assert_eq!(first_line, "error: cannot load plugin `x`");
            assert!(!first_line.contains("from"), "got: {first_line}");
        }

        #[test]
        fn diag_source_chain_exposes_inner_loader_error() {
            // Property: std::error::Error::source() routes through
            // LoadKind::Load to surface the LoaderError; Resolve has no
            // source (it carries structural data only).
            use std::error::Error;
            let loaded_diag = loaded(
                "./libfoo.so",
                LoadKind::Load(LoaderError::RegisterFailed(7)),
            );
            let src = loaded_diag
                .source()
                .expect("Load variant must expose source");
            assert!(
                src.downcast_ref::<LoaderError>().is_some(),
                "source should be a LoaderError"
            );

            let resolve_diag = PluginLoadDiagnostic {
                plugin_name: "x".to_string(),
                attempted_path: None,
                kind: LoadKind::Resolve(ResolveFailure {
                    filename: "libx.so".to_string(),
                    searched_dirs: vec![],
                }),
            };
            assert!(resolve_diag.source().is_none());
        }
    }
}
