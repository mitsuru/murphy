use murphy_core::{
    MURPHY_CALL_ARGUMENT_KIND_STRING, MURPHY_CALL_ARGUMENT_KIND_SYMBOL, MurphyCallContext,
    MurphyEmitOffense, MurphyPluginOffense, MurphySlice,
};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/MultipleRoutePaths";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Use separate routes instead of combining multiple route paths in a single route.";

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
    let arguments = if ctx.arguments_len == 0 {
        &[]
    } else if ctx.arguments_ptr.is_null() {
        return 1;
    } else {
        unsafe { std::slice::from_raw_parts(ctx.arguments_ptr, ctx.arguments_len) }
    };

    let route_path_count = arguments
        .iter()
        .filter(|argument| {
            matches!(
                argument.kind,
                MURPHY_CALL_ARGUMENT_KIND_STRING | MURPHY_CALL_ARGUMENT_KIND_SYMBOL
            )
        })
        .count();

    if route_path_count >= 2 {
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
