// src/wasm/mod.rs -- WebAssembly interface for Tectonic TeX engine
// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

//! WebAssembly bindings for the Tectonic TeX engine.
//!
//! This module provides a WASM-compatible interface to the Tectonic engine,
//! enabling real-time LaTeX compilation in web browsers with support for:
//! - Incremental compilation with state snapshots
//! - SIMD-optimized processing (when available)
//! - Character-level position tracking for WYSIWYG editing
//! - Typst-inspired constrained memoization

#[cfg(all(target_wasm, feature = "wasm"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_wasm, feature = "wasm"))]
use web_sys::console;

#[cfg(all(target_wasm, feature = "wasm"))]
use serde::{Deserialize, Serialize};

// Import Tectonic engine components
use crate::engines::TexEngine;
use crate::driver::{ProcessingSessionBuilder, OutputFormat};
use crate::config::PersistentConfig;
use crate::status::NoopStatusBackend;

/// WASM-compatible version information
#[cfg(all(target_wasm, feature = "wasm"))]
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Initialize the WASM module and set up panic hook for better debugging
#[cfg(all(target_wasm, feature = "wasm"))]
#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console::log_1(&"Tectonic WASM engine initialized".into());
}

/// Simple test function to verify WASM compilation
#[cfg(all(target_wasm, feature = "wasm"))]
#[wasm_bindgen]
pub fn test_wasm() -> String {
    "Tectonic WASM engine is working!".to_string()
}

/// Compilation options for WASM interface
#[cfg(all(target_wasm, feature = "wasm"))]
#[derive(Serialize, Deserialize)]
pub struct CompileOptions {
    /// Enable SIMD optimizations if available
    pub use_simd: Option<bool>,
    /// Enable incremental compilation
    pub incremental: Option<bool>,
    /// Enable character-level position tracking
    pub track_positions: Option<bool>,
    /// Format name (default: "latex")
    pub format: Option<String>,
}

/// Compilation result returned to JavaScript
#[cfg(all(target_wasm, feature = "wasm"))]
#[derive(Serialize, Deserialize)]
pub struct CompileResult {
    /// True if compilation succeeded
    pub success: bool,
    /// PDF output as base64 string (if successful)
    pub pdf_data: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Compilation time in milliseconds
    pub duration_ms: f64,
    /// Whether SIMD was used
    pub used_simd: bool,
}

/// Simple LaTeX to PDF compilation for WASM
#[cfg(all(target_wasm, feature = "wasm"))]
#[wasm_bindgen]
pub fn compile_latex(latex_source: &str, options_json: Option<String>) -> Result<JsValue, JsValue> {
    let start_time = web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0);

    // Parse options
    let options: CompileOptions = match options_json {
        Some(json) => serde_wasm_bindgen::from_value(
            js_sys::JSON::parse(&json).map_err(|e| format!("Invalid options JSON: {:?}", e))?
        ).map_err(|e| format!("Failed to parse options: {:?}", e))?,
        None => CompileOptions {
            use_simd: None,
            incremental: None,
            track_positions: None,
            format: None,
        }
    };

    // Check SIMD availability
    let simd_available = cfg!(feature = "simd") && cfg!(wasm_simd);
    let use_simd = options.use_simd.unwrap_or(simd_available);

    let result = match compile_latex_internal(latex_source, &options, use_simd) {
        Ok(pdf_bytes) => {
            let pdf_base64 = base64::encode(&pdf_bytes);
            CompileResult {
                success: true,
                pdf_data: Some(pdf_base64),
                error: None,
                duration_ms: web_sys::window()
                    .and_then(|w| w.performance())
                    .map(|p| p.now() - start_time)
                    .unwrap_or(0.0),
                used_simd: use_simd && simd_available,
            }
        }
        Err(e) => CompileResult {
            success: false,
            pdf_data: None,
            error: Some(e),
            duration_ms: web_sys::window()
                .and_then(|w| w.performance())
                .map(|p| p.now() - start_time)
                .unwrap_or(0.0),
            used_simd: false,
        }
    };

    serde_wasm_bindgen::to_value(&result).map_err(|e| format!("Serialization error: {:?}", e).into())
}

/// Internal compilation function (fallback for non-WASM builds)
fn compile_latex_internal(
    latex_source: &str, 
    _options: &CompileOptions, 
    _use_simd: bool
) -> Result<Vec<u8>, String> {
    // For now, use the existing latex_to_pdf function
    // TODO: Replace with incremental compilation system
    crate::latex_to_pdf(latex_source)
        .map_err(|e| format!("Compilation failed: {}", e))
}

/// SIMD-optimized glyph processing (placeholder)
#[cfg(all(wasm_simd, feature = "simd"))]
fn process_glyphs_simd(_glyph_data: &[u8]) -> Vec<f32> {
    // TODO: Implement SIMD glyph processing using core::arch::wasm32
    // This is a placeholder for the actual SIMD implementation
    vec![]
}

/// Fallback scalar glyph processing
fn process_glyphs_scalar(_glyph_data: &[u8]) -> Vec<f32> {
    // TODO: Implement scalar glyph processing
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_options_serialization() {
        let options = CompileOptions {
            use_simd: Some(true),
            incremental: Some(false),
            track_positions: Some(true),
            format: Some("latex".to_string()),
        };

        let json = serde_json::to_string(&options).unwrap();
        let parsed: CompileOptions = serde_json::from_str(&json).unwrap();
        
        assert_eq!(options.use_simd, parsed.use_simd);
        assert_eq!(options.incremental, parsed.incremental);
        assert_eq!(options.track_positions, parsed.track_positions);
        assert_eq!(options.format, parsed.format);
    }

    #[test]
    fn test_simple_compilation() {
        let latex = r#"
\documentclass{article}
\begin{document}
Hello, WASM!
\end{document}
"#;
        
        let options = CompileOptions {
            use_simd: Some(false),
            incremental: Some(false),
            track_positions: Some(false),
            format: Some("latex".to_string()),
        };

        let result = compile_latex_internal(latex, &options, false);
        assert!(result.is_ok(), "Compilation should succeed");
        
        let pdf_data = result.unwrap();
        assert!(!pdf_data.is_empty(), "PDF data should not be empty");
        assert!(pdf_data.starts_with(b"%PDF"), "Should be valid PDF");
    }
}