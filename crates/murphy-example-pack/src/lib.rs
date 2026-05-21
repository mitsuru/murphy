use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense,
    MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense, MurphyPluginV1, MurphyRange,
    MurphySlice,
};
use std::ffi::c_void;

const COP_NAME: &[u8] = b"Example/FileBanner";
const CALL_COP_NAME: &[u8] = b"Example/CallDispatch";
const MESSAGE: &[u8] = b"example native plugin ran";
const CALL_MESSAGE: &[u8] = b"example call dispatch ran";
const EXAMPLE_CALL: &[u8] = b"example_call";

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

unsafe extern "C" fn run_file(
    _ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    let offense = MurphyPluginOffense {
        cop_name: slice(COP_NAME),
        message: slice(MESSAGE),
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

unsafe extern "C" fn run_call(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*ctx };
    let offense = MurphyPluginOffense {
        cop_name: slice(CALL_COP_NAME),
        message: slice(CALL_MESSAGE),
        range: ctx.message_range,
        severity: 0,
        autocorrect: std::ptr::null(),
    };
    unsafe { emit(sink, &offense) };
    0
}

const CALL_COP_INDEX: usize = 1;
static EXAMPLE_CALL_COPS: [usize; 1] = [CALL_COP_INDEX];

static COPS: [MurphyPluginCopV1; 2] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(COP_NAME),
        run_file: Some(run_file),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(CALL_COP_NAME),
        run_file: None,
        run_call: Some(run_call),
    },
];

static CALL_DISPATCH: [MurphyCallDispatchV1; 1] = [MurphyCallDispatchV1 {
    method_name: slice(EXAMPLE_CALL),
    cop_indices_ptr: EXAMPLE_CALL_COPS.as_ptr(),
    cop_indices_len: EXAMPLE_CALL_COPS.len(),
}];

#[unsafe(no_mangle)]
pub extern "C" fn murphy_plugin_abi_version() -> u32 {
    MURPHY_PLUGIN_ABI_VERSION
}

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
        };
    }

    0
}
