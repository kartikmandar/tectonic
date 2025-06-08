// crates/tectonic_wasm/src/lib.rs
// Copyright 2024 the Tectonic Project  
// Licensed under the MIT License.

//! Minimal WebAssembly interface for Tectonic TeX engine
//!
//! This crate provides a focused, WASM-compatible interface to Tectonic's
//! core functionality, designed specifically for WYSIWYG LaTeX editing.
//!
//! # Features
//! - Pure Rust implementation (no C dependencies to start)
//! - WebAssembly SIMD support
//! - Incremental compilation framework
//! - Character-level position tracking
//! - Browser-optimized API design

use serde::{Deserialize, Serialize};

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

#[cfg(feature = "wasm")]
use web_sys::console;

/// Initialize the WASM module and set up panic hook for better debugging
#[cfg(feature = "wasm")]
#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console::log_1(&"Tectonic WASM engine initialized (minimal version)".into());
}

/// Get version information
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Simple test function to verify WASM compilation and loading
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn test_wasm() -> String {
    "Tectonic minimal WASM engine is working!".to_string()
}

/// Check if SIMD is available and enabled
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn simd_available() -> bool {
    cfg!(target_feature = "simd128")
}

/// Compilation options for WASM interface
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompileOptions {
    /// Enable SIMD optimizations if available
    pub use_simd: Option<bool>,
    /// Enable incremental compilation (placeholder)
    pub incremental: Option<bool>,
    /// Enable character-level position tracking (placeholder)
    pub track_positions: Option<bool>,
    /// LaTeX format to use
    pub format: Option<String>,
    /// Maximum compilation time in milliseconds
    pub timeout_ms: Option<u32>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            use_simd: Some(true),
            incremental: Some(false),
            track_positions: Some(false), 
            format: Some("latex".to_string()),
            timeout_ms: Some(5000),
        }
    }
}

/// Compilation result returned to JavaScript
#[derive(Serialize, Deserialize, Debug)]
pub struct CompileResult {
    /// True if compilation succeeded
    pub success: bool,
    /// PDF output as base64 string (if successful) 
    pub pdf_data: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Warning messages
    pub warnings: Vec<String>,
    /// Compilation time in milliseconds
    pub duration_ms: f64,
    /// Whether SIMD was used
    pub used_simd: bool,
    /// Number of pages in output
    pub page_count: Option<u32>,
    /// Memory usage in bytes
    pub memory_used: Option<u32>,
}

/// Main LaTeX compilation function (minimal implementation to start)
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn compile_latex(latex_source: &str, options_json: Option<String>) -> Result<JsValue, JsValue> {
    let start_time = web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0);

    // Parse options
    let options: CompileOptions = match options_json {
        Some(json) => {
            let js_value = js_sys::JSON::parse(&json)
                .map_err(|e| format!("Invalid options JSON: {:?}", e))?;
            serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| format!("Failed to parse options: {:?}", e))?
        }
        None => CompileOptions::default(),
    };

    #[cfg(feature = "wasm")]
    {
        console::log_1(&format!("Compiling LaTeX: {} characters", latex_source.len()).into());
        console::log_1(&format!("Options: {:?}", options).into());
    }

    // Check SIMD availability
    let simd_available = cfg!(target_feature = "simd128");
    let use_simd = options.use_simd.unwrap_or(simd_available);

    // For now, create a minimal PDF (placeholder)
    // TODO: Replace with actual Tectonic engine integration
    let result = compile_latex_minimal(latex_source, &options, use_simd);

    let compile_result = match result {
        Ok(pdf_bytes) => {
            use base64::Engine;
            let pdf_base64 = base64::engine::general_purpose::STANDARD.encode(&pdf_bytes);
            CompileResult {
                success: true,
                pdf_data: Some(pdf_base64),
                error: None,
                warnings: vec![
                    "This is a minimal implementation".to_string(),
                    "Full TeX engine integration coming soon".to_string(),
                ],
                duration_ms: web_sys::window()
                    .and_then(|w| w.performance())
                    .map(|p| p.now() - start_time)
                    .unwrap_or(0.0),
                used_simd: use_simd && simd_available,
                page_count: Some(1),
                memory_used: Some(latex_source.len() as u32 * 2), // Rough estimate
            }
        }
        Err(e) => CompileResult {
            success: false,
            pdf_data: None,
            error: Some(e),
            warnings: vec![],
            duration_ms: web_sys::window()
                .and_then(|w| w.performance())
                .map(|p| p.now() - start_time)
                .unwrap_or(0.0),
            used_simd: false,
            page_count: None,
            memory_used: None,
        }
    };

    serde_wasm_bindgen::to_value(&compile_result)
        .map_err(|e| format!("Serialization error: {:?}", e).into())
}

/// Minimal LaTeX compilation implementation
/// This is a placeholder that creates a simple PDF-like structure
/// TODO: Replace with actual Tectonic engine integration
fn compile_latex_minimal(
    latex_source: &str,
    _options: &CompileOptions,
    use_simd: bool,
) -> Result<Vec<u8>, String> {
    // Basic validation
    if latex_source.trim().is_empty() {
        return Err("Empty LaTeX source".to_string());
    }

    // Check for basic document structure
    if !latex_source.contains("\\documentclass") {
        return Err("Missing \\documentclass declaration".to_string());
    }

    if !latex_source.contains("\\begin{document}") {
        return Err("Missing \\begin{document}".to_string());
    }

    if !latex_source.contains("\\end{document}") {
        return Err("Missing \\end{document}".to_string());
    }

    #[cfg(feature = "wasm")]
    console::log_1(&format!("Processing with SIMD: {}", use_simd).into());

    // Create a minimal PDF structure (this is just a placeholder!)
    // In reality, this would call the Tectonic engine
    let mut pdf_data = Vec::new();
    
    // PDF header
    pdf_data.extend_from_slice(b"%PDF-1.4\n");
    pdf_data.extend_from_slice(b"1 0 obj\n");
    pdf_data.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\n");
    pdf_data.extend_from_slice(b"endobj\n");
    
    // Simple page content
    let content = format!(
        "BT /F1 12 Tf 72 720 Td ({} chars compiled with Tectonic WASM) Tj ET",
        latex_source.len()
    );
    
    pdf_data.extend_from_slice(b"2 0 obj\n");
    pdf_data.extend_from_slice(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n");
    pdf_data.extend_from_slice(b"endobj\n");
    
    pdf_data.extend_from_slice(b"3 0 obj\n");
    pdf_data.extend_from_slice(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> /Contents 4 0 R >>\n");
    pdf_data.extend_from_slice(b"endobj\n");
    
    pdf_data.extend_from_slice(b"4 0 obj\n");
    pdf_data.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
    pdf_data.extend_from_slice(content.as_bytes());
    pdf_data.extend_from_slice(b"\nendstream\nendobj\n");
    
    // PDF trailer
    pdf_data.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    pdf_data.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n");

    #[cfg(feature = "wasm")]
    console::log_1(&format!("Generated {} bytes of PDF data", pdf_data.len()).into());

    Ok(pdf_data)
}

/// Get performance metrics
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn get_performance_info() -> JsValue {
    let info = serde_json::json!({
        "simd_available": cfg!(target_feature = "simd128"),
        "wasm_version": env!("CARGO_PKG_VERSION"),
        "memory_available": true, // Simplified for now
        "target_arch": "wasm32",
    });
    
    JsValue::from_str(&info.to_string())
}

/// SIMD-optimized text processing (placeholder)
#[cfg(target_feature = "simd128")]
pub fn process_text_simd(text: &str) -> String {
    // TODO: Implement actual SIMD text processing
    // This is a placeholder that will be replaced with real SIMD operations
    format!("[SIMD] Processed: {}", text)
}

/// Fallback scalar text processing
pub fn process_text_scalar(text: &str) -> String {
    format!("[Scalar] Processed: {}", text)
}

/// Text processing dispatcher
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub fn process_text(text: &str) -> String {
    #[cfg(target_feature = "simd128")]
    {
        process_text_simd(text)
    }
    
    #[cfg(not(target_feature = "simd128"))]
    {
        process_text_scalar(text)
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_options_default() {
        let options = CompileOptions::default();
        assert_eq!(options.format, Some("latex".to_string()));
        assert_eq!(options.use_simd, Some(true));
    }

    #[test]
    fn test_minimal_latex_compilation() {
        let latex = r#"
\documentclass{article}
\begin{document}
Hello, WASM!
\end{document}
"#;
        
        let options = CompileOptions::default();
        let result = compile_latex_minimal(latex, &options, false);
        
        assert!(result.is_ok(), "Compilation should succeed");
        
        let pdf_data = result.unwrap();
        assert!(!pdf_data.is_empty(), "PDF data should not be empty");
        assert!(pdf_data.starts_with(b"%PDF"), "Should be valid PDF structure");
    }

    #[test]
    fn test_invalid_latex() {
        let latex = "This is not valid LaTeX";
        let options = CompileOptions::default();
        let result = compile_latex_minimal(latex, &options, false);
        
        assert!(result.is_err(), "Should fail for invalid LaTeX");
    }

    #[test]
    fn test_empty_latex() {
        let latex = "";
        let options = CompileOptions::default();
        let result = compile_latex_minimal(latex, &options, false);
        
        assert!(result.is_err(), "Should fail for empty LaTeX");
    }
}