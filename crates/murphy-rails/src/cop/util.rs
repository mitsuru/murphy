use murphy_core::{MurphyEmitOffense, MurphyPluginOffense, MurphyRange, MurphySlice};
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
pub(crate) fn emit_match_simple(
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
