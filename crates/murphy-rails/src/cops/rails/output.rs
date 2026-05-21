use murphy_core::{
    MurphyCallContext, MurphyEmitOffense, MurphyFileContext, MurphyPluginOffense, MurphySlice,
};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/Output";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Do not write to stdout. Use Rails's logger if you want to log.";

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

    let patterns: [&[u8]; 10] = [
        b"ap",
        b"p",
        b"pp",
        b"pretty_print",
        b"print",
        b"puts",
        b"binwrite",
        b"syswrite",
        b"write",
        b"write_nonblock",
    ];
    for pattern in patterns {
        if util::emit_identifier_match(
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

pub(crate) unsafe extern "C" fn run_call(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }

    let ctx = unsafe { &*ctx };
    let offense = MurphyPluginOffense {
        cop_name: NAME,
        message: util::slice(MESSAGE_BYTES),
        range: ctx.message_range,
        severity: 0,
        autocorrect: std::ptr::null(),
    };
    unsafe { emit(sink, &offense) };

    0
}
