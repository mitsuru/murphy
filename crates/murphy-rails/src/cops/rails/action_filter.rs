use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/ActionFilter";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Prefer `%<prefer>s` over `%<current>s`.";

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

    let patterns: [&[u8]; 26] = [
        b"after_filter",
        b"append_after_filter",
        b"append_around_filter",
        b"append_before_filter",
        b"around_filter",
        b"before_filter",
        b"prepend_after_filter",
        b"prepend_around_filter",
        b"prepend_before_filter",
        b"skip_after_filter",
        b"skip_around_filter",
        b"skip_before_filter",
        b"skip_filter",
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
