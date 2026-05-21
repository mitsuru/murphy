use murphy_plugin_api::{Cop, CopOptions, MurphyCopOptionV1, MurphySlice};

#[derive(Default)]
struct MaxDepthOptions;

static SCHEMA: [MurphyCopOptionV1; 1] = [MurphyCopOptionV1 {
    name: MurphySlice {
        ptr: b"max_depth".as_ptr(),
        len: 9,
    },
    ty: MurphySlice {
        ptr: b"int".as_ptr(),
        len: 3,
    },
    default_json: MurphySlice {
        ptr: b"3".as_ptr(),
        len: 1,
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

impl CopOptions for MaxDepthOptions {
    const SCHEMA: &'static [MurphyCopOptionV1] = &SCHEMA;
}

struct DeepNest;

impl Cop for DeepNest {
    type Options = MaxDepthOptions;
    const NAME: &'static str = "Plugin/DeepNest";
}

murphy_plugin_macros::register_cops!(DeepNest);

fn main() {}
