fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=MURPHY_TARGET_TRIPLE={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
