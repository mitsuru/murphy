use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyEmitOffense, MurphyFileContext, MurphyPluginCopV1,
    MurphyPluginOffense, MurphyPluginV1, MurphyRange, MurphySlice,
};
use std::ffi::c_void;

const COP_NAME: &[u8] = b"Example/FileBanner";
const MESSAGE: &[u8] = b"example native plugin ran";

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

static COPS: [MurphyPluginCopV1; 1] = [MurphyPluginCopV1 {
    size: std::mem::size_of::<MurphyPluginCopV1>(),
    name: slice(COP_NAME),
    run_file: Some(run_file),
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
        };
    }

    0
}
