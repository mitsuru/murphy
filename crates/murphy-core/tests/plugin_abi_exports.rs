use murphy_core::{
    MurphyEmitOffense, MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense, MurphyPluginV1,
    MurphyRange, MurphyRunFile, MurphySlice,
};

#[test]
fn native_plugin_abi_types_are_public() {
    let _ = std::mem::size_of::<MurphySlice>();
    let _ = std::mem::size_of::<MurphyRange>();
    let _ = std::mem::size_of::<MurphyPluginOffense>();
    let _ = std::mem::size_of::<MurphyFileContext>();
    let _ = std::mem::size_of::<MurphyPluginCopV1>();
    let _ = std::mem::size_of::<MurphyPluginV1>();
    let _: Option<MurphyEmitOffense> = None;
    let _: Option<MurphyRunFile> = None;
}
