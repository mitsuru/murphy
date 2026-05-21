mod cops;

use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense,
    MurphyPluginCopV1, MurphyPluginV1, MurphySlice,
};
use std::ffi::c_void;

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

static CALL_DISPATCH: [MurphyCallDispatchV1; 10] = [
    output_dispatch(b"ap", OUTPUT_DISPATCH_ID),
    output_dispatch(b"p", OUTPUT_DISPATCH_ID),
    output_dispatch(b"pp", OUTPUT_DISPATCH_ID),
    output_dispatch(b"pretty_print", OUTPUT_DISPATCH_ID),
    output_dispatch(b"print", OUTPUT_DISPATCH_ID),
    output_dispatch(b"puts", OUTPUT_DISPATCH_ID),
    output_dispatch(b"binwrite", OUTPUT_DISPATCH_ID),
    output_dispatch(b"syswrite", OUTPUT_DISPATCH_ID),
    output_dispatch(b"write", OUTPUT_DISPATCH_ID),
    output_dispatch(b"write_nonblock", OUTPUT_DISPATCH_ID),
];

const OUTPUT_DISPATCH_ID: usize = 1;

const fn output_dispatch(method_name: &'static [u8], dispatch_id: usize) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        dispatch_id,
    }
}

unsafe extern "C" fn run_call_dispatch(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx_ref = unsafe { &*ctx };
    match ctx_ref.dispatch_id {
        OUTPUT_DISPATCH_ID => unsafe { cops::rails::output::run_call(ctx, emit, sink) },
        _ => 0,
    }
}

const COPS: [MurphyPluginCopV1; 138] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_controller_flash_before_render::NAME,
        run_file: Some(cops::rails::action_controller_flash_before_render::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_controller_test_case::NAME,
        run_file: Some(cops::rails::action_controller_test_case::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_filter::NAME,
        run_file: Some(cops::rails::action_filter::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_order::NAME,
        run_file: Some(cops::rails::action_order::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_aliases::NAME,
        run_file: Some(cops::rails::active_record_aliases::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_callbacks_order::NAME,
        run_file: Some(cops::rails::active_record_callbacks_order::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_override::NAME,
        run_file: Some(cops::rails::active_record_override::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_support_aliases::NAME,
        run_file: Some(cops::rails::active_support_aliases::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_support_on_load::NAME,
        run_file: Some(cops::rails::active_support_on_load::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::add_column_index::NAME,
        run_file: Some(cops::rails::add_column_index::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::after_commit_override::NAME,
        run_file: Some(cops::rails::after_commit_override::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_controller::NAME,
        run_file: Some(cops::rails::application_controller::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_job::NAME,
        run_file: Some(cops::rails::application_job::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_mailer::NAME,
        run_file: Some(cops::rails::application_mailer::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_record::NAME,
        run_file: Some(cops::rails::application_record::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::arel_star::NAME,
        run_file: Some(cops::rails::arel_star::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::assert_not::NAME,
        run_file: Some(cops::rails::assert_not::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::attribute_default_block_value::NAME,
        run_file: Some(cops::rails::attribute_default_block_value::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::belongs_to::NAME,
        run_file: Some(cops::rails::belongs_to::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::blank::NAME,
        run_file: Some(cops::rails::blank::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::bulk_change_table::NAME,
        run_file: Some(cops::rails::bulk_change_table::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::compact_blank::NAME,
        run_file: Some(cops::rails::compact_blank::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::content_tag::NAME,
        run_file: Some(cops::rails::content_tag::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::create_table_with_timestamps::NAME,
        run_file: Some(cops::rails::create_table_with_timestamps::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dangerous_column_names::NAME,
        run_file: Some(cops::rails::dangerous_column_names::run),
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
        name: cops::rails::delegate::NAME,
        run_file: Some(cops::rails::delegate::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::delegate_allow_blank::NAME,
        run_file: Some(cops::rails::delegate_allow_blank::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::deprecated_active_model_errors_methods::NAME,
        run_file: Some(cops::rails::deprecated_active_model_errors_methods::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dot_separated_keys::NAME,
        run_file: Some(cops::rails::dot_separated_keys::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duplicate_association::NAME,
        run_file: Some(cops::rails::duplicate_association::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duplicate_scope::NAME,
        run_file: Some(cops::rails::duplicate_scope::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duration_arithmetic::NAME,
        run_file: Some(cops::rails::duration_arithmetic::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dynamic_find_by::NAME,
        run_file: Some(cops::rails::dynamic_find_by::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::eager_evaluation_log_message::NAME,
        run_file: Some(cops::rails::eager_evaluation_log_message::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_hash::NAME,
        run_file: Some(cops::rails::enum_hash::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_syntax::NAME,
        run_file: Some(cops::rails::enum_syntax::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_uniqueness::NAME,
        run_file: Some(cops::rails::enum_uniqueness::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::env::NAME,
        run_file: Some(cops::rails::env::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::env_local::NAME,
        run_file: Some(cops::rails::env_local::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::environment_comparison::NAME,
        run_file: Some(cops::rails::environment_comparison::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::environment_variable_access::NAME,
        run_file: Some(cops::rails::environment_variable_access::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::exit::NAME,
        run_file: Some(cops::rails::exit::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::expanded_date_range::NAME,
        run_file: Some(cops::rails::expanded_date_range::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::file_path::NAME,
        run_file: Some(cops::rails::file_path::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by::NAME,
        run_file: Some(cops::rails::find_by::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by_id::NAME,
        run_file: Some(cops::rails::find_by_id::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by_or_assignment_memoization::NAME,
        run_file: Some(cops::rails::find_by_or_assignment_memoization::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_each::NAME,
        run_file: Some(cops::rails::find_each::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::freeze_time::NAME,
        run_file: Some(cops::rails::freeze_time::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_and_belongs_to_many::NAME,
        run_file: Some(cops::rails::has_and_belongs_to_many::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_many_or_has_one_dependent::NAME,
        run_file: Some(cops::rails::has_many_or_has_one_dependent::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::helper_instance_variable::NAME,
        run_file: Some(cops::rails::helper_instance_variable::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_positional_arguments::NAME,
        run_file: Some(cops::rails::http_positional_arguments::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_status::NAME,
        run_file: Some(cops::rails::http_status::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_status_name_consistency::NAME,
        run_file: Some(cops::rails::http_status_name_consistency::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_lazy_lookup::NAME,
        run_file: Some(cops::rails::i18n_lazy_lookup::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_locale_assignment::NAME,
        run_file: Some(cops::rails::i18n_locale_assignment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_locale_texts::NAME,
        run_file: Some(cops::rails::i18n_locale_texts::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::ignored_columns_assignment::NAME,
        run_file: Some(cops::rails::ignored_columns_assignment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::ignored_skip_action_filter_option::NAME,
        run_file: Some(cops::rails::ignored_skip_action_filter_option::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::index_by::NAME,
        run_file: Some(cops::rails::index_by::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::index_with::NAME,
        run_file: Some(cops::rails::index_with::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::inquiry::NAME,
        run_file: Some(cops::rails::inquiry::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::inverse_of::NAME,
        run_file: Some(cops::rails::inverse_of::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::lexically_scoped_action_filter::NAME,
        run_file: Some(cops::rails::lexically_scoped_action_filter::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::link_to_blank::NAME,
        run_file: Some(cops::rails::link_to_blank::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::mailer_name::NAME,
        run_file: Some(cops::rails::mailer_name::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::match_route::NAME,
        run_file: Some(cops::rails::match_route::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::migration_class_name::NAME,
        run_file: Some(cops::rails::migration_class_name::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::multiple_route_paths::NAME,
        run_file: Some(cops::rails::multiple_route_paths::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::negate_include::NAME,
        run_file: Some(cops::rails::negate_include::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::not_null_column::NAME,
        run_file: Some(cops::rails::not_null_column::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::order_arguments::NAME,
        run_file: Some(cops::rails::order_arguments::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::order_by_id::NAME,
        run_file: Some(cops::rails::order_by_id::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::output::NAME,
        run_file: Some(cops::rails::output::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::output_safety::NAME,
        run_file: Some(cops::rails::output_safety::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pick::NAME,
        run_file: Some(cops::rails::pick::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck::NAME,
        run_file: Some(cops::rails::pluck::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck_id::NAME,
        run_file: Some(cops::rails::pluck_id::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck_in_where::NAME,
        run_file: Some(cops::rails::pluck_in_where::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluralization_grammar::NAME,
        run_file: Some(cops::rails::pluralization_grammar::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::presence::NAME,
        run_file: Some(cops::rails::presence::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::present::NAME,
        run_file: Some(cops::rails::present::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::rake_environment::NAME,
        run_file: Some(cops::rails::rake_environment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::read_write_attribute::NAME,
        run_file: Some(cops::rails::read_write_attribute::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redirect_back_or_to::NAME,
        run_file: Some(cops::rails::redirect_back_or_to::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_active_record_all_method::NAME,
        run_file: Some(cops::rails::redundant_active_record_all_method::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_allow_nil::NAME,
        run_file: Some(cops::rails::redundant_allow_nil::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_foreign_key::NAME,
        run_file: Some(cops::rails::redundant_foreign_key::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_presence_validation_on_belongs_to::NAME,
        run_file: Some(cops::rails::redundant_presence_validation_on_belongs_to::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_receiver_in_with_options::NAME,
        run_file: Some(cops::rails::redundant_receiver_in_with_options::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_travel_back::NAME,
        run_file: Some(cops::rails::redundant_travel_back::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reflection_class_name::NAME,
        run_file: Some(cops::rails::reflection_class_name::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::refute_methods::NAME,
        run_file: Some(cops::rails::refute_methods::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::relative_date_constant::NAME,
        run_file: Some(cops::rails::relative_date_constant::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_inline::NAME,
        run_file: Some(cops::rails::render_inline::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_plain_text::NAME,
        run_file: Some(cops::rails::render_plain_text::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::request_referer::NAME,
        run_file: Some(cops::rails::request_referer::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::require_dependency::NAME,
        run_file: Some(cops::rails::require_dependency::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::response_parsed_body::NAME,
        run_file: Some(cops::rails::response_parsed_body::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reversible_migration::NAME,
        run_file: Some(cops::rails::reversible_migration::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reversible_migration_method_definition::NAME,
        run_file: Some(cops::rails::reversible_migration_method_definition::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_join_chain::NAME,
        run_file: Some(cops::rails::root_join_chain::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_pathname_methods::NAME,
        run_file: Some(cops::rails::root_pathname_methods::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_public_path::NAME,
        run_file: Some(cops::rails::root_public_path::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::safe_navigation::NAME,
        run_file: Some(cops::rails::safe_navigation::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::safe_navigation_with_blank::NAME,
        run_file: Some(cops::rails::safe_navigation_with_blank::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::save_bang::NAME,
        run_file: Some(cops::rails::save_bang::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::schema_comment::NAME,
        run_file: Some(cops::rails::schema_comment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::scope_args::NAME,
        run_file: Some(cops::rails::scope_args::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::select_map::NAME,
        run_file: Some(cops::rails::select_map::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::short_i18n::NAME,
        run_file: Some(cops::rails::short_i18n::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::skips_model_validations::NAME,
        run_file: Some(cops::rails::skips_model_validations::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::squished_sql_heredocs::NAME,
        run_file: Some(cops::rails::squished_sql_heredocs::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::strip_heredoc::NAME,
        run_file: Some(cops::rails::strip_heredoc::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::strong_parameters_expect::NAME,
        run_file: Some(cops::rails::strong_parameters_expect::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::table_name_assignment::NAME,
        run_file: Some(cops::rails::table_name_assignment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::three_state_boolean_column::NAME,
        run_file: Some(cops::rails::three_state_boolean_column::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::time_zone::NAME,
        run_file: Some(cops::rails::time_zone::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::time_zone_assignment::NAME,
        run_file: Some(cops::rails::time_zone_assignment::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::to_formatted_s::NAME,
        run_file: Some(cops::rails::to_formatted_s::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::to_s_with_argument::NAME,
        run_file: Some(cops::rails::to_s_with_argument::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::top_level_hash_with_indifferent_access::NAME,
        run_file: Some(cops::rails::top_level_hash_with_indifferent_access::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::transaction_exit_statement::NAME,
        run_file: Some(cops::rails::transaction_exit_statement::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::uniq_before_pluck::NAME,
        run_file: Some(cops::rails::uniq_before_pluck::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unique_validation_without_index::NAME,
        run_file: Some(cops::rails::unique_validation_without_index::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unknown_env::NAME,
        run_file: Some(cops::rails::unknown_env::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unused_ignored_columns::NAME,
        run_file: Some(cops::rails::unused_ignored_columns::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unused_render_content::NAME,
        run_file: Some(cops::rails::unused_render_content::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::validation::NAME,
        run_file: Some(cops::rails::validation::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_equals::NAME,
        run_file: Some(cops::rails::where_equals::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_exists::NAME,
        run_file: Some(cops::rails::where_exists::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_missing::NAME,
        run_file: Some(cops::rails::where_missing::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_not::NAME,
        run_file: Some(cops::rails::where_not::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_not_with_multiple_conditions::NAME,
        run_file: Some(cops::rails::where_not_with_multiple_conditions::run),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_range::NAME,
        run_file: Some(cops::rails::where_range::run),
    },
];

#[unsafe(no_mangle)]
pub extern "C" fn murphy_plugin_abi_version() -> u32 {
    MURPHY_PLUGIN_ABI_VERSION
}

/// Register the Rails plugin's static ABI tables.
///
/// # Safety
///
/// `plugin` must be either null or a valid, writable pointer to a
/// `MurphyPluginV1` owned by the Murphy host for the duration of this call.
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
            call_dispatch_ptr: CALL_DISPATCH.as_ptr(),
            call_dispatch_len: CALL_DISPATCH.len(),
            run_call_dispatch: Some(run_call_dispatch),
        };
    }

    0
}
