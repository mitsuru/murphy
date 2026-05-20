use murphy_core::{
    MurphyEmitOffense, MurphyFileContext, MurphyPluginCopV1, MurphyPluginEdit,
    MurphyPluginOffense, MurphyPluginV1, MurphyRange, MurphyRunFile, MurphySlice,
};

unsafe extern "C" fn noop_run_file(
    _ctx: *const MurphyFileContext,
    _emit: MurphyEmitOffense,
    _sink: *mut std::ffi::c_void,
) -> i32 {
    0
}

static COP_NAME: &[u8] = b"Plugin/Test";
static COPS: [MurphyPluginCopV1; 1] = [MurphyPluginCopV1 {
    size: std::mem::size_of::<MurphyPluginCopV1>(),
    name: MurphySlice {
        ptr: COP_NAME.as_ptr(),
        len: COP_NAME.len(),
    },
    run_file: Some(noop_run_file),
}];

#[test]
fn native_plugin_abi_types_are_public() {
    let _ = std::mem::size_of::<MurphySlice>();
    let _ = std::mem::size_of::<MurphyRange>();
    let _ = std::mem::size_of::<MurphyPluginOffense>();
    let _ = std::mem::size_of::<MurphyFileContext>();
    let _ = std::mem::size_of::<MurphyPluginCopV1>();
    let _ = std::mem::size_of::<MurphyPluginV1>();
    let _ = std::mem::size_of::<MurphyPluginEdit>();
    let _: Option<MurphyEmitOffense> = None;
    let _: Option<MurphyRunFile> = None;
}

#[test]
fn plugin_cops_can_be_declared_static() {
    assert_eq!(COPS.len(), 1);
}
