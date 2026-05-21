use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/PluralizationGrammar";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Prefer `%<number>s.%<correct>s`.";

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

    let patterns: [&[u8]; 32] = [
        b"second",
        b"minute",
        b"hour",
        b"day",
        b"week",
        b"fortnight",
        b"month",
        b"year",
        b"byte",
        b"kilobyte",
        b"megabyte",
        b"gigabyte",
        b"terabyte",
        b"petabyte",
        b"exabyte",
        b"zettabyte",
        b"seconds",
        b"minutes",
        b"hours",
        b"days",
        b"weeks",
        b"fortnights",
        b"months",
        b"years",
        b"bytes",
        b"kilobytes",
        b"megabytes",
        b"gigabytes",
        b"terabytes",
        b"petabytes",
        b"exabytes",
        b"zettabytes",
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
