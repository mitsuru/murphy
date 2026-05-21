use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/HttpPositionalArguments";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Use keyword arguments instead of positional arguments for http call: `%<verb>s`.";

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
        b"get '",
        b"get \"",
        b"post '",
        b"post \"",
        b"put '",
        b"put \"",
        b"patch '",
        b"patch \"",
        b"delete '",
        b"delete \"",
        b"head '",
        b"head \"",
        b"http_positional_arguments",
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
