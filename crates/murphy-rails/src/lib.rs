mod cops;

use murphy_core::{MURPHY_PLUGIN_ABI_VERSION, MurphyPluginCopV1, MurphyPluginV1};

const COPS: [MurphyPluginCopV1; 16] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_filter::NAME,
        run_file: Some(cops::rails::action_filter::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_and_belongs_to_many::NAME,
        run_file: Some(cops::rails::has_and_belongs_to_many::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_each::NAME,
        run_file: Some(cops::rails::find_each::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::html_safe::NAME,
        run_file: Some(cops::rails::html_safe::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::output_safety::NAME,
        run_file: Some(cops::rails::output_safety::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::date::NAME,
        run_file: Some(cops::rails::date::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::default_scope::NAME,
        run_file: Some(cops::rails::default_scope::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dynamic_find_by::NAME,
        run_file: Some(cops::rails::dynamic_find_by::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_many_or_has_one_dependent::NAME,
        run_file: Some(cops::rails::has_many_or_has_one_dependent::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::request_referer::NAME,
        run_file: Some(cops::rails::request_referer::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_inline::NAME,
        run_file: Some(cops::rails::render_inline::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_text::NAME,
        run_file: Some(cops::rails::render_text::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_json::NAME,
        run_file: Some(cops::rails::render_json::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::read_write_attribute::NAME,
        run_file: Some(cops::rails::read_write_attribute::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::save_bang::NAME,
        run_file: Some(cops::rails::save_bang::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::link_to_blank::NAME,
        run_file: Some(cops::rails::link_to_blank::run),
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
