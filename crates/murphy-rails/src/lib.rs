mod cop;

use murphy_core::{
    MurphyPluginCopV1, MurphyPluginV1, MURPHY_PLUGIN_ABI_VERSION,
};

const COPS: [MurphyPluginCopV1; 11] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::action_filter::NAME,
        run_file: Some(cop::rails::action_filter::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::has_and_belongs_to_many::NAME,
        run_file: Some(cop::rails::has_and_belongs_to_many::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::find_each::NAME,
        run_file: Some(cop::rails::find_each::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::html_safe::NAME,
        run_file: Some(cop::rails::html_safe::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::date::NAME,
        run_file: Some(cop::rails::date::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::default_scope::NAME,
        run_file: Some(cop::rails::default_scope::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::dynamic_find_by::NAME,
        run_file: Some(cop::rails::dynamic_find_by::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::has_many_or_has_one_dependent::NAME,
        run_file: Some(cop::rails::has_many_or_has_one_dependent::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::request_referer::NAME,
        run_file: Some(cop::rails::request_referer::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::render_text::NAME,
        run_file: Some(cop::rails::render_text::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cop::rails::read_write_attribute::NAME,
        run_file: Some(cop::rails::read_write_attribute::run),
    },
];

#[unsafe(no_mangle)]
pub extern "C" fn murphy_plugin_abi_version() -> u32 {
    MURPHY_PLUGIN_ABI_VERSION
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn murphy_register_plugin(plugin: *mut MurphyPluginV1) -> i32 {
    if plugin.is_null() {
        return -1;
    }

    unsafe {
        *plugin = MurphyPluginV1 {
            size: std::mem::size_of::<MurphyPluginV1>(),
            cops_ptr: COPS.as_ptr(),
            cops_len: COPS.len(),
        };
    }

    0
}
