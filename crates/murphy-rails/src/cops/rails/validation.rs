use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/Validation";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"Prefer the new style validations `%<prefer>s` over `%<current>s`.";

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

    let patterns: [&[u8]; 12] = [
        b"validates_acceptance_of",
        b"validates_comparison_of",
        b"validates_confirmation_of",
        b"validates_exclusion_of",
        b"validates_format_of",
        b"validates_inclusion_of",
        b"validates_length_of",
        b"validates_numericality_of",
        b"validates_presence_of",
        b"validates_absence_of",
        b"validates_size_of",
        b"validates_uniqueness_of",
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
