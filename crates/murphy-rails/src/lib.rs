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

const DATE_NAME: &[u8] = b"Rails/Date";
const DATE_MESSAGE: &[u8] = b"prefer Rails time-zone-aware date helpers";

const DEFAULT_SCOPE_NAME: &[u8] = b"Rails/DefaultScope";
const DEFAULT_SCOPE_MESSAGE: &[u8] = b"avoid default_scope";

const HAS_MANY_OR_HAS_ONE_DEPENDENT_NAME: &[u8] = b"Rails/HasManyOrHasOneDependent";
const HAS_MANY_OR_HAS_ONE_DEPENDENT_MESSAGE: &[u8] = b"define dependent option for has_many/has_one";

const REQUEST_REFERER_NAME: &[u8] = b"Rails/RequestReferer";
const REQUEST_REFERER_MESSAGE: &[u8] = b"use request.referrer";

const COPS: [MurphyPluginCopV1; 7] = [
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
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(DATE_NAME),
        run_file: Some(date),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(DEFAULT_SCOPE_NAME),
        run_file: Some(default_scope),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(HAS_MANY_OR_HAS_ONE_DEPENDENT_NAME),
        run_file: Some(has_many_or_has_one_dependent),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: slice(REQUEST_REFERER_NAME),
        run_file: Some(request_referer),
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
    require_line_without: Option<&[u8]>,
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

            if requires_line_without_match(source, start, end, require_line_without) {
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
            }

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

#[inline]
fn requires_line_without_match(
    source: &[u8],
    start: usize,
    end: usize,
    require_line_without: Option<&[u8]>,
) -> bool {
    if let Some(bad_pattern) = require_line_without {
        if bad_pattern.is_empty() {
            return true;
        }

        let line_start = source[..start]
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |position| position + 1);
        let line_len = source[end..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(source.len() - end, |position| position);
        let line_end = end + line_len;
        let line = &source[line_start..line_end];

        !line.windows(bad_pattern.len()).any(|window| window == bad_pattern)
    } else {
        true
    }
}

#[inline]
fn emit_match_simple(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    emit_match(
        source,
        pattern,
        cop_name,
        message,
        None,
        emit,
        sink,
    )
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
    emit_match_simple(
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
    emit_match_simple(
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
    emit_match_simple(
        source,
        b"html_safe",
        slice(HTML_SAFE_NAME),
        slice(HTML_SAFE_MESSAGE),
        emit,
        sink,
    )
}

unsafe extern "C" fn date(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };

    if emit_match_simple(
        source,
        b"Date.today",
        slice(DATE_NAME),
        slice(DATE_MESSAGE),
        emit,
        sink,
    ) != 0
    {
        return 1;
    }

    emit_match_simple(
        source,
        b"Time.now",
        slice(DATE_NAME),
        slice(DATE_MESSAGE),
        emit,
        sink,
    )
}

unsafe extern "C" fn default_scope(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };
    emit_match_simple(
        source,
        b"default_scope",
        slice(DEFAULT_SCOPE_NAME),
        slice(DEFAULT_SCOPE_MESSAGE),
        emit,
        sink,
    )
}

unsafe extern "C" fn has_many_or_has_one_dependent(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };

    let patterns: [&[u8]; 2] = [b"has_many", b"has_one"];
    for pattern in patterns {
        if emit_match(
            source,
            pattern,
            slice(HAS_MANY_OR_HAS_ONE_DEPENDENT_NAME),
            slice(HAS_MANY_OR_HAS_ONE_DEPENDENT_MESSAGE),
            Some(b"dependent:"),
            emit,
            sink,
        ) != 0
        {
            return 1;
        }
    }

    0
}

unsafe extern "C" fn request_referer(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };
    emit_match_simple(
        source,
        b"request.referer",
        slice(REQUEST_REFERER_NAME),
        slice(REQUEST_REFERER_MESSAGE),
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
