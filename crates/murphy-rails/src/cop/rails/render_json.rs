use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/RenderJson";
pub(crate) const MESSAGE_BYTES: &[u8] = b"prefer template or object rendering style";
pub(crate) const REPLACE_JSON_SPACE: &[u8] = b"render json:";
pub(crate) const REPLACE_JSON_PAREN: &[u8] = b"render(json:";

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

    let replacements: [(&[u8], &[u8]); 2] = [
        (b"render :json =>", REPLACE_JSON_SPACE),
        (b"render(:json", REPLACE_JSON_PAREN),
    ];

    for (pattern, replacement) in replacements {
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
