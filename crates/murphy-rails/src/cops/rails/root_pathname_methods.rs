use murphy_core::{MurphyEmitOffense, MurphyFileContext, MurphySlice};
use std::ffi::c_void;

use crate::cops::util;

pub(crate) const NAME_BYTES: &[u8] = b"Rails/RootPathnameMethods";
pub(crate) const MESSAGE_BYTES: &[u8] =
    b"`%<rails_root>s` is a `Pathname`, so you can use `%<replacement>s`.";

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

    let patterns: [&[u8]; 68] = [
        b"[]",
        b"glob",
        b"children",
        b"delete",
        b"each_child",
        b"empty?",
        b"entries",
        b"exist?",
        b"mkdir",
        b"open",
        b"rmdir",
        b"unlink",
        b"atime",
        b"basename",
        b"binread",
        b"binwrite",
        b"birthtime",
        b"blockdev?",
        b"chardev?",
        b"chmod",
        b"chown",
        b"ctime",
        b"directory?",
        b"dirname",
        b"executable?",
        b"executable_real?",
        b"expand_path",
        b"extname",
        b"file?",
        b"fnmatch",
        b"fnmatch?",
        b"ftype",
        b"grpowned?",
        b"join",
        b"lchmod",
        b"lchown",
        b"lstat",
        b"mtime",
        b"owned?",
        b"pipe?",
        b"read",
        b"readable?",
        b"readable_real?",
        b"readlines",
        b"readlink",
        b"realdirpath",
        b"realpath",
        b"rename",
        b"setgid?",
        b"setuid?",
        b"size",
        b"size?",
        b"socket?",
        b"split",
        b"stat",
        b"sticky?",
        b"symlink?",
        b"sysopen",
        b"truncate",
        b"utime",
        b"world_readable?",
        b"world_writable?",
        b"writable?",
        b"writable_real?",
        b"write",
        b"zero?",
        b"mkpath",
        b"rmtree",
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
