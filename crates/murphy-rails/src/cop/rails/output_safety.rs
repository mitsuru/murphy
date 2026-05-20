use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/OutputSafety";
pub(crate) const MESSAGE_BYTES: &[u8] = b"avoid calling raw directly in views";

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

    util::emit_match_simple(
        source,
        b"raw(",
        NAME,
        util::slice(MESSAGE_BYTES),
        emit,
        sink,
    )
}
