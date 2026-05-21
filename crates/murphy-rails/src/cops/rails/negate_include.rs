use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/NegateInclude";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Use `.exclude?` and remove the negation part.";

pub(crate) const NAME: MurphySlice = util::slice(NAME_BYTES);

pub(crate) unsafe extern "C" fn run(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }

    let source = unsafe { std::slice::from_raw_parts((*ctx).source.ptr, (*ctx).source.len) };

    let patterns: [&[u8]; 2] = [b"!", b".include?"];
    for pattern in patterns {
        if util::emit_match_filtered(
            source,
            pattern,
            NAME,
            util::slice(MESSAGE_BYTES),
            emit,
            sink,
            |source, start, _| pattern != b"!" || source[start..].starts_with(b"!.include?"),
        ) != 0
        {
            return 1;
        }
    }

    0
}
