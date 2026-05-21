use murphy_core::{MurphyCallContext, MurphyEmitOffense, MurphyPluginOffense, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/HttpPositionalArguments";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Use keyword arguments instead of positional arguments for http call: `%<verb>s`.";

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
    let Some(method_name) = slice_bytes(ctx.name) else {
        return 1;
    };

    if is_http_verb(method_name) {
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

fn is_http_verb(method_name: &[u8]) -> bool {
    matches!(
        method_name,
        b"get" | b"post" | b"put" | b"patch" | b"delete" | b"head"
    )
}

fn slice_bytes(slice: murphy_core::MurphySlice) -> Option<&'static [u8]> {
    if slice.len == 0 {
        return Some(&[]);
    }
    if slice.ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) })
}
