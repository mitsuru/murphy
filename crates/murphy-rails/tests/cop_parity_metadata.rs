use murphy_plugin_api::test_support::assert_cop_parity_metadata_for_crate;

#[test]
fn every_cop_has_a_matching_parity_metadata_block() {
    assert_cop_parity_metadata_for_crate(env!("CARGO_MANIFEST_DIR"));
}
