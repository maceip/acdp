use std::env;

fn main() {
    // Only set rpath if llm feature is enabled
    #[cfg(feature = "llm")]
    {
        // Get the LiteRT-LM path from environment or use default
        let litert_path = env::var("LITERT_LM_PATH").unwrap_or_else(|_| {
            // Default to home directory
            let home = env::var("HOME").expect("HOME not set");
            format!("{}/LiteRT-LM", home)
        });

        let lib_path = format!("{}/bazel-bin/rust_api", litert_path);

        // Set rpath so the dynamic linker can find the LiteRT library at runtime
        if cfg!(target_os = "macos") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path);
        } else if cfg!(target_os = "linux") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path);
        }
    }
}
