mod cops;

use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallDispatchV1, MurphyPluginCopV1, MurphyPluginV1, MurphySlice,
};

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

const OUTPUT_COP_INDEX: usize = 76;
static OUTPUT_CALL_COPS: [usize; 1] = [OUTPUT_COP_INDEX];
static CALL_DISPATCH: [MurphyCallDispatchV1; 10] = [
    output_dispatch(b"ap"),
    output_dispatch(b"p"),
    output_dispatch(b"pp"),
    output_dispatch(b"pretty_print"),
    output_dispatch(b"print"),
    output_dispatch(b"puts"),
    output_dispatch(b"binwrite"),
    output_dispatch(b"syswrite"),
    output_dispatch(b"write"),
    output_dispatch(b"write_nonblock"),
];

const fn output_dispatch(method_name: &'static [u8]) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_indices_ptr: OUTPUT_CALL_COPS.as_ptr(),
        cop_indices_len: OUTPUT_CALL_COPS.len(),
    }
}

const COPS: [MurphyPluginCopV1; 138] = [
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_controller_flash_before_render::NAME,
        run_file: Some(cops::rails::action_controller_flash_before_render::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_controller_test_case::NAME,
        run_file: Some(cops::rails::action_controller_test_case::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_filter::NAME,
        run_file: Some(cops::rails::action_filter::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::action_order::NAME,
        run_file: Some(cops::rails::action_order::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_aliases::NAME,
        run_file: Some(cops::rails::active_record_aliases::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_callbacks_order::NAME,
        run_file: Some(cops::rails::active_record_callbacks_order::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_record_override::NAME,
        run_file: Some(cops::rails::active_record_override::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_support_aliases::NAME,
        run_file: Some(cops::rails::active_support_aliases::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::active_support_on_load::NAME,
        run_file: Some(cops::rails::active_support_on_load::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::add_column_index::NAME,
        run_file: Some(cops::rails::add_column_index::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::after_commit_override::NAME,
        run_file: Some(cops::rails::after_commit_override::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_controller::NAME,
        run_file: Some(cops::rails::application_controller::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_job::NAME,
        run_file: Some(cops::rails::application_job::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_mailer::NAME,
        run_file: Some(cops::rails::application_mailer::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::application_record::NAME,
        run_file: Some(cops::rails::application_record::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::arel_star::NAME,
        run_file: Some(cops::rails::arel_star::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::assert_not::NAME,
        run_file: Some(cops::rails::assert_not::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::attribute_default_block_value::NAME,
        run_file: Some(cops::rails::attribute_default_block_value::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::belongs_to::NAME,
        run_file: Some(cops::rails::belongs_to::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::blank::NAME,
        run_file: Some(cops::rails::blank::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::bulk_change_table::NAME,
        run_file: Some(cops::rails::bulk_change_table::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::compact_blank::NAME,
        run_file: Some(cops::rails::compact_blank::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::content_tag::NAME,
        run_file: Some(cops::rails::content_tag::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::create_table_with_timestamps::NAME,
        run_file: Some(cops::rails::create_table_with_timestamps::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dangerous_column_names::NAME,
        run_file: Some(cops::rails::dangerous_column_names::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::date::NAME,
        run_file: Some(cops::rails::date::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::default_scope::NAME,
        run_file: Some(cops::rails::default_scope::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::delegate::NAME,
        run_file: Some(cops::rails::delegate::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::delegate_allow_blank::NAME,
        run_file: Some(cops::rails::delegate_allow_blank::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::deprecated_active_model_errors_methods::NAME,
        run_file: Some(cops::rails::deprecated_active_model_errors_methods::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dot_separated_keys::NAME,
        run_file: Some(cops::rails::dot_separated_keys::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duplicate_association::NAME,
        run_file: Some(cops::rails::duplicate_association::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duplicate_scope::NAME,
        run_file: Some(cops::rails::duplicate_scope::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::duration_arithmetic::NAME,
        run_file: Some(cops::rails::duration_arithmetic::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::dynamic_find_by::NAME,
        run_file: Some(cops::rails::dynamic_find_by::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::eager_evaluation_log_message::NAME,
        run_file: Some(cops::rails::eager_evaluation_log_message::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_hash::NAME,
        run_file: Some(cops::rails::enum_hash::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_syntax::NAME,
        run_file: Some(cops::rails::enum_syntax::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::enum_uniqueness::NAME,
        run_file: Some(cops::rails::enum_uniqueness::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::env::NAME,
        run_file: Some(cops::rails::env::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::env_local::NAME,
        run_file: Some(cops::rails::env_local::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::environment_comparison::NAME,
        run_file: Some(cops::rails::environment_comparison::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::environment_variable_access::NAME,
        run_file: Some(cops::rails::environment_variable_access::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::exit::NAME,
        run_file: Some(cops::rails::exit::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::expanded_date_range::NAME,
        run_file: Some(cops::rails::expanded_date_range::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::file_path::NAME,
        run_file: Some(cops::rails::file_path::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by::NAME,
        run_file: Some(cops::rails::find_by::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by_id::NAME,
        run_file: Some(cops::rails::find_by_id::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_by_or_assignment_memoization::NAME,
        run_file: Some(cops::rails::find_by_or_assignment_memoization::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::find_each::NAME,
        run_file: Some(cops::rails::find_each::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::freeze_time::NAME,
        run_file: Some(cops::rails::freeze_time::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_and_belongs_to_many::NAME,
        run_file: Some(cops::rails::has_and_belongs_to_many::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::has_many_or_has_one_dependent::NAME,
        run_file: Some(cops::rails::has_many_or_has_one_dependent::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::helper_instance_variable::NAME,
        run_file: Some(cops::rails::helper_instance_variable::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_positional_arguments::NAME,
        run_file: Some(cops::rails::http_positional_arguments::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_status::NAME,
        run_file: Some(cops::rails::http_status::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::http_status_name_consistency::NAME,
        run_file: Some(cops::rails::http_status_name_consistency::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_lazy_lookup::NAME,
        run_file: Some(cops::rails::i18n_lazy_lookup::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_locale_assignment::NAME,
        run_file: Some(cops::rails::i18n_locale_assignment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::i18n_locale_texts::NAME,
        run_file: Some(cops::rails::i18n_locale_texts::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::ignored_columns_assignment::NAME,
        run_file: Some(cops::rails::ignored_columns_assignment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::ignored_skip_action_filter_option::NAME,
        run_file: Some(cops::rails::ignored_skip_action_filter_option::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::index_by::NAME,
        run_file: Some(cops::rails::index_by::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::index_with::NAME,
        run_file: Some(cops::rails::index_with::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::inquiry::NAME,
        run_file: Some(cops::rails::inquiry::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::inverse_of::NAME,
        run_file: Some(cops::rails::inverse_of::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::lexically_scoped_action_filter::NAME,
        run_file: Some(cops::rails::lexically_scoped_action_filter::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::link_to_blank::NAME,
        run_file: Some(cops::rails::link_to_blank::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::mailer_name::NAME,
        run_file: Some(cops::rails::mailer_name::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::match_route::NAME,
        run_file: Some(cops::rails::match_route::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::migration_class_name::NAME,
        run_file: Some(cops::rails::migration_class_name::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::multiple_route_paths::NAME,
        run_file: Some(cops::rails::multiple_route_paths::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::negate_include::NAME,
        run_file: Some(cops::rails::negate_include::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::not_null_column::NAME,
        run_file: Some(cops::rails::not_null_column::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::order_arguments::NAME,
        run_file: Some(cops::rails::order_arguments::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::order_by_id::NAME,
        run_file: Some(cops::rails::order_by_id::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::output::NAME,
        run_file: Some(cops::rails::output::run),
        run_call: Some(cops::rails::output::run_call),
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::output_safety::NAME,
        run_file: Some(cops::rails::output_safety::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pick::NAME,
        run_file: Some(cops::rails::pick::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck::NAME,
        run_file: Some(cops::rails::pluck::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck_id::NAME,
        run_file: Some(cops::rails::pluck_id::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluck_in_where::NAME,
        run_file: Some(cops::rails::pluck_in_where::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::pluralization_grammar::NAME,
        run_file: Some(cops::rails::pluralization_grammar::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::presence::NAME,
        run_file: Some(cops::rails::presence::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::present::NAME,
        run_file: Some(cops::rails::present::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::rake_environment::NAME,
        run_file: Some(cops::rails::rake_environment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::read_write_attribute::NAME,
        run_file: Some(cops::rails::read_write_attribute::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redirect_back_or_to::NAME,
        run_file: Some(cops::rails::redirect_back_or_to::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_active_record_all_method::NAME,
        run_file: Some(cops::rails::redundant_active_record_all_method::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_allow_nil::NAME,
        run_file: Some(cops::rails::redundant_allow_nil::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_foreign_key::NAME,
        run_file: Some(cops::rails::redundant_foreign_key::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_presence_validation_on_belongs_to::NAME,
        run_file: Some(cops::rails::redundant_presence_validation_on_belongs_to::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_receiver_in_with_options::NAME,
        run_file: Some(cops::rails::redundant_receiver_in_with_options::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::redundant_travel_back::NAME,
        run_file: Some(cops::rails::redundant_travel_back::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reflection_class_name::NAME,
        run_file: Some(cops::rails::reflection_class_name::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::refute_methods::NAME,
        run_file: Some(cops::rails::refute_methods::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::relative_date_constant::NAME,
        run_file: Some(cops::rails::relative_date_constant::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_inline::NAME,
        run_file: Some(cops::rails::render_inline::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::render_plain_text::NAME,
        run_file: Some(cops::rails::render_plain_text::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::request_referer::NAME,
        run_file: Some(cops::rails::request_referer::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::require_dependency::NAME,
        run_file: Some(cops::rails::require_dependency::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::response_parsed_body::NAME,
        run_file: Some(cops::rails::response_parsed_body::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reversible_migration::NAME,
        run_file: Some(cops::rails::reversible_migration::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::reversible_migration_method_definition::NAME,
        run_file: Some(cops::rails::reversible_migration_method_definition::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_join_chain::NAME,
        run_file: Some(cops::rails::root_join_chain::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_pathname_methods::NAME,
        run_file: Some(cops::rails::root_pathname_methods::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::root_public_path::NAME,
        run_file: Some(cops::rails::root_public_path::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::safe_navigation::NAME,
        run_file: Some(cops::rails::safe_navigation::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::safe_navigation_with_blank::NAME,
        run_file: Some(cops::rails::safe_navigation_with_blank::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::save_bang::NAME,
        run_file: Some(cops::rails::save_bang::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::schema_comment::NAME,
        run_file: Some(cops::rails::schema_comment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::scope_args::NAME,
        run_file: Some(cops::rails::scope_args::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::select_map::NAME,
        run_file: Some(cops::rails::select_map::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::short_i18n::NAME,
        run_file: Some(cops::rails::short_i18n::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::skips_model_validations::NAME,
        run_file: Some(cops::rails::skips_model_validations::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::squished_sql_heredocs::NAME,
        run_file: Some(cops::rails::squished_sql_heredocs::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::strip_heredoc::NAME,
        run_file: Some(cops::rails::strip_heredoc::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::strong_parameters_expect::NAME,
        run_file: Some(cops::rails::strong_parameters_expect::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::table_name_assignment::NAME,
        run_file: Some(cops::rails::table_name_assignment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::three_state_boolean_column::NAME,
        run_file: Some(cops::rails::three_state_boolean_column::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::time_zone::NAME,
        run_file: Some(cops::rails::time_zone::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::time_zone_assignment::NAME,
        run_file: Some(cops::rails::time_zone_assignment::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::to_formatted_s::NAME,
        run_file: Some(cops::rails::to_formatted_s::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::to_s_with_argument::NAME,
        run_file: Some(cops::rails::to_s_with_argument::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::top_level_hash_with_indifferent_access::NAME,
        run_file: Some(cops::rails::top_level_hash_with_indifferent_access::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::transaction_exit_statement::NAME,
        run_file: Some(cops::rails::transaction_exit_statement::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::uniq_before_pluck::NAME,
        run_file: Some(cops::rails::uniq_before_pluck::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unique_validation_without_index::NAME,
        run_file: Some(cops::rails::unique_validation_without_index::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unknown_env::NAME,
        run_file: Some(cops::rails::unknown_env::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unused_ignored_columns::NAME,
        run_file: Some(cops::rails::unused_ignored_columns::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::unused_render_content::NAME,
        run_file: Some(cops::rails::unused_render_content::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::validation::NAME,
        run_file: Some(cops::rails::validation::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_equals::NAME,
        run_file: Some(cops::rails::where_equals::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_exists::NAME,
        run_file: Some(cops::rails::where_exists::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_missing::NAME,
        run_file: Some(cops::rails::where_missing::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_not::NAME,
        run_file: Some(cops::rails::where_not::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_not_with_multiple_conditions::NAME,
        run_file: Some(cops::rails::where_not_with_multiple_conditions::run),
        run_call: None,
    },
    MurphyPluginCopV1 {
        size: std::mem::size_of::<MurphyPluginCopV1>(),
        name: cops::rails::where_range::NAME,
        run_file: Some(cops::rails::where_range::run),
        run_call: None,
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
            call_dispatch_ptr: CALL_DISPATCH.as_ptr(),
            call_dispatch_len: CALL_DISPATCH.len(),
        };
    }

    0
}
