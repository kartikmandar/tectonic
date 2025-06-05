# Tectonic Codebase Analysis for WASM/SIMD Implementation

## 1. Main Entry Points in src/lib.rs

### Primary Functions
- **`latex_to_pdf<T: AsRef<str>>(latex: T) -> Result<Vec<u8>>`** (lines 144-184)
  - All-in-one interface for LaTeX to PDF compilation
  - Uses default bundle and configuration
  - Returns PDF as byte vector
  - Uses `ProcessingSessionBuilder` internally

### Key Modules
- **`config`** - Configuration management (PersistentConfig)
- **`driver`** - High-level interface for driving engines (ProcessingSessionBuilder)
- **`engines`** - Processing backends (bibtex, tex, xdvipdfmx, spx2html)
- **`io`** - I/O abstraction framework
- **`status`** - User-facing status reporting

### Important Re-exports
- `TexEngine`, `TexOutcome` from `engines::tex`
- `BibtexEngine`, `XdvipdfmxEngine`, `Spx2HtmlEngine`
- Error types from `errors` module
- `FORMAT_SERIAL` from `tectonic_engine_xetex`

### Critical for WASM Implementation
- The `latex_to_pdf` function shows the complete compilation workflow
- `ProcessingSessionBuilder` in driver module is key entry point
- Engine coordination happens through the driver module
- All engines use global mutex for thread safety (line 139-143)

## 2. Engine Core Analysis (src/engines/)

### Engine Structure
The `src/engines/` directory contains **wrapper modules** only:

#### src/engines/mod.rs
- Defines public interface for all engines
- Re-exports: `BibtexEngine`, `TexEngine`, `XdvipdfmxEngine`, `Spx2HtmlEngine`

#### src/engines/tex.rs (lines 1-14)
- **CRITICAL FINDING**: This is just a thin wrapper!
- Actual implementation is in `tectonic_engine_xetex` crate
- Re-exports: `TexEngine`, `TexOutcome` from external crate
- Only adds `DefinitelySame` trait implementation

#### src/engines/bibtex.rs, xdvipdfmx.rs, spx2html.rs
- Similar pattern - thin wrappers around external crates

### Key Insight for WASM
The actual TeX engine is NOT in `src/engines/tex.rs` - it's in the separate `crates/engine_xetex/` crate!

## 3. Actual TeX Engine Implementation (crates/engine_xetex/)

### Rust Interface (crates/engine_xetex/src/lib.rs)

#### Core Types
- **`TexEngine`** struct (lines 79-88)
  - Builder pattern interface
  - Configuration: `halt_on_error`, `initex_mode`, `synctex_enabled`, etc.
  - Uses global state variables in C code

#### Key Methods
- **`process()`** (lines 181-236)
  - Main compilation entry point
  - Takes: `CoreBridgeLauncher`, format file name, input file name
  - Uses global mutex for thread safety
  - Calls C function `tt_engine_xetex_main()`

#### C FFI Interface (lines 240-259)
```rust
extern "C" {
    fn tt_xetex_set_int_variable(var_name: *const c_char, value: c_int) -> c_int;
    fn tt_engine_xetex_main(api: &mut CoreBridgeState, ...) -> c_int;
}
```

### C Implementation Core Files

#### xetex-engine-interface.c (lines 1-100)
- **`tt_engine_xetex_main()`** - Main engine entry point
- **`tt_xetex_set_int_variable()`** - Configuration interface
- Uses `setjmp/longjmp` for error handling
- Calls `tt_run_engine()` internally

#### xetex-xetexd.h (lines 1-100)
- Defines critical `memory_word` union type (lines 99-100)
- Endianness-aware memory layout
- B32x2 and B16x4 structures for memory access
- **CRITICAL FOR STATE MANAGEMENT**

#### xetex-core.h
- Core type definitions
- `scaled_t` (32-bit integers)
- Font and selector enumerations

## 4. Memory Management Analysis

**CORRECTION**: The task mentioned `src/engines/tex/mem.rs` which doesn't exist. 
The actual memory management is split across multiple components:

### A. TeX Dynamic Memory (crates/xetex_format/src/mem.rs)
This handles TeX's internal memory structure for format files:

#### Key Components (lines 18-26)
- **`MemPointer`** (i32) - Memory address type
- **`Memory`** struct - Contains `Vec<u8>` for raw memory and `lo_mem_max` pointer
- **Compressed memory loading** (lines 48-74) - Efficient storage/retrieval
- **Memory word access** - Functions to read 32-bit values from byte arrays

#### Critical Functions
- **`parse()`** (lines 32-95) - Deserializes memory from format files
- **`decode_toklist()`** (lines 97-102) - Extracts token lists from memory
- Uses **endian-aware** memory word reading (`memword_read_b32_s0/s1`)

### B. I/O Memory Management (src/io/memory.rs)
This provides in-memory file system for WASM compatibility:

#### Key Components (lines 24-34)
- **`MemoryFileInfo`** - File data stored as `Vec<u8>` with modification time
- **`MemoryFileCollection`** - HashMap of filename → file data
- **`MemoryIoItem`** - Individual file handle with cursor state

#### Critical for WASM (lines 149-232)
- **`MemoryIo`** - IoProvider that works entirely in memory
- No disk I/O - perfect for WebAssembly environment
- Handles both input and output file operations
- Supports file truncation and modification tracking

### C. Low-Level Memory (C code)
The actual TeX engine memory management is in the C code:

### Memory Word Structure (xetex-xetexd.h lines 87-100)
```c
// Endian-aware memory layout
typedef union {
    b32x2 b32;  // Two 32-bit integers
    b16x4 b16;  // Four 16-bit integers  
    // Other fields for different access patterns
} memory_word;
```

### Key Memory Components
- **Global state variables** in C code (configured via `tt_xetex_set_int_variable`)
- **Memory allocation** using `xmalloc_array`, `xcalloc_array` macros
- **State snapshots** would need to capture C global state
- **Pascal-style indexing** (size + 1 allocation pattern)

### Critical for Incremental Compilation
- Need to hook into memory allocation routines
- Must capture/restore entire C global state
- Memory words are the fundamental storage unit
- Endianness matters for state serialization

## Implementation Strategy for WASM/SIMD

### Key Findings
1. **Engine is C-based**: Core logic in XeTeX C code, not Rust
2. **Global state**: Heavy use of C global variables
3. **FFI boundary**: Rust provides safe interface to unsafe C code
4. **Thread safety**: Global mutex prevents concurrent execution

### WASM Modifications Needed
1. **Cargo.toml**: Add wasm-bindgen, web-sys dependencies
2. **Memory management**: Hook into C allocation routines  
3. **State capture**: Serialize C global state for snapshots
4. **SIMD**: Add WebAssembly SIMD intrinsics to hot paths
5. **I/O abstraction**: Replace file I/O with memory operations

### Next Steps
- Modify `crates/engine_xetex/Cargo.toml` for WASM targets
- Add state capture hooks in `xetex-engine-interface.c`
- Implement memory word serialization for incremental compilation
- Add WASM-specific build configuration