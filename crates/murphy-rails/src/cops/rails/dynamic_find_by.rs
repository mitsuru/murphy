use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/DynamicFindBy";
pub(crate) const MESSAGE_BYTES: &[u8] = b"prefer find_by";

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
        b"find_by_",
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
        b"find_all_by_",
        NAME,
        util::slice(MESSAGE_BYTES),
        emit,
        sink,
    )
}
