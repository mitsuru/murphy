use murphy_core::{
    MurphyEmitOffense, MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense, MurphyPluginV1,
    MurphyRange, MurphySlice, MURPHY_PLUGIN_ABI_VERSION,
};
use std::ffi::c_void;

const HAS_AND_BELONGS_TO_MANY_NAME: &[u8] = b"Rails/HasAndBelongsToMany";
const HAS_AND_BELONGS_TO_MANY_MESSAGE: &[u8] = b"prefer has_many :through instead";

const FIND_ALL_NAME: &[u8] = b"Rails/FindEach";
const FIND_ALL_MESSAGE: &[u8] = b"use find_each for batch processing";

const HTML_SAFE_NAME: &[u8] = b"Rails/HtmlSafe";
const HTML_SAFE_MESSAGE: &[u8] = b"avoid calling html_safe directly";

const COPS: [MurphyPluginCopV1; 3] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(HAS_AND_BELONGS_TO_MANY_NAME),
        run_file: Some(has_and_belongs_to_many),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(FIND_ALL_NAME),
        run_file: Some(find_all),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(HTML_SAFE_NAME),
        run_file: Some(html_safe),
    },
];

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

#[inline]
fn emit_match(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if pattern.is_empty() || source.is_empty() {
        return 0;
    }

    let mut i = 0usize;
    while i + pattern.len() <= source.len() {
        if let Some(offset) = source[i..]
            .windows(pattern.len())
            .position(|window| window == pattern)
        {
            let start = i + offset;
            let end = start + pattern.len();

            let start = match u32::try_from(start) {
                Ok(v) => v,
                Err(_) => return 1,
            };
            let end = match u32::try_from(end) {
                Ok(v) => v,
                Err(_) => return 1,
            };

            let offense = MurphyPluginOffense {
                cop_name,
                message,
                range: MurphyRange {
                    start_offset: start,
                    end_offset: end,
                },
                severity: 0,
            };
            unsafe { emit(sink, &offense) };

            let next_index = match usize::try_from(end) {
                Ok(v) => v,
                Err(_) => return 1,
            };
            i = next_index;
            continue;
        }
        break;
    }

    0
}

unsafe extern "C" fn has_and_belongs_to_many(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };
    emit_match(
        source,
        b"has_and_belongs_to_many",
        slice(HAS_AND_BELONGS_TO_MANY_NAME),
        slice(HAS_AND_BELONGS_TO_MANY_MESSAGE),
        emit,
        sink,
    )
}

unsafe extern "C" fn find_all(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };
    emit_match(
        source,
        b"find(:all",
        slice(FIND_ALL_NAME),
        slice(FIND_ALL_MESSAGE),
        emit,
        sink,
    )
}

unsafe extern "C" fn html_safe(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };
    emit_match(
        source,
        b"html_safe",
        slice(HTML_SAFE_NAME),
        slice(HTML_SAFE_MESSAGE),
        emit,
        sink,
    )
}

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
