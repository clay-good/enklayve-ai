fn main() {
    link_macos_compiler_rt();
    tauri_build::build()
}

/// llama.cpp's Metal backend (ggml-metal-device.m) uses `@available(macOS 15.0, *)`
/// checks, which the compiler lowers to calls to the compiler-rt builtin
/// `___isPlatformVersionAtLeast`. That symbol lives in `libclang_rt.osx.a` and is
/// not pulled in by default when Rust performs the final link on Apple Silicon,
/// producing "Undefined symbols for architecture arm64: ___isPlatformVersionAtLeast".
/// Link clang's compiler-rt explicitly on macOS so the builtin is available.
fn link_macos_compiler_rt() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let Ok(output) = std::process::Command::new("clang")
        .arg("-print-resource-dir")
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let lib_dir = std::path::Path::new(&resource_dir).join("lib").join("darwin");

    // The archive is a static library; the linker only pulls the objects it needs,
    // so adding it is a no-op when the symbol is already resolved.
    if lib_dir.join("libclang_rt.osx.a").exists() {
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
}
