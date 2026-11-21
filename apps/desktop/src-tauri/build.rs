use std::process::Command;

fn main() {
    link_clang_runtime_for_metal();
    tauri_build::build()
}

/// whisper.cpp's Metal backend uses Objective-C `@available` checks. When the
/// app targets a macOS older than the version checked, those compile to calls
/// into clang's runtime library, which rustc does not link by default.
fn link_clang_runtime_for_metal() {
    let metal = std::env::var_os("CARGO_FEATURE_METAL").is_some();
    let macos = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    if !(metal && macos) {
        return;
    }
    let output = Command::new("clang")
        .arg("-print-file-name=libclang_rt.osx.a")
        .output();
    let Ok(output) = output else {
        println!("cargo:warning=clang not found; the Metal backend may fail to link");
        return;
    };
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // clang echoes the bare file name back when it cannot find the library.
    if path.contains('/') {
        println!("cargo:rustc-link-arg={path}");
    } else {
        println!("cargo:warning=libclang_rt.osx.a not found; the Metal backend may fail to link");
    }
}
