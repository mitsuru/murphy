extern crate bindgen;

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

fn require_tool(tool: &str) {
    let output = Command::new(tool)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| {
            panic!("ruby/rake required on PATH to build mruby3-sys; could not run `{tool} --version`: {error}")
        });
    if !output.status.success() {
        panic!(
            "ruby/rake required on PATH to build mruby3-sys; `{tool} --version` exited with {:?}",
            output.status
        );
    }
}

fn remove_lock_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => panic!(
            "failed to remove mruby lock file `{}`: {error}",
            path.display()
        ),
    }
}

fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let lock_path = out_path.join("mruby3-sys.lock");

    require_tool("ruby");
    require_tool("rake");
    // mruby3-sys is downloaded into Cargo's shared registry. Give each Cargo
    // build its own lock file instead of mutating that shared source directory.
    remove_lock_file(&lock_path);
    let make_status = Command::new("make")
        .env("CFLAGS", "-fPIE")
        .args(["-C", "mruby"])
        .env("INSTALL_DIR", out_path.clone())
        .env("MRUBY_BUILD_DIR", out_path.clone())
        .env("MRUBY_LOCKFILE", &lock_path)
        .status()
        .unwrap_or_else(|error| panic!("failed to run `make` for mruby3-sys: {error}"));
    // Clean the per-build lock on both success and failure. Never remove a file
    // from the shared crate source tree.
    remove_lock_file(&lock_path);
    if !make_status.success() {
        panic!("mruby make failed with status {make_status:?}");
    }

    println!(
        "cargo:rustc-link-search={}/host/lib",
        out_path.to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=mruby");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=mruby/lib/mruby/lockfile.rb");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-I./mruby/include")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    Command::new("patch")
        .args(&[out_path.join("bindings.rs"), "bindings.patch".into()])
        .status()
        .expect("patching bindings.rs failed");
}
