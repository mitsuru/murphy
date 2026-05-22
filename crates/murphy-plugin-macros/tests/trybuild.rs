//! Compile-pass / compile-fail coverage for the plugin macros, against
//! the single-surface ABI (murphy-9cr.21).

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
