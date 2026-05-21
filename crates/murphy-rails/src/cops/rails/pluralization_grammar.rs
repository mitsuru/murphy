use murphy_core::{
    MURPHY_CALL_RECEIVER_FLOAT, MURPHY_CALL_RECEIVER_INTEGER, MurphyCallContext, MurphyEmitOffense,
    MurphyPluginOffense, MurphyRange, MurphySlice,
};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/PluralizationGrammar";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Prefer `%<number>s.%<correct>s`.";

pub(crate) const NAME: MurphySlice = util::slice(NAME_BYTES);

pub(crate) unsafe extern "C" fn run_call(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*ctx };
    if ctx.receiver_kind != MURPHY_CALL_RECEIVER_INTEGER
        && ctx.receiver_kind != MURPHY_CALL_RECEIVER_FLOAT
    {
        return 0;
    }

    let Some(method_name) = slice_bytes(ctx.name) else {
        return 1;
    };
    let Some(source) = slice_bytes(ctx.source) else {
        return 1;
    };
    let Some(receiver_source) = range_slice(source, ctx.receiver_range) else {
        return 1;
    };
    let Some(number) = parse_numeric_literal(receiver_source) else {
        return 0;
    };

    let singular_receiver = number.abs() == 1.0;
    let plural_method = is_plural_method(method_name);
    if (singular_receiver && plural_method) || (!singular_receiver && !plural_method) {
        let offense = MurphyPluginOffense {
            cop_name: NAME,
            message: util::slice(MESSAGE_BYTES),
            range: ctx.message_range,
            severity: 0,
            autocorrect: std::ptr::null(),
        };
        unsafe { emit(sink, &offense) };
    }

    0
}

fn is_plural_method(method_name: &[u8]) -> bool {
    matches!(
        method_name,
        b"seconds"
            | b"minutes"
            | b"hours"
            | b"days"
            | b"weeks"
            | b"fortnights"
            | b"months"
            | b"years"
            | b"bytes"
            | b"kilobytes"
            | b"megabytes"
            | b"gigabytes"
            | b"terabytes"
            | b"petabytes"
            | b"exabytes"
            | b"zettabytes"
    )
}

fn slice_bytes(slice: MurphySlice) -> Option<&'static [u8]> {
    if slice.len == 0 {
        return Some(&[]);
    }
    if slice.ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) })
}

fn range_slice(source: &[u8], range: MurphyRange) -> Option<&[u8]> {
    source.get(range.start_offset as usize..range.end_offset as usize)
}

fn parse_numeric_literal(source: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(source).ok()?.replace('_', "");
    text.parse::<f64>().ok()
}
