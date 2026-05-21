use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/RefuteMethods";
pub(crate) const MESSAGE_BYTES: &[u8] = b"Prefer `%<good_method>s` over `%<bad_method>s`.";

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

    let patterns: [&[u8]; 28] = [
        b"refute",
        b"refute_empty",
        b"refute_equal",
        b"refute_in_delta",
        b"refute_in_epsilon",
        b"refute_includes",
        b"refute_instance_of",
        b"refute_kind_of",
        b"refute_nil",
        b"refute_operator",
        b"refute_predicate",
        b"refute_respond_to",
        b"refute_same",
        b"refute_match",
        b"assert_not",
        b"assert_not_empty",
        b"assert_not_equal",
        b"assert_not_in_delta",
        b"assert_not_in_epsilon",
        b"assert_not_includes",
        b"assert_not_instance_of",
        b"assert_not_kind_of",
        b"assert_not_nil",
        b"assert_not_operator",
        b"assert_not_predicate",
        b"assert_not_respond_to",
        b"assert_not_same",
        b"assert_no_match",
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
