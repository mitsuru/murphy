use murphy_core::{MurphyCallContext, MurphyEmitOffense, MurphyPluginOffense, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/Validation";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Prefer the new style validations `%<prefer>s` over `%<current>s`.";

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
