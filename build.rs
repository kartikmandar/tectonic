// Copyright 2016-2021 the Tectonic Project
// Licensed under the MIT License.

use std::env;

fn main() {
    // Depend on this file to prevent rebuilding on any change - see #1173 for details
    println!("cargo:rerun-if-changed=build.rs");

    // Register custom cfg conditions
    println!("cargo:rustc-check-cfg=cfg(target_wasm)");
    println!("cargo:rustc-check-cfg=cfg(wasm_enabled)");
    println!("cargo:rustc-check-cfg=cfg(wasm_simd)");

    // Re-export $TARGET during the build so that our executable tests know
    // what environment variable CARGO_TARGET_@TARGET@_RUNNER to check when
    // they want to spawn off executables.
    let target = env::var("TARGET").unwrap();
    println!("cargo:rustc-env=TARGET={target}");

    // WASM target detection
    if target.starts_with("wasm32") {
        println!("cargo:rustc-cfg=target_wasm");
        
        // Enable WASM-specific features
        if cfg!(feature = "wasm") {
            println!("cargo:rustc-cfg=wasm_enabled");
        }

        // SIMD support for WebAssembly
        if cfg!(feature = "simd") {
            println!("cargo:rustc-cfg=wasm_simd");
            // Enable WASM SIMD features (requires nightly or recent stable)
            println!("cargo:rustc-cfg=target_feature=\"simd128\"");
        }
    }

    // Set appropriate compiler flags for SIMD
    if cfg!(feature = "simd") {
        if target.starts_with("wasm32") {
            // WASM SIMD flags
            println!("cargo:rustc-link-arg=-C");
            println!("cargo:rustc-link-arg=target-feature=+simd128");
        } else {
            // Native SIMD flags (for development/testing)
            #[cfg(target_arch = "x86_64")]
            {
                println!("cargo:rustc-link-arg=-C");
                println!("cargo:rustc-link-arg=target-cpu=native");
            }
        }
    }
}
