use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/SaveBang";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Use `%<prefer>s` instead of `%<current>s` if the return value is not checked.";

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

    let patterns: [&[u8]; 17] = [
        b".create",
        b"create(",
        b".create_or_find_by",
        b"create_or_find_by(",
        b".first_or_create",
        b"first_or_create(",
        b".find_or_create_by",
        b"find_or_create_by(",
        b".save",
        b"save(",
        b".update",
        b"update(",
        b".update_attributes",
        b"update_attributes(",
        b".destroy",
        b"destroy(",
        b"save_bang",
    ];
    for pattern in patterns {
        if util::emit_match_simple(
            source,
            pattern,
            NAME,
            util::slice(MESSAGE_BYTES),
            emit,
            sink,
        ) != 0
        {
            return 1;
        }
    }

    0
}
