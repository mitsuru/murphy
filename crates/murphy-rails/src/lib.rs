mod cops;

use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense,
    MurphyPluginCopV1, MurphyPluginV1, MurphySlice, cop_v1, cop_v1_dispatch_only,
};
use std::ffi::c_void;

const fn slice(bytes: &'static [u8]) -> MurphySlice {
    MurphySlice {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

static CALL_DISPATCH: [MurphyCallDispatchV1; 106] = [
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
    pluralization_dispatch(b"second", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"minute", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"hour", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"day", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"week", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"fortnight", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"month", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"year", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"byte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"kilobyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"megabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"gigabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"terabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"petabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"exabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"zettabyte", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"seconds", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"minutes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"hours", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"days", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"weeks", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"fortnights", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"months", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"years", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"bytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"kilobytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"megabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"gigabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"terabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"petabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"exabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    pluralization_dispatch(b"zettabytes", PLURALIZATION_GRAMMAR_DISPATCH_ID),
    refute_dispatch(b"refute", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_empty", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_equal", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_in_delta", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_in_epsilon", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_includes", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_instance_of", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_kind_of", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_nil", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_operator", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_predicate", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_respond_to", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_same", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"refute_match", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_empty", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_equal", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_in_delta", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_in_epsilon", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_includes", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_instance_of", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_kind_of", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_nil", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_operator", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_predicate", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_respond_to", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_not_same", REFUTE_METHODS_DISPATCH_ID),
    refute_dispatch(b"assert_no_match", REFUTE_METHODS_DISPATCH_ID),
    lexically_scoped_action_filter_dispatch(
        b"after_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"append_after_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"append_around_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"append_before_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"around_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"before_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"prepend_after_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"prepend_around_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"prepend_before_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"skip_after_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"skip_around_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"skip_before_action",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    lexically_scoped_action_filter_dispatch(
        b"skip_action_callback",
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID,
    ),
    http_positional_arguments_dispatch(b"get", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    http_positional_arguments_dispatch(b"post", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    http_positional_arguments_dispatch(b"put", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    http_positional_arguments_dispatch(b"patch", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    http_positional_arguments_dispatch(b"delete", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    http_positional_arguments_dispatch(b"head", HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID),
    multiple_route_paths_dispatch(b"get", MULTIPLE_ROUTE_PATHS_DISPATCH_ID),
    multiple_route_paths_dispatch(b"post", MULTIPLE_ROUTE_PATHS_DISPATCH_ID),
    multiple_route_paths_dispatch(b"put", MULTIPLE_ROUTE_PATHS_DISPATCH_ID),
    multiple_route_paths_dispatch(b"patch", MULTIPLE_ROUTE_PATHS_DISPATCH_ID),
    multiple_route_paths_dispatch(b"delete", MULTIPLE_ROUTE_PATHS_DISPATCH_ID),
    validation_dispatch(b"validates_acceptance_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_comparison_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_confirmation_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_exclusion_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_format_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_inclusion_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_length_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_numericality_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_presence_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_absence_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_size_of", VALIDATION_DISPATCH_ID),
    validation_dispatch(b"validates_uniqueness_of", VALIDATION_DISPATCH_ID),
];

const OUTPUT_COP_INDEX: usize = 76;
const OUTPUT_DISPATCH_ID: usize = 1;
const PLURALIZATION_GRAMMAR_COP_INDEX: usize = 82;
const PLURALIZATION_GRAMMAR_DISPATCH_ID: usize = 2;
const REFUTE_METHODS_COP_INDEX: usize = 95;
const REFUTE_METHODS_DISPATCH_ID: usize = 3;
const LEXICALLY_SCOPED_ACTION_FILTER_COP_INDEX: usize = 66;
const LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID: usize = 4;
const HTTP_POSITIONAL_ARGUMENTS_COP_INDEX: usize = 54;
const HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID: usize = 5;
const MULTIPLE_ROUTE_PATHS_COP_INDEX: usize = 71;
const MULTIPLE_ROUTE_PATHS_DISPATCH_ID: usize = 6;
const VALIDATION_COP_INDEX: usize = 131;
const VALIDATION_DISPATCH_ID: usize = 7;

const fn output_dispatch(method_name: &'static [u8], dispatch_id: usize) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: OUTPUT_COP_INDEX,
        dispatch_id,
    }
}

const fn pluralization_dispatch(
    method_name: &'static [u8],
    dispatch_id: usize,
) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: PLURALIZATION_GRAMMAR_COP_INDEX,
        dispatch_id,
    }
}

const fn refute_dispatch(method_name: &'static [u8], dispatch_id: usize) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: REFUTE_METHODS_COP_INDEX,
        dispatch_id,
    }
}

const fn lexically_scoped_action_filter_dispatch(
    method_name: &'static [u8],
    dispatch_id: usize,
) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: LEXICALLY_SCOPED_ACTION_FILTER_COP_INDEX,
        dispatch_id,
    }
}

const fn http_positional_arguments_dispatch(
    method_name: &'static [u8],
    dispatch_id: usize,
) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: HTTP_POSITIONAL_ARGUMENTS_COP_INDEX,
        dispatch_id,
    }
}

const fn multiple_route_paths_dispatch(
    method_name: &'static [u8],
    dispatch_id: usize,
) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: MULTIPLE_ROUTE_PATHS_COP_INDEX,
        dispatch_id,
    }
}

const fn validation_dispatch(
    method_name: &'static [u8],
    dispatch_id: usize,
) -> MurphyCallDispatchV1 {
    MurphyCallDispatchV1 {
        method_name: slice(method_name),
        cop_index: VALIDATION_COP_INDEX,
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
        PLURALIZATION_GRAMMAR_DISPATCH_ID => unsafe {
            cops::rails::pluralization_grammar::run_call(ctx, emit, sink)
        },
        REFUTE_METHODS_DISPATCH_ID => unsafe {
            cops::rails::refute_methods::run_call(ctx, emit, sink)
        },
        LEXICALLY_SCOPED_ACTION_FILTER_DISPATCH_ID => unsafe {
            cops::rails::lexically_scoped_action_filter::run_call(ctx, emit, sink)
        },
        HTTP_POSITIONAL_ARGUMENTS_DISPATCH_ID => unsafe {
            cops::rails::http_positional_arguments::run_call(ctx, emit, sink)
        },
        MULTIPLE_ROUTE_PATHS_DISPATCH_ID => unsafe {
            cops::rails::multiple_route_paths::run_call(ctx, emit, sink)
        },
        VALIDATION_DISPATCH_ID => unsafe { cops::rails::validation::run_call(ctx, emit, sink) },
        _ => 0,
    }
}

const COPS: [MurphyPluginCopV1; 138] = [
    cop_v1(
        cops::rails::action_controller_flash_before_render::NAME,
        cops::rails::action_controller_flash_before_render::run,
    ),
    cop_v1(
        cops::rails::action_controller_test_case::NAME,
        cops::rails::action_controller_test_case::run,
    ),
    cop_v1(
        cops::rails::action_filter::NAME,
        cops::rails::action_filter::run,
    ),
    cop_v1(
        cops::rails::action_order::NAME,
        cops::rails::action_order::run,
    ),
    cop_v1(
        cops::rails::active_record_aliases::NAME,
        cops::rails::active_record_aliases::run,
    ),
    cop_v1(
        cops::rails::active_record_callbacks_order::NAME,
        cops::rails::active_record_callbacks_order::run,
    ),
    cop_v1(
        cops::rails::active_record_override::NAME,
        cops::rails::active_record_override::run,
    ),
    cop_v1(
        cops::rails::active_support_aliases::NAME,
        cops::rails::active_support_aliases::run,
    ),
    cop_v1(
        cops::rails::active_support_on_load::NAME,
        cops::rails::active_support_on_load::run,
    ),
    cop_v1(
        cops::rails::add_column_index::NAME,
        cops::rails::add_column_index::run,
    ),
    cop_v1(
        cops::rails::after_commit_override::NAME,
        cops::rails::after_commit_override::run,
    ),
    cop_v1(
        cops::rails::application_controller::NAME,
        cops::rails::application_controller::run,
    ),
    cop_v1(
        cops::rails::application_job::NAME,
        cops::rails::application_job::run,
    ),
    cop_v1(
        cops::rails::application_mailer::NAME,
        cops::rails::application_mailer::run,
    ),
    cop_v1(
        cops::rails::application_record::NAME,
        cops::rails::application_record::run,
    ),
    cop_v1(cops::rails::arel_star::NAME, cops::rails::arel_star::run),
    cop_v1(cops::rails::assert_not::NAME, cops::rails::assert_not::run),
    cop_v1(
        cops::rails::attribute_default_block_value::NAME,
        cops::rails::attribute_default_block_value::run,
    ),
    cop_v1(cops::rails::belongs_to::NAME, cops::rails::belongs_to::run),
    cop_v1(cops::rails::blank::NAME, cops::rails::blank::run),
    cop_v1(
        cops::rails::bulk_change_table::NAME,
        cops::rails::bulk_change_table::run,
    ),
    cop_v1(
        cops::rails::compact_blank::NAME,
        cops::rails::compact_blank::run,
    ),
    cop_v1(
        cops::rails::content_tag::NAME,
        cops::rails::content_tag::run,
    ),
    cop_v1(
        cops::rails::create_table_with_timestamps::NAME,
        cops::rails::create_table_with_timestamps::run,
    ),
    cop_v1(
        cops::rails::dangerous_column_names::NAME,
        cops::rails::dangerous_column_names::run,
    ),
    cop_v1(cops::rails::date::NAME, cops::rails::date::run),
    cop_v1(
        cops::rails::default_scope::NAME,
        cops::rails::default_scope::run,
    ),
    cop_v1(cops::rails::delegate::NAME, cops::rails::delegate::run),
    cop_v1(
        cops::rails::delegate_allow_blank::NAME,
        cops::rails::delegate_allow_blank::run,
    ),
    cop_v1(
        cops::rails::deprecated_active_model_errors_methods::NAME,
        cops::rails::deprecated_active_model_errors_methods::run,
    ),
    cop_v1(
        cops::rails::dot_separated_keys::NAME,
        cops::rails::dot_separated_keys::run,
    ),
    cop_v1(
        cops::rails::duplicate_association::NAME,
        cops::rails::duplicate_association::run,
    ),
    cop_v1(
        cops::rails::duplicate_scope::NAME,
        cops::rails::duplicate_scope::run,
    ),
    cop_v1(
        cops::rails::duration_arithmetic::NAME,
        cops::rails::duration_arithmetic::run,
    ),
    cop_v1(
        cops::rails::dynamic_find_by::NAME,
        cops::rails::dynamic_find_by::run,
    ),
    cop_v1(
        cops::rails::eager_evaluation_log_message::NAME,
        cops::rails::eager_evaluation_log_message::run,
    ),
    cop_v1(cops::rails::enum_hash::NAME, cops::rails::enum_hash::run),
    cop_v1(
        cops::rails::enum_syntax::NAME,
        cops::rails::enum_syntax::run,
    ),
    cop_v1(
        cops::rails::enum_uniqueness::NAME,
        cops::rails::enum_uniqueness::run,
    ),
    cop_v1(cops::rails::env::NAME, cops::rails::env::run),
    cop_v1(cops::rails::env_local::NAME, cops::rails::env_local::run),
    cop_v1(
        cops::rails::environment_comparison::NAME,
        cops::rails::environment_comparison::run,
    ),
    cop_v1(
        cops::rails::environment_variable_access::NAME,
        cops::rails::environment_variable_access::run,
    ),
    cop_v1(cops::rails::exit::NAME, cops::rails::exit::run),
    cop_v1(
        cops::rails::expanded_date_range::NAME,
        cops::rails::expanded_date_range::run,
    ),
    cop_v1(cops::rails::file_path::NAME, cops::rails::file_path::run),
    cop_v1(cops::rails::find_by::NAME, cops::rails::find_by::run),
    cop_v1(cops::rails::find_by_id::NAME, cops::rails::find_by_id::run),
    cop_v1(
        cops::rails::find_by_or_assignment_memoization::NAME,
        cops::rails::find_by_or_assignment_memoization::run,
    ),
    cop_v1(cops::rails::find_each::NAME, cops::rails::find_each::run),
    cop_v1(
        cops::rails::freeze_time::NAME,
        cops::rails::freeze_time::run,
    ),
    cop_v1(
        cops::rails::has_and_belongs_to_many::NAME,
        cops::rails::has_and_belongs_to_many::run,
    ),
    cop_v1(
        cops::rails::has_many_or_has_one_dependent::NAME,
        cops::rails::has_many_or_has_one_dependent::run,
    ),
    cop_v1(
        cops::rails::helper_instance_variable::NAME,
        cops::rails::helper_instance_variable::run,
    ),
    cop_v1_dispatch_only(cops::rails::http_positional_arguments::NAME),
    cop_v1(
        cops::rails::http_status::NAME,
        cops::rails::http_status::run,
    ),
    cop_v1(
        cops::rails::http_status_name_consistency::NAME,
        cops::rails::http_status_name_consistency::run,
    ),
    cop_v1(
        cops::rails::i18n_lazy_lookup::NAME,
        cops::rails::i18n_lazy_lookup::run,
    ),
    cop_v1(
        cops::rails::i18n_locale_assignment::NAME,
        cops::rails::i18n_locale_assignment::run,
    ),
    cop_v1(
        cops::rails::i18n_locale_texts::NAME,
        cops::rails::i18n_locale_texts::run,
    ),
    cop_v1(
        cops::rails::ignored_columns_assignment::NAME,
        cops::rails::ignored_columns_assignment::run,
    ),
    cop_v1(
        cops::rails::ignored_skip_action_filter_option::NAME,
        cops::rails::ignored_skip_action_filter_option::run,
    ),
    cop_v1(cops::rails::index_by::NAME, cops::rails::index_by::run),
    cop_v1(cops::rails::index_with::NAME, cops::rails::index_with::run),
    cop_v1(cops::rails::inquiry::NAME, cops::rails::inquiry::run),
    cop_v1(cops::rails::inverse_of::NAME, cops::rails::inverse_of::run),
    cop_v1_dispatch_only(cops::rails::lexically_scoped_action_filter::NAME),
    cop_v1(
        cops::rails::link_to_blank::NAME,
        cops::rails::link_to_blank::run,
    ),
    cop_v1(
        cops::rails::mailer_name::NAME,
        cops::rails::mailer_name::run,
    ),
    cop_v1(
        cops::rails::match_route::NAME,
        cops::rails::match_route::run,
    ),
    cop_v1(
        cops::rails::migration_class_name::NAME,
        cops::rails::migration_class_name::run,
    ),
    cop_v1_dispatch_only(cops::rails::multiple_route_paths::NAME),
    cop_v1(
        cops::rails::negate_include::NAME,
        cops::rails::negate_include::run,
    ),
    cop_v1(
        cops::rails::not_null_column::NAME,
        cops::rails::not_null_column::run,
    ),
    cop_v1(
        cops::rails::order_arguments::NAME,
        cops::rails::order_arguments::run,
    ),
    cop_v1(
        cops::rails::order_by_id::NAME,
        cops::rails::order_by_id::run,
    ),
    cop_v1_dispatch_only(cops::rails::output::NAME),
    cop_v1(
        cops::rails::output_safety::NAME,
        cops::rails::output_safety::run,
    ),
    cop_v1(cops::rails::pick::NAME, cops::rails::pick::run),
    cop_v1(cops::rails::pluck::NAME, cops::rails::pluck::run),
    cop_v1(cops::rails::pluck_id::NAME, cops::rails::pluck_id::run),
    cop_v1(
        cops::rails::pluck_in_where::NAME,
        cops::rails::pluck_in_where::run,
    ),
    cop_v1_dispatch_only(cops::rails::pluralization_grammar::NAME),
    cop_v1(cops::rails::presence::NAME, cops::rails::presence::run),
    cop_v1(cops::rails::present::NAME, cops::rails::present::run),
    cop_v1(
        cops::rails::rake_environment::NAME,
        cops::rails::rake_environment::run,
    ),
    cop_v1(
        cops::rails::read_write_attribute::NAME,
        cops::rails::read_write_attribute::run,
    ),
    cop_v1(
        cops::rails::redirect_back_or_to::NAME,
        cops::rails::redirect_back_or_to::run,
    ),
    cop_v1(
        cops::rails::redundant_active_record_all_method::NAME,
        cops::rails::redundant_active_record_all_method::run,
    ),
    cop_v1(
        cops::rails::redundant_allow_nil::NAME,
        cops::rails::redundant_allow_nil::run,
    ),
    cop_v1(
        cops::rails::redundant_foreign_key::NAME,
        cops::rails::redundant_foreign_key::run,
    ),
    cop_v1(
        cops::rails::redundant_presence_validation_on_belongs_to::NAME,
        cops::rails::redundant_presence_validation_on_belongs_to::run,
    ),
    cop_v1(
        cops::rails::redundant_receiver_in_with_options::NAME,
        cops::rails::redundant_receiver_in_with_options::run,
    ),
    cop_v1(
        cops::rails::redundant_travel_back::NAME,
        cops::rails::redundant_travel_back::run,
    ),
    cop_v1(
        cops::rails::reflection_class_name::NAME,
        cops::rails::reflection_class_name::run,
    ),
    cop_v1_dispatch_only(cops::rails::refute_methods::NAME),
    cop_v1(
        cops::rails::relative_date_constant::NAME,
        cops::rails::relative_date_constant::run,
    ),
    cop_v1(
        cops::rails::render_inline::NAME,
        cops::rails::render_inline::run,
    ),
    cop_v1(
        cops::rails::render_plain_text::NAME,
        cops::rails::render_plain_text::run,
    ),
    cop_v1(
        cops::rails::request_referer::NAME,
        cops::rails::request_referer::run,
    ),
    cop_v1(
        cops::rails::require_dependency::NAME,
        cops::rails::require_dependency::run,
    ),
    cop_v1(
        cops::rails::response_parsed_body::NAME,
        cops::rails::response_parsed_body::run,
    ),
    cop_v1(
        cops::rails::reversible_migration::NAME,
        cops::rails::reversible_migration::run,
    ),
    cop_v1(
        cops::rails::reversible_migration_method_definition::NAME,
        cops::rails::reversible_migration_method_definition::run,
    ),
    cop_v1(
        cops::rails::root_join_chain::NAME,
        cops::rails::root_join_chain::run,
    ),
    cop_v1(
        cops::rails::root_pathname_methods::NAME,
        cops::rails::root_pathname_methods::run,
    ),
    cop_v1(
        cops::rails::root_public_path::NAME,
        cops::rails::root_public_path::run,
    ),
    cop_v1(
        cops::rails::safe_navigation::NAME,
        cops::rails::safe_navigation::run,
    ),
    cop_v1(
        cops::rails::safe_navigation_with_blank::NAME,
        cops::rails::safe_navigation_with_blank::run,
    ),
    cop_v1(cops::rails::save_bang::NAME, cops::rails::save_bang::run),
    cop_v1(
        cops::rails::schema_comment::NAME,
        cops::rails::schema_comment::run,
    ),
    cop_v1(cops::rails::scope_args::NAME, cops::rails::scope_args::run),
    cop_v1(cops::rails::select_map::NAME, cops::rails::select_map::run),
    cop_v1(cops::rails::short_i18n::NAME, cops::rails::short_i18n::run),
    cop_v1(
        cops::rails::skips_model_validations::NAME,
        cops::rails::skips_model_validations::run,
    ),
    cop_v1(
        cops::rails::squished_sql_heredocs::NAME,
        cops::rails::squished_sql_heredocs::run,
    ),
    cop_v1(
        cops::rails::strip_heredoc::NAME,
        cops::rails::strip_heredoc::run,
    ),
    cop_v1(
        cops::rails::strong_parameters_expect::NAME,
        cops::rails::strong_parameters_expect::run,
    ),
    cop_v1(
        cops::rails::table_name_assignment::NAME,
        cops::rails::table_name_assignment::run,
    ),
    cop_v1(
        cops::rails::three_state_boolean_column::NAME,
        cops::rails::three_state_boolean_column::run,
    ),
    cop_v1(cops::rails::time_zone::NAME, cops::rails::time_zone::run),
    cop_v1(
        cops::rails::time_zone_assignment::NAME,
        cops::rails::time_zone_assignment::run,
    ),
    cop_v1(
        cops::rails::to_formatted_s::NAME,
        cops::rails::to_formatted_s::run,
    ),
    cop_v1(
        cops::rails::to_s_with_argument::NAME,
        cops::rails::to_s_with_argument::run,
    ),
    cop_v1(
        cops::rails::top_level_hash_with_indifferent_access::NAME,
        cops::rails::top_level_hash_with_indifferent_access::run,
    ),
    cop_v1(
        cops::rails::transaction_exit_statement::NAME,
        cops::rails::transaction_exit_statement::run,
    ),
    cop_v1(
        cops::rails::uniq_before_pluck::NAME,
        cops::rails::uniq_before_pluck::run,
    ),
    cop_v1(
        cops::rails::unique_validation_without_index::NAME,
        cops::rails::unique_validation_without_index::run,
    ),
    cop_v1(
        cops::rails::unknown_env::NAME,
        cops::rails::unknown_env::run,
    ),
    cop_v1(
        cops::rails::unused_ignored_columns::NAME,
        cops::rails::unused_ignored_columns::run,
    ),
    cop_v1(
        cops::rails::unused_render_content::NAME,
        cops::rails::unused_render_content::run,
    ),
    cop_v1_dispatch_only(cops::rails::validation::NAME),
    cop_v1(
        cops::rails::where_equals::NAME,
        cops::rails::where_equals::run,
    ),
    cop_v1(
        cops::rails::where_exists::NAME,
        cops::rails::where_exists::run,
    ),
    cop_v1(
        cops::rails::where_missing::NAME,
        cops::rails::where_missing::run,
    ),
    cop_v1(cops::rails::where_not::NAME, cops::rails::where_not::run),
    cop_v1(
        cops::rails::where_not_with_multiple_conditions::NAME,
        cops::rails::where_not_with_multiple_conditions::run,
    ),
    cop_v1(
        cops::rails::where_range::NAME,
        cops::rails::where_range::run,
    ),
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
            node_dispatch_ptr: std::ptr::null(),
            node_dispatch_len: 0,
            run_node_dispatch: None,
        };
    }

    0
}
