use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/RenderText";
pub(crate) const MESSAGE_BYTES: &[u8] = b"use modern render template options instead of render text";
pub(crate) const REPLACE_TEXT_SPACE: &[u8] = b"render plain:";
pub(crate) const REPLACE_TEXT_PAREN: &[u8] = b"render(plain:";
pub(crate) const FIND_TEXT_SPACE: &[u8] = b"render :text =>";
pub(crate) const FIND_TEXT_PAREN: &[u8] = b"render(:text =>";

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

    let patterns: [(&[u8], &[u8]); 2] = [
        (FIND_TEXT_SPACE, REPLACE_TEXT_SPACE),
        (FIND_TEXT_PAREN, REPLACE_TEXT_PAREN),
    ];

    for (pattern, replacement) in patterns {
        if util::emit_match_with_replacement(
            source,
            pattern,
            NAME,
            util::slice(MESSAGE_BYTES),
            None,
            replacement,
            emit,
            sink,
        ) != 0
        {
            return 1;
        }
    }

    0
}
