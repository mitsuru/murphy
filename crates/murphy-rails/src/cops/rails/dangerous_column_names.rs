use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/DangerousColumnNames";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Avoid dangerous column names.";

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
        b"add_column",
        b"rename",
        b"rename_column",
        b"bigint",
        b"binary",
        b"blob",
        b"boolean",
        b"date",
        b"datetime",
        b"decimal",
        b"float",
        b"integer",
        b"numeric",
        b"primary_key",
        b"string",
        b"text",
        b"time",
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
