use murphy_core::{
    MurphyEmitOffense, MurphyPluginAutocorrect, MurphyPluginEdit, MurphyPluginOffense, MurphyRange,
    MurphySlice,
};
use std::ffi::c_void;

pub(crate) const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

#[inline]
pub(crate) fn emit_match(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    require_line_without: Option<&[u8]>,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    emit_match_with_replacement_opt(
        source,
        pattern,
        cop_name,
        message,
        require_line_without,
        None,
        emit,
        sink,
        |_, _, _| true,
    )
}

#[inline]
pub(crate) fn emit_match_simple(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    emit_match(source, pattern, cop_name, message, None, emit, sink)
}

#[inline]
pub(crate) fn emit_match_filtered<F>(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
    mut keep: F,
) -> i32
where
    F: FnMut(&[u8], usize, usize) -> bool,
{
    emit_match_with_replacement_opt(
        source,
        pattern,
        cop_name,
        message,
        None,
        None,
        emit,
        sink,
        |source, start, end| keep(source, start, end),
    )
}

#[inline]
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_match_with_replacement(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    require_line_without: Option<&[u8]>,
    replacement: &'static [u8],
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    emit_match_with_replacement_opt(
        source,
        pattern,
        cop_name,
        message,
        require_line_without,
        Some(replacement),
        emit,
        sink,
        |_, _, _| true,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_match_with_replacement_opt(
    source: &[u8],
    pattern: &[u8],
    cop_name: MurphySlice,
    message: MurphySlice,
    require_line_without: Option<&[u8]>,
    replacement: Option<&'static [u8]>,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
    mut keep: impl FnMut(&[u8], usize, usize) -> bool,
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

            if keep(source, start, end)
                && requires_line_without_match(source, start, end, require_line_without)
            {
                let start = match u32::try_from(start) {
                    Ok(v) => v,
                    Err(_) => return 1,
                };
                let end = match u32::try_from(end) {
                    Ok(v) => v,
                    Err(_) => return 1,
                };

                let autocorrect: Option<(MurphyPluginEdit, MurphyPluginAutocorrect)> = replacement
                    .map(|replacement| {
                        let edit = MurphyPluginEdit {
                            range: MurphyRange {
                                start_offset: start,
                                end_offset: end,
                            },
                            replacement: MurphySlice {
                                ptr: replacement.as_ptr(),
                                len: replacement.len(),
                            },
                        };
                        let plugin_autocorrect = MurphyPluginAutocorrect {
                            edits_ptr: &edit,
                            edits_len: 1,
                        };
                        (edit, plugin_autocorrect)
                    });
                let autocorrect_ptr = autocorrect
                    .as_ref()
                    .map_or(std::ptr::null(), |(_, plugin_autocorrect)| {
                        plugin_autocorrect as *const MurphyPluginAutocorrect
                    });

                let offense = MurphyPluginOffense {
                    cop_name,
                    message,
                    range: MurphyRange {
                        start_offset: start,
                        end_offset: end,
                    },
                    severity: 0,
                    autocorrect: autocorrect_ptr,
                };

                unsafe { emit(sink, &offense) };
            }

            i = end;
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

        !line
            .windows(bad_pattern.len())
            .any(|window| window == bad_pattern)
    } else {
        true
    }
}
