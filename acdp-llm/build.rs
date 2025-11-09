use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(litert_dynamic)");
    println!("cargo:rustc-check-cfg=cfg(litert_stub)");
    println!("cargo:rerun-if-env-changed=LITERT_LM_PATH");

    // Get the LiteRT-LM path from environment or use default
    let litert_path = env::var("LITERT_LM_PATH").unwrap_or_else(|_| {
        // Default to home directory
        let home = env::var("HOME").expect("HOME not set");
        format!("{}/LiteRT-LM", home)
    });

    let lib_path = format!("{}/bazel-bin/rust_api", litert_path);

    let dylib = format!("{}/litert_lm_rust_api.dylib", lib_path);
    let so = format!("{}/litert_lm_rust_api.so", lib_path);
    let has_dylib = Path::new(&dylib).exists();
    let has_so = Path::new(&so).exists();

    if !has_dylib && !has_so {
        println!("cargo:rustc-cfg=litert_stub");
        println!(
            "cargo:warning=LiteRT-LM library not found in {}. Building with stub bindings.",
            lib_path
        );
        return;
    }

    println!("cargo:rustc-cfg=litert_dynamic");
    // Link-time search path
    println!("cargo:rustc-link-search=native={}", lib_path);
    println!("cargo:rustc-link-lib=dylib=litert_lm_rust_api");

    // On macOS, also link c++
    println!("cargo:rustc-link-lib=dylib=c++");

    // CRITICAL: Set rpath so the dynamic linker can find the library at runtime
    // This embeds the library search path into the binary
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path);
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path);
    }

    // Tell cargo to rerun if the library changes
    if has_so {
        println!("cargo:rerun-if-changed={}/litert_lm_rust_api.so", lib_path);
    }
    if has_dylib {
        println!(
            "cargo:rerun-if-changed={}/litert_lm_rust_api.dylib",
            lib_path
        );
    }

    // Print the path for debugging
    eprintln!("Looking for LiteRT-LM library in: {}", lib_path);
}
