use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/LexicallyScopedActionFilter";
pub(crate) const MESSAGE_BYTES: &[u8] = b"%<action>s not explicitly defined on the %<type>s.";

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

    let patterns: [&[u8]; 13] = [
        b"after_action",
        b"append_after_action",
        b"append_around_action",
        b"append_before_action",
        b"around_action",
        b"before_action",
        b"prepend_after_action",
        b"prepend_around_action",
        b"prepend_before_action",
        b"skip_after_action",
        b"skip_around_action",
        b"skip_before_action",
        b"skip_action_callback",
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
