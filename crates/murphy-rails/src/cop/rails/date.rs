use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/Date";
pub(crate) const MESSAGE_BYTES: &[u8] = b"prefer Rails time-zone-aware date helpers";

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

    if util::emit_match_simple(
        source,
        b"Date.today",
        NAME,
        util::slice(MESSAGE_BYTES),
        emit,
        sink,
    ) != 0
    {
        return 1;
    }

    util::emit_match_simple(
        source,
        b"Time.now",
        NAME,
        util::slice(MESSAGE_BYTES),
        emit,
        sink,
    )
}
