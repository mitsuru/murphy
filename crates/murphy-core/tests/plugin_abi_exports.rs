use murphy_core::{
    MURPHY_CALL_ARGUMENT_KIND_OTHER, MURPHY_CALL_ARGUMENT_KIND_STRING,
    MURPHY_CALL_ARGUMENT_KIND_SYMBOL, MURPHY_PLUGIN_ABI_VERSION, MURPHY_SEVERITY_ERROR,
    MURPHY_SEVERITY_UNSET, MURPHY_SEVERITY_WARNING, MURPHY_TRISTATE_FALSE, MURPHY_TRISTATE_TRUE,
    MURPHY_TRISTATE_UNSET, MurphyCallContext, MurphyCallDispatchV1, MurphyCopOptionV1,
    MurphyEmitOffense, MurphyFileContext, MurphyNodeContext, MurphyNodeDispatchV1,
    MurphyPluginCallArgument, MurphyPluginCopV1, MurphyPluginEdit, MurphyPluginOffense,
    MurphyPluginV1, MurphyRange, MurphyRunCallDispatch, MurphyRunFile, MurphyRunNodeDispatch,
    MurphySlice,
};

unsafe extern "C" fn noop_run_file(
    _ctx: *const MurphyFileContext,
    _emit: MurphyEmitOffense,
    _sink: *mut std::ffi::c_void,
) -> i32 {
    0
}

static COP_NAME: &[u8] = b"Plugin/Test";
static OPTION_NAME: &[u8] = b"sample_option";
static OPTION_TY: &[u8] = b"bool";
static OPTIONS: [MurphyCopOptionV1; 1] = [MurphyCopOptionV1 {
    name: MurphySlice {
        ptr: OPTION_NAME.as_ptr(),
        len: OPTION_NAME.len(),
    },
    ty: MurphySlice {
        ptr: OPTION_TY.as_ptr(),
        len: OPTION_TY.len(),
    },
    default_json: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
    description: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
    enum_values_json: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
    replacement: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
    reason: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
}];
static COPS: [MurphyPluginCopV1; 1] = [MurphyPluginCopV1 {
    size: std::mem::size_of::<MurphyPluginCopV1>(),
    name: MurphySlice {
        ptr: COP_NAME.as_ptr(),
        len: COP_NAME.len(),
    },
    run_file: Some(noop_run_file),
    description: MurphySlice {
        ptr: std::ptr::null(),
        len: 0,
    },
    default_severity: MURPHY_SEVERITY_UNSET,
    default_enabled: MURPHY_TRISTATE_UNSET,
    options_ptr: OPTIONS.as_ptr(),
    options_len: OPTIONS.len(),
}];

static CALL_DISPATCH: [MurphyCallDispatchV1; 1] = [MurphyCallDispatchV1 {
    method_name: MurphySlice {
        ptr: b"example_call".as_ptr(),
        len: b"example_call".len(),
    },
    cop_index: 0,
    dispatch_id: 7,
}];

static NODE_DISPATCH: [MurphyNodeDispatchV1; 1] = [MurphyNodeDispatchV1 {
    node_kind: MurphySlice {
        ptr: b"class".as_ptr(),
        len: b"class".len(),
    },
    cop_index: 0,
    dispatch_id: 11,
}];

#[test]
fn native_plugin_abi_types_are_public() {
    assert_eq!(MURPHY_PLUGIN_ABI_VERSION, 1);
    assert_eq!(MURPHY_CALL_ARGUMENT_KIND_OTHER, 0);
    assert_eq!(MURPHY_CALL_ARGUMENT_KIND_STRING, 1);
    assert_eq!(MURPHY_CALL_ARGUMENT_KIND_SYMBOL, 2);
    assert_eq!(MURPHY_SEVERITY_WARNING, 0);
    assert_eq!(MURPHY_SEVERITY_ERROR, 1);
    assert_eq!(MURPHY_SEVERITY_UNSET, 255);
    assert_eq!(MURPHY_TRISTATE_FALSE, 0);
    assert_eq!(MURPHY_TRISTATE_TRUE, 1);
    assert_eq!(MURPHY_TRISTATE_UNSET, 255);
    let _ = std::mem::size_of::<MurphySlice>();
    let _ = std::mem::size_of::<MurphyRange>();
    let _ = std::mem::size_of::<MurphyPluginOffense>();
    let _ = std::mem::size_of::<MurphyFileContext>();
    let _ = std::mem::size_of::<MurphyCallContext>();
    let _ = std::mem::size_of::<MurphyNodeContext>();
    let _ = std::mem::size_of::<MurphyPluginCallArgument>();
    let _ = std::mem::size_of::<MurphyPluginCopV1>();
    let _ = std::mem::size_of::<MurphyCopOptionV1>();
    let _ = std::mem::size_of::<MurphyCallDispatchV1>();
    let _ = std::mem::size_of::<MurphyNodeDispatchV1>();
    let _ = std::mem::size_of::<MurphyPluginV1>();
    let _ = std::mem::size_of::<MurphyPluginEdit>();
    let _: Option<MurphyEmitOffense> = None;
    let _: Option<MurphyRunFile> = None;
    let _: Option<MurphyRunCallDispatch> = None;
    let _: Option<MurphyRunNodeDispatch> = None;
    let argument = MurphyPluginCallArgument {
        kind: MURPHY_CALL_ARGUMENT_KIND_OTHER,
        range: MurphyRange {
            start_offset: 0,
            end_offset: 0,
        },
    };
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
        arguments_ptr: &argument,
        arguments_len: 1,
    };
    let _ = MurphyNodeContext {
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
        node_kind: NODE_DISPATCH[0].node_kind,
        dispatch_id: NODE_DISPATCH[0].dispatch_id,
        range: MurphyRange {
            start_offset: 0,
            end_offset: 0,
        },
    };
}

#[test]
fn plugin_cops_can_be_declared_static() {
    assert_eq!(COPS.len(), 1);
    assert_eq!(COPS[0].default_severity, MURPHY_SEVERITY_UNSET);
    assert_eq!(COPS[0].default_enabled, MURPHY_TRISTATE_UNSET);
    assert_eq!(COPS[0].options_len, OPTIONS.len());
    assert_eq!(COPS[0].options_len, 1);
    let opt = &OPTIONS[0];
    assert_eq!(opt.name.len, OPTION_NAME.len());
    assert_eq!(opt.ty.len, OPTION_TY.len());
}
