// Phase 7 preparatory build hooks for an optional custom mruby build.
//
// The current production path remains untouched (`mruby3-sys`).
// When `MURPHY_MRUBY_CUSTOM_BUILD=1` is set in the build environment,
// this emits a cfg flag for the optional custom-runtime path.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(mruby_custom_build)");
    println!("cargo:rerun-if-env-changed=MURPHY_MRUBY_CUSTOM_BUILD");
    println!("cargo:rerun-if-env-changed=MURPHY_MRUBY_CUSTOM_BUILD_PATH");

    let feature_enabled = std::env::var("CARGO_FEATURE_MRUBY_CUSTOM_BUILD").is_ok();
    let env_enabled = std::env::var("MURPHY_MRUBY_CUSTOM_BUILD").is_ok();

    if feature_enabled || env_enabled {
        println!("cargo:rustc-cfg=mruby_custom_build");

        if let Ok(path) = std::env::var("MURPHY_MRUBY_CUSTOM_BUILD_PATH") {
            println!("cargo:rustc-env=MURPHY_MRUBY_CUSTOM_BUILD_PATH={path}");
            println!("cargo:warning=Using custom mruby build path: {path}");
        }
    }
}
