use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense,
    MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense, MurphyPluginV1, MurphyRange,
    MurphySlice,
};
use std::ffi::c_void;

const COP_NAME: &[u8] = b"Example/FileBanner";
const CALL_COP_NAME: &[u8] = b"Example/CallDispatch";
const PACK_DISPATCH_COP_NAME: &[u8] = b"Example/PackDispatch";
const MESSAGE: &[u8] = b"example native plugin ran";
const CALL_MESSAGE: &[u8] = b"example call dispatch ran";
const PACK_DISPATCH_MESSAGE: &[u8] = b"example pack dispatch ran";
const EXAMPLE_CALL: &[u8] = b"example_call";
const EXAMPLE_CALL_DISPATCH_ID: usize = 7;

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

fn slice_to_bytes(slice: MurphySlice) -> Option<Vec<u8>> {
    if slice.len == 0 {
        return Some(Vec::new());
    }
    if slice.ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) }.to_vec())
}

fn configured_message(ctx: &MurphyFileContext) -> Option<String> {
    let config = slice_to_bytes(ctx.config)?;
    let json: serde_json::Value = serde_json::from_slice(&config).ok()?;
    json.get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn configured_call_message(ctx: &MurphyCallContext, cop_name: &str) -> Option<String> {
    let config = slice_to_bytes(ctx.config)?;
    let json: serde_json::Value = serde_json::from_slice(&config).ok()?;
    json.get(cop_name)
        .and_then(|cop| cop.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

unsafe extern "C" fn run_file(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*ctx };
    let configured_message = configured_message(ctx);
    let message = configured_message
        .as_deref()
        .unwrap_or_else(|| std::str::from_utf8(MESSAGE).expect("static message is valid UTF-8"));
    let offense = MurphyPluginOffense {
        cop_name: slice(COP_NAME),
        message: MurphySlice {
            ptr: message.as_ptr(),
            len: message.len(),
        },
        range: MurphyRange {
            start_offset: 0,
            end_offset: 0,
        },
        severity: 0,
        autocorrect: std::ptr::null(),
    };
    unsafe { emit(sink, &offense) };
    0
}

unsafe extern "C" fn run_call_dispatch(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*ctx };
    if ctx.dispatch_id != EXAMPLE_CALL_DISPATCH_ID {
        return 0;
    }
    let pack_offense = MurphyPluginOffense {
        cop_name: slice(PACK_DISPATCH_COP_NAME),
        message: slice(PACK_DISPATCH_MESSAGE),
        range: ctx.message_range,
        severity: 0,
        autocorrect: std::ptr::null(),
    };
    unsafe { emit(sink, &pack_offense) };

    let configured_call_message = configured_call_message(ctx, "Example/CallDispatch");
    let call_message = configured_call_message.as_deref().unwrap_or_else(|| {
        std::str::from_utf8(CALL_MESSAGE).expect("static message is valid UTF-8")
    });
    let call_offense = MurphyPluginOffense {
        cop_name: slice(CALL_COP_NAME),
        message: MurphySlice {
            ptr: call_message.as_ptr(),
            len: call_message.len(),
        },
        range: ctx.message_range,
        severity: 0,
        autocorrect: std::ptr::null(),
    };
    unsafe { emit(sink, &call_offense) };
    0
}

static COPS: [MurphyPluginCopV1; 3] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(COP_NAME),
        run_file: Some(run_file),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(CALL_COP_NAME),
        run_file: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(PACK_DISPATCH_COP_NAME),
        run_file: None,
    },
];

static CALL_DISPATCH: [MurphyCallDispatchV1; 1] = [MurphyCallDispatchV1 {
    method_name: slice(EXAMPLE_CALL),
    dispatch_id: EXAMPLE_CALL_DISPATCH_ID,
}];

#[unsafe(no_mangle)]
pub extern "C" fn murphy_plugin_abi_version() -> u32 {
    MURPHY_PLUGIN_ABI_VERSION
}

/// Register the example plugin's static ABI tables.
///
/// # Safety
///
/// `plugin` must be either null or a valid, writable pointer to a
/// `MurphyPluginV1` owned by the Murphy host for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn murphy_register_plugin(plugin: *mut MurphyPluginV1) -> i32 {
    if plugin.is_null() {
        return -1;
    }

    unsafe {
        *plugin = MurphyPluginV1 {
            size: std::mem::size_of::<MurphyPluginV1>(),
            cops_ptr: COPS.as_ptr(),
            cops_len: COPS.len(),
            call_dispatch_ptr: CALL_DISPATCH.as_ptr(),
            call_dispatch_len: CALL_DISPATCH.len(),
            run_call_dispatch: Some(run_call_dispatch),
        };
    }

    0
}
