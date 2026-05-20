use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cop::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/LinkToBlank";
pub(crate) const MESSAGE_BYTES: &[u8] = b"add rel=\"noopener\" when opening new windows";
pub(crate) const REPLACE_HASHROCKET_TARGET: &[u8] = b":target => \"_blank\", :rel => \"noopener\"";
pub(crate) const REPLACE_TARGET: &[u8] = b"target: \"_blank\", rel: \"noopener\"";

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

    let matchers: [(&[u8], Option<&'static [u8]>, &[u8]); 2] = [
        (
            b":target => \"_blank\"",
            Some(b":rel =>".as_ref()),
            REPLACE_HASHROCKET_TARGET,
        ),
        (b"target: \"_blank\"", Some(b"rel:".as_ref()), REPLACE_TARGET),
    ];

    for (pattern, rel_pattern, replacement) in matchers {
        if util::emit_match_with_replacement(
            source,
            pattern,
            NAME,
            util::slice(MESSAGE_BYTES),
            rel_pattern,
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
