use murphy_core::{
    MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense, MurphyFileContext,
    MurphyPluginCopV1, MurphyPluginEdit, MurphyPluginOffense, MurphyPluginV1, MurphyRange,
    MurphyRunCallDispatch, MurphyRunFile, MurphySlice,
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

static CALL_DISPATCH: [MurphyCallDispatchV1; 1] = [MurphyCallDispatchV1 {
    method_name: MurphySlice {
        ptr: b"example_call".as_ptr(),
        len: b"example_call".len(),
    },
    cop_index: 0,
    dispatch_id: 7,
}];

#[test]
fn native_plugin_abi_types_are_public() {
    let _ = std::mem::size_of::<MurphySlice>();
    let _ = std::mem::size_of::<MurphyRange>();
    let _ = std::mem::size_of::<MurphyPluginOffense>();
    let _ = std::mem::size_of::<MurphyFileContext>();
    let _ = std::mem::size_of::<MurphyCallContext>();
    let _ = std::mem::size_of::<MurphyPluginCopV1>();
    let _ = std::mem::size_of::<MurphyCallDispatchV1>();
    let _ = std::mem::size_of::<MurphyPluginV1>();
    let _ = std::mem::size_of::<MurphyPluginEdit>();
    let _: Option<MurphyEmitOffense> = None;
    let _: Option<MurphyRunFile> = None;
    let _: Option<MurphyRunCallDispatch> = None;
    let _ = MurphyCallContext {
        file: MurphySlice {
            ptr: std::ptr::null(),
            len: 0,
        },
        source: MurphySlice {
            ptr: std::ptr::null(),
            len: 0,
        },
        config: MurphySlice {
            ptr: std::ptr::null(),
            len: 0,
        },
        name: MurphySlice {
            ptr: std::ptr::null(),
            len: 0,
        },
        dispatch_id: CALL_DISPATCH[0].dispatch_id,
        message_range: MurphyRange {
            start_offset: 0,
            end_offset: 0,
        },
        receiver_kind: murphy_core::MURPHY_CALL_RECEIVER_NONE,
        receiver_range: MurphyRange {
            start_offset: 0,
            end_offset: 0,
        },
    };
}

#[test]
fn plugin_cops_can_be_declared_static() {
    assert_eq!(COPS.len(), 1);
}
