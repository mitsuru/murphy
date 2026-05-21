use murphy_core::{MurphyCallContext, MurphyEmitOffense, MurphyPluginOffense, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/RefuteMethods";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Prefer `%<good_method>s` over `%<bad_method>s`.";

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

    if is_refute_method(method_name) {
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

fn is_refute_method(method_name: &[u8]) -> bool {
    matches!(
        method_name,
        b"refute"
            | b"refute_empty"
            | b"refute_equal"
            | b"refute_in_delta"
            | b"refute_in_epsilon"
            | b"refute_includes"
            | b"refute_instance_of"
            | b"refute_kind_of"
            | b"refute_nil"
            | b"refute_operator"
            | b"refute_predicate"
            | b"refute_respond_to"
            | b"refute_same"
            | b"refute_match"
            | b"assert_not"
            | b"assert_not_empty"
            | b"assert_not_equal"
            | b"assert_not_in_delta"
            | b"assert_not_in_epsilon"
            | b"assert_not_includes"
            | b"assert_not_instance_of"
            | b"assert_not_kind_of"
            | b"assert_not_nil"
            | b"assert_not_operator"
            | b"assert_not_predicate"
            | b"assert_not_respond_to"
            | b"assert_not_same"
            | b"assert_no_match"
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
