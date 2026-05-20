use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

const NAME_BYTES: &[u8] = b"Rails/HasAndBelongsToMany";
const MESSAGE_BYTES: &[u8] = b"prefer has_many :through instead";

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
    util::emit_match_simple(
        source,
        b"has_and_belongs_to_many",
        NAME,
        util::slice(MESSAGE_BYTES),
        emit,
        sink,
    )
}
