use crate::cop::CallDispatchRestriction;
use crate::{Autocorrect, Cop, CopContext, Edit, Offense, Range, Severity};
use std::ffi::c_void;

pub const MURPHY_PLUGIN_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphySlice {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyRange {
    pub start_offset: u32,
    pub end_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginEdit {
    pub range: MurphyRange,
    pub replacement: MurphySlice,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginAutocorrect {
    pub edits_ptr: *const MurphyPluginEdit,
    pub edits_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginOffense {
    pub cop_name: MurphySlice,
    pub message: MurphySlice,
    pub range: MurphyRange,
    pub severity: u32,
    pub autocorrect: *const MurphyPluginAutocorrect,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyFileContext {
    pub file: MurphySlice,
    pub source: MurphySlice,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyCallContext {
    pub file: MurphySlice,
    pub source: MurphySlice,
    pub name: MurphySlice,
    pub dispatch_id: usize,
    pub message_range: MurphyRange,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginCopV1 {
    pub size: usize,
    pub name: MurphySlice,
    pub run_file: Option<MurphyRunFile>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyCallDispatchV1 {
    pub method_name: MurphySlice,
    pub dispatch_id: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginV1 {
    pub size: usize,
    pub cops_ptr: *const MurphyPluginCopV1,
    pub cops_len: usize,
    pub call_dispatch_ptr: *const MurphyCallDispatchV1,
    pub call_dispatch_len: usize,
    pub run_call_dispatch: Option<MurphyRunCallDispatch>,
}

// Safety: these are immutable ABI descriptors / pointer-length views used in
// plugin static tables; pointed-to data thread-safety is part of the plugin ABI
// contract.
unsafe impl Sync for MurphySlice {}
unsafe impl Sync for MurphyRange {}
unsafe impl Sync for MurphyPluginEdit {}
unsafe impl Sync for MurphyPluginAutocorrect {}
unsafe impl Sync for MurphyPluginOffense {}
unsafe impl Sync for MurphyFileContext {}
unsafe impl Sync for MurphyCallContext {}
unsafe impl Sync for MurphyPluginCopV1 {}
unsafe impl Sync for MurphyCallDispatchV1 {}
unsafe impl Sync for MurphyPluginV1 {}

pub type MurphyEmitOffense = unsafe extern "C" fn(*mut c_void, *const MurphyPluginOffense);
pub type MurphyRunFile =
    unsafe extern "C" fn(*const MurphyFileContext, MurphyEmitOffense, *mut c_void) -> i32;
pub type MurphyRunCallDispatch =
    unsafe extern "C" fn(*const MurphyCallContext, MurphyEmitOffense, *mut c_void) -> i32;

pub fn validate_plugin_cop_ids(
    existing: &[Box<dyn Cop>],
    plugin_names: &[String],
) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for cop in existing {
        seen.insert(cop.name().to_string());
    }

    for name in plugin_names {
        if name.is_empty() {
            return Err("plugin cop ID must not be empty".to_string());
        }
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate cop ID: {name}"));
        }
    }

    Ok(())
}

pub struct PluginFileCop {
    name: String,
    run_file: Option<MurphyRunFile>,
    run_call_dispatch: Option<MurphyRunCallDispatch>,
    restrict_on_send: Vec<CallDispatchRestriction>,
    #[cfg(not(target_os = "windows"))]
    _library: Option<std::sync::Arc<libloading::Library>>,
}

// Safety: the adapter owns immutable data: a String, a function pointer, and
// an optional library lifetime guard. Callbacks are expected to be thread-safe
// by the native plugin ABI contract.
unsafe impl Send for PluginFileCop {}

// Safety: shared access only reads the owned String, immutable function pointer,
// and optional library lifetime guard.
unsafe impl Sync for PluginFileCop {}

struct OffenseSink<'a> {
    file: &'a str,
    source_len: usize,
    offenses: &'a mut Vec<Offense>,
}

impl PluginFileCop {
    pub fn new(name: String, run_file: MurphyRunFile) -> Self {
        Self {
            name,
            run_file: Some(run_file),
            run_call_dispatch: None,
            restrict_on_send: Vec::new(),
            #[cfg(not(target_os = "windows"))]
            _library: None,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn with_library_guard(
        name: String,
        run_file: Option<MurphyRunFile>,
        run_call_dispatch: Option<MurphyRunCallDispatch>,
        restrict_on_send: Vec<CallDispatchRestriction>,
        library: std::sync::Arc<libloading::Library>,
    ) -> Self {
        Self {
            name,
            run_file,
            run_call_dispatch,
            restrict_on_send,
            _library: Some(library),
        }
    }
}

impl Cop for PluginFileCop {
    fn name(&self) -> &str {
        &self.name
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let Some(run_file) = self.run_file else {
            return;
        };
        let file = MurphySlice {
            ptr: ctx.file.as_ptr(),
            len: ctx.file.len(),
        };
        let source = MurphySlice {
            ptr: ctx.source.as_ptr(),
            len: ctx.source.len(),
        };
        let file_ctx = MurphyFileContext { file, source };
        let mut offense_sink = OffenseSink {
            file: ctx.file,
            source_len: ctx.source.len(),
            offenses: sink,
        };
        let status = unsafe {
            (run_file)(
                &file_ctx,
                emit_offense,
                (&mut offense_sink as *mut OffenseSink<'_>).cast(),
            )
        };
        if status != 0 {
            offense_sink.offenses.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Error,
                "native plugin callback failed",
            ));
        }
    }

    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        self.run_call_node(node, ctx, sink, 0);
    }

    fn on_restricted_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
        dispatch_id: usize,
    ) {
        self.run_call_node(node, ctx, sink, dispatch_id);
    }

    fn restrict_on_send(&self) -> Option<&[CallDispatchRestriction]> {
        if self.restrict_on_send.is_empty() {
            None
        } else {
            Some(&self.restrict_on_send)
        }
    }
}

impl PluginFileCop {
    fn run_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
        dispatch_id: usize,
    ) {
        let Some(run_call_dispatch) = self.run_call_dispatch else {
            return;
        };
        let Some(message_loc) = node.message_loc() else {
            return;
        };
        let name = node.name();
        let file = MurphySlice {
            ptr: ctx.file.as_ptr(),
            len: ctx.file.len(),
        };
        let source = MurphySlice {
            ptr: ctx.source.as_ptr(),
            len: ctx.source.len(),
        };
        let name = MurphySlice {
            ptr: name.as_slice().as_ptr(),
            len: name.as_slice().len(),
        };
        let call_ctx = MurphyCallContext {
            file,
            source,
            name,
            dispatch_id,
            message_range: Range::from_prism_location(&message_loc).into(),
        };
        let mut offense_sink = OffenseSink {
            file: ctx.file,
            source_len: ctx.source.len(),
            offenses: sink,
        };
        let status = unsafe {
            (run_call_dispatch)(
                &call_ctx,
                emit_offense,
                (&mut offense_sink as *mut OffenseSink<'_>).cast(),
            )
        };
        if status != 0 {
            offense_sink.offenses.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Error,
                "native plugin callback failed",
            ));
        }
    }
}

impl From<Range> for MurphyRange {
    fn from(range: Range) -> Self {
        MurphyRange {
            start_offset: range.start_offset,
            end_offset: range.end_offset,
        }
    }
}

unsafe extern "C" fn emit_offense(sink: *mut c_void, offense: *const MurphyPluginOffense) {
    if sink.is_null() || offense.is_null() {
        return;
    }

    let sink = unsafe { &mut *sink.cast::<OffenseSink<'_>>() };
    let offense = unsafe { &*offense };
    let Some(cop_name) = slice_to_str(&offense.cop_name) else {
        return;
    };
    let Some(message) = slice_to_str(&offense.message) else {
        return;
    };
    if offense.range.start_offset > offense.range.end_offset {
        return;
    }
    if usize::try_from(offense.range.end_offset)
        .map(|end| end > sink.source_len)
        .unwrap_or(true)
    {
        return;
    }

    let severity = if offense.severity == 1 {
        Severity::Error
    } else {
        Severity::Warning
    };
    let mut output = Offense::new(
        sink.file,
        cop_name,
        Range {
            start_offset: offense.range.start_offset,
            end_offset: offense.range.end_offset,
        },
        severity,
        message,
    );

    if let Some(autocorrect) = plugin_autocorrect_from_raw(offense.autocorrect, sink.source_len) {
        output = output.with_autocorrect(autocorrect);
    }

    sink.offenses.push(output);
}

fn plugin_autocorrect_from_raw(
    offense_autocorrect: *const MurphyPluginAutocorrect,
    source_len: usize,
) -> Option<Autocorrect> {
    if offense_autocorrect.is_null() {
        return None;
    }

    let plugin_autocorrect = unsafe { &*offense_autocorrect };
    if plugin_autocorrect.edits_len == 0 {
        return None;
    }
    if plugin_autocorrect.edits_ptr.is_null() {
        return None;
    }

    let edits = unsafe {
        std::slice::from_raw_parts(plugin_autocorrect.edits_ptr, plugin_autocorrect.edits_len)
    }
    .iter()
    .filter_map(|edit| {
        if edit.range.start_offset > edit.range.end_offset {
            return None;
        }

        let Some(replacement) = slice_to_str(&edit.replacement) else {
            return None;
        };
        let start_offset = usize::try_from(edit.range.start_offset).ok()?;
        let end_offset = usize::try_from(edit.range.end_offset).ok()?;
        if end_offset > source_len || start_offset > source_len {
            return None;
        }

        Some(Edit {
            range: Range {
                start_offset: edit.range.start_offset,
                end_offset: edit.range.end_offset,
            },
            replacement: replacement.to_string(),
        })
    })
    .collect::<Vec<_>>();

    if edits.is_empty() {
        return None;
    }

    Some(Autocorrect { edits })
}

fn slice_to_str(slice: &MurphySlice) -> Option<&str> {
    if slice.len == 0 {
        return Some("");
    }
    if slice.ptr.is_null() && slice.len != 0 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) };
    std::str::from_utf8(bytes).ok()
}

fn cop_table_len_is_valid(cops_len: usize) -> Result<(), String> {
    let max_len = isize::MAX as usize / std::mem::size_of::<MurphyPluginCopV1>();
    if cops_len > max_len {
        return Err(format!(
            "plugin cop table too large: {cops_len} entries exceeds {max_len}"
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub mod dynamic {
    use super::*;
    use libloading::{Library, Symbol};
    use std::path::Path;
    use std::sync::Arc;

    type AbiVersionFn = unsafe extern "C" fn() -> u32;
    type RegisterFn = unsafe extern "C" fn(*mut MurphyPluginV1) -> i32;

    pub struct LoadedPluginPack {
        pub name: String,
        pub cops: Vec<Box<dyn Cop>>,
        _library: Arc<Library>,
    }

    pub fn load_plugin_pack(name: &str, path: &Path) -> Result<LoadedPluginPack, String> {
        let library = Arc::new(
            unsafe { Library::new(path) }
                .map_err(|e| format!("failed to load plugin pack {}: {e}", path.display()))?,
        );

        let mut plugin = MurphyPluginV1 {
            size: std::mem::size_of::<MurphyPluginV1>(),
            cops_ptr: std::ptr::null(),
            cops_len: 0,
            call_dispatch_ptr: std::ptr::null(),
            call_dispatch_len: 0,
            run_call_dispatch: None,
        };
        {
            let abi_version: Symbol<'_, AbiVersionFn> = unsafe {
                library
                    .get(b"murphy_plugin_abi_version")
                    .map_err(|e| format!("missing symbol murphy_plugin_abi_version: {e}"))?
            };
            let got = unsafe { abi_version() };
            if got != MURPHY_PLUGIN_ABI_VERSION {
                return Err(format!(
                    "plugin ABI version mismatch: got {got}, expected {MURPHY_PLUGIN_ABI_VERSION}"
                ));
            }

            let register: Symbol<'_, RegisterFn> = unsafe {
                library
                    .get(b"murphy_register_plugin")
                    .map_err(|e| format!("missing symbol murphy_register_plugin: {e}"))?
            };
            let status = unsafe { register(&mut plugin) };
            if status != 0 {
                return Err(format!("plugin registration failed with status {status}"));
            }
        }
        if plugin.cops_len > 0 && plugin.cops_ptr.is_null() {
            return Err("plugin registered cops_len > 0 with null cops_ptr".to_string());
        }
        if plugin.call_dispatch_len > 0 && plugin.call_dispatch_ptr.is_null() {
            return Err(
                "plugin registered call_dispatch_len > 0 with null call_dispatch_ptr".to_string(),
            );
        }
        cop_table_len_is_valid(plugin.cops_len)?;

        let raw_cops = if plugin.cops_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(plugin.cops_ptr, plugin.cops_len) }
        };
        let mut plugin_names = Vec::with_capacity(raw_cops.len());
        for cop in raw_cops {
            if cop.size != std::mem::size_of::<MurphyPluginCopV1>() {
                return Err(format!(
                    "invalid plugin cop size: got {}, expected {}",
                    cop.size,
                    std::mem::size_of::<MurphyPluginCopV1>()
                ));
            }
            let name = slice_to_str(&cop.name)
                .ok_or_else(|| "plugin cop name must be valid UTF-8".to_string())?;
            if name.is_empty() {
                return Err("plugin cop ID must not be empty".to_string());
            }
            plugin_names.push(name.to_string());
        }
        validate_plugin_cop_ids(&[], &plugin_names)?;

        let raw_call_dispatch = if plugin.call_dispatch_len == 0 {
            &[]
        } else {
            unsafe {
                std::slice::from_raw_parts(plugin.call_dispatch_ptr, plugin.call_dispatch_len)
            }
        };
        let mut restrict_on_send = (0..raw_cops.len())
            .map(|_| Vec::<CallDispatchRestriction>::new())
            .collect::<Vec<_>>();
        if !raw_call_dispatch.is_empty() && plugin.run_call_dispatch.is_none() {
            return Err(
                "plugin registered call dispatch entries with null run_call_dispatch".to_string(),
            );
        }
        for entry in raw_call_dispatch {
            let method_name = slice_to_str(&entry.method_name).ok_or_else(|| {
                "plugin call dispatch method_name must be valid UTF-8".to_string()
            })?;
            if method_name.is_empty() {
                return Err("plugin call dispatch method_name must not be empty".to_string());
            }
            if raw_cops.is_empty() {
                return Err("plugin registered call dispatch entries without any cops".to_string());
            }
            restrict_on_send[0].push(CallDispatchRestriction {
                method_name: method_name.as_bytes().to_vec(),
                dispatch_id: entry.dispatch_id,
            });
        }

        let cops = raw_cops
            .iter()
            .zip(plugin_names)
            .zip(restrict_on_send)
            .map(|((cop, name), restrict_on_send)| {
                Box::new(PluginFileCop::with_library_guard(
                    name,
                    cop.run_file,
                    if restrict_on_send.is_empty() {
                        None
                    } else {
                        plugin.run_call_dispatch
                    },
                    restrict_on_send,
                    Arc::clone(&library),
                )) as Box<dyn Cop>
            })
            .collect();

        Ok(LoadedPluginPack {
            name: name.to_string(),
            cops,
            _library: library,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoReceiverPuts;

    unsafe extern "C" fn noop_run_file(
        _ctx: *const MurphyFileContext,
        _emit: MurphyEmitOffense,
        _sink: *mut c_void,
    ) -> i32 {
        0
    }

    #[test]
    fn rejects_duplicate_plugin_cop_id() {
        let existing: Vec<Box<dyn crate::Cop>> = vec![Box::new(NoReceiverPuts)];
        let err = validate_plugin_cop_ids(&existing, &["Murphy/NoReceiverPuts".to_string()])
            .expect_err("duplicate cop ID must be rejected");
        assert!(err.contains("duplicate cop ID"));
        assert!(err.contains("Murphy/NoReceiverPuts"));
    }

    #[test]
    fn plugin_cop_abi_allows_nullable_run_file() {
        let cop = MurphyPluginCopV1 {
            size: std::mem::size_of::<MurphyPluginCopV1>(),
            name: MurphySlice {
                ptr: std::ptr::null(),
                len: 0,
            },
            run_file: None,
        };

        assert!(cop.run_file.is_none());
    }

    #[test]
    fn emit_offense_rejects_range_past_source_len() {
        let cop_name = MurphySlice {
            ptr: b"Plugin/Test".as_ptr(),
            len: b"Plugin/Test".len(),
        };
        let message = MurphySlice {
            ptr: b"bad".as_ptr(),
            len: b"bad".len(),
        };
        let offense = MurphyPluginOffense {
            cop_name,
            message,
            range: MurphyRange {
                start_offset: 0,
                end_offset: 5,
            },
            severity: 0,
            autocorrect: std::ptr::null(),
        };
        let mut offenses = Vec::new();
        let mut sink = OffenseSink {
            file: "t.rb",
            source_len: 4,
            offenses: &mut offenses,
        };

        unsafe { emit_offense((&mut sink as *mut OffenseSink<'_>).cast(), &offense) };

        assert!(offenses.is_empty());
    }

    #[test]
    fn emit_offense_includes_autocorrect_payload() {
        let cop_name = MurphySlice {
            ptr: b"Plugin/Test".as_ptr(),
            len: b"Plugin/Test".len(),
        };
        let message = MurphySlice {
            ptr: b"with fix".as_ptr(),
            len: b"with fix".len(),
        };
        let replacement = b"replacement";
        let edit = MurphyPluginEdit {
            range: MurphyRange {
                start_offset: 0,
                end_offset: 4,
            },
            replacement: MurphySlice {
                ptr: replacement.as_ptr(),
                len: replacement.len(),
            },
        };
        let autocorrect = MurphyPluginAutocorrect {
            edits_ptr: &edit,
            edits_len: 1,
        };
        let offense = MurphyPluginOffense {
            cop_name,
            message,
            range: MurphyRange {
                start_offset: 0,
                end_offset: 4,
            },
            severity: 0,
            autocorrect: &autocorrect,
        };

        let mut offenses = Vec::new();
        let mut sink = OffenseSink {
            file: "t.rb",
            source_len: 10,
            offenses: &mut offenses,
        };

        unsafe { emit_offense((&mut sink as *mut OffenseSink<'_>).cast(), &offense) };

        assert_eq!(offenses.len(), 1);
        let o = &offenses[0];
        let autocorrect = o
            .autocorrect
            .as_ref()
            .expect("autocorrect should be present");
        assert_eq!(autocorrect.edits.len(), 1);
        assert_eq!(autocorrect.edits[0].replacement, "replacement");
        assert_eq!(autocorrect.edits[0].range.start_offset, 0);
        assert_eq!(autocorrect.edits[0].range.end_offset, 4);
    }

    #[test]
    fn emit_offense_ignores_invalid_autocorrect_payload() {
        let offense = MurphyPluginOffense {
            cop_name: MurphySlice {
                ptr: b"Plugin/Test".as_ptr(),
                len: b"Plugin/Test".len(),
            },
            message: MurphySlice {
                ptr: b"bad fix".as_ptr(),
                len: b"bad fix".len(),
            },
            range: MurphyRange {
                start_offset: 0,
                end_offset: 4,
            },
            severity: 0,
            autocorrect: std::ptr::null(),
        };

        let mut offenses = Vec::new();
        let mut sink = OffenseSink {
            file: "t.rb",
            source_len: 10,
            offenses: &mut offenses,
        };

        unsafe { emit_offense((&mut sink as *mut OffenseSink<'_>).cast(), &offense) };

        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].autocorrect.is_none());
    }

    #[test]
    fn cop_table_len_rejects_isize_overflow() {
        let too_many = (isize::MAX as usize / std::mem::size_of::<MurphyPluginCopV1>()) + 1;

        assert!(cop_table_len_is_valid(too_many).is_err());
    }

    #[test]
    fn plugin_file_cop_can_be_constructed_without_library_guard() {
        let cop = PluginFileCop::new("Plugin/Test".to_string(), noop_run_file);

        assert_eq!(cop.name(), "Plugin/Test");
    }
}
