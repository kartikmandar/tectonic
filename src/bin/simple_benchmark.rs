// Simple state capture performance benchmark
// Run with: cargo run --bin simple_benchmark

use std::time::Instant;

#[derive(Clone)]
struct MockState {
    memory: Vec<u8>,
    registers: [i32; 1000], 
    buffers: Vec<Vec<u8>>,
    timestamp: u64,
}

fn calculate_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    })
}

fn main() {
    println!("🚀 TeX Engine State Capture Performance Benchmark");
    println!("==================================================");
    
    // Create mock state similar to our TeX engine state
    let state = MockState {
        memory: vec![42u8; 1024 * 1024], // 1MB memory snapshot
        registers: [1; 1000],            // 4KB registers
        buffers: vec![
            vec![0u8; 256 * 1024],       // 256KB eqtb
            vec![0u8; 64 * 1024],        // 64KB input buffer  
            vec![0u8; 128 * 1024],       // 128KB font data
        ],
        timestamp: 1234567890,
    };
    
    let total_size = state.memory.len() + 
                    state.registers.len() * 4 +
                    state.buffers.iter().map(|b| b.len()).sum::<usize>();
    
    println!("📊 Test State Size: {:.1} MB", total_size as f64 / 1024.0 / 1024.0);
    
    // 1. Benchmark state cloning (simulates capture)
    let iterations = 100;
    println!("\n1️⃣  State Capture (Clone) Performance:");
    
    let start = Instant::now();
    for _ in 0..iterations {
        let _captured = state.clone();
    }
    let capture_duration = start.elapsed();
    let capture_avg = capture_duration / iterations;
    
    println!("   ⏱️  Average time: {:?}", capture_avg);
    println!("   🎯 Target: <5ms - {}", if capture_avg.as_millis() < 5 { "✅ PASS" } else { "❌ FAIL" });
    
    // 2. Benchmark individual memory operations
    println!("\n2️⃣  Memory Operations Performance:");
    
    // Memory allocation
    let start = Instant::now();
    for _ in 0..1000 {
        let _mem = vec![0u8; 512 * 1024]; // 512KB
    }
    let alloc_avg = start.elapsed() / 1000;
    println!("   💾 Memory alloc: {:?}", alloc_avg);
    
    // Checksum calculation
    let start = Instant::now();
    for _ in 0..100 {
        let _checksum = calculate_checksum(&state.memory);
    }
    let checksum_avg = start.elapsed() / 100;
    println!("   🔍 Checksum calc: {:?}", checksum_avg);
    
    // Register copy
    let start = Instant::now();
    for _ in 0..1000 {
        let _regs = state.registers;
    }
    let register_avg = start.elapsed() / 1000;
    println!("   📝 Register copy: {:?}", register_avg);
    
    // 3. Benchmark memory validation
    println!("\n3️⃣  Validation Performance:");
    
    let start = Instant::now();
    for _ in 0..100 {
        // Simulate state validation
        let _size_check = state.memory.len() > 0;
        let _reg_check = state.registers[0] != 0;
        let _buf_check = state.buffers.len() == 3;
        let _checksum = calculate_checksum(&state.memory[..1024]);
    }
    let validation_avg = start.elapsed() / 100;
    println!("   ✅ Validation: {:?}", validation_avg);
    
    // 4. Benchmark incremental operations
    println!("\n4️⃣  Incremental Operations:");
    
    // Partial memory copy (simulates incremental capture)
    let start = Instant::now();
    for _ in 0..1000 {
        let _partial = state.memory[..64*1024].to_vec(); // 64KB
    }
    let partial_avg = start.elapsed() / 1000;
    println!("   🔄 Partial capture: {:?}", partial_avg);
    
    // Memory comparison (change detection)
    let state2 = state.clone();
    let start = Instant::now();
    for _ in 0..100 {
        let _same = state.memory == state2.memory;
    }
    let compare_avg = start.elapsed() / 100;
    println!("   🔍 Change detection: {:?}", compare_avg);
    
    // 5. Summary
    println!("\n📋 Performance Summary:");
    println!("   📸 Full capture:    {:>8?} (target: <5ms)", capture_avg);
    println!("   💾 Memory alloc:    {:>8?} (target: <1ms)", alloc_avg);
    println!("   🔍 Checksum:        {:>8?} (target: <1ms)", checksum_avg);
    println!("   📝 Register copy:   {:>8?} (target: <100μs)", register_avg);
    println!("   ✅ Validation:      {:>8?} (target: <1ms)", validation_avg);
    println!("   🔄 Partial capture: {:>8?} (target: <500μs)", partial_avg);
    println!("   🔍 Change detect:   {:>8?} (target: <10ms)", compare_avg);
    
    // Performance analysis
    let capture_ok = capture_avg.as_millis() < 5;
    let alloc_ok = alloc_avg.as_micros() < 1000;
    let checksum_ok = checksum_avg.as_micros() < 1000;
    let register_ok = register_avg.as_micros() < 100;
    let validation_ok = validation_avg.as_micros() < 1000;
    let partial_ok = partial_avg.as_micros() < 500;
    let compare_ok = compare_avg.as_millis() < 10;
    
    let all_passed = capture_ok && alloc_ok && checksum_ok && 
                     register_ok && validation_ok && partial_ok && compare_ok;
    
    println!("\n🏁 Overall Result: {}", if all_passed { "✅ ALL TARGETS MET" } else { "❌ SOME TARGETS MISSED" });
    
    if !all_passed {
        println!("\n💡 Performance Analysis:");
        if !capture_ok {
            println!("   - Full state capture is too slow ({}ms)", capture_avg.as_millis());
            println!("     → Consider incremental capture strategies");
        }
        if !alloc_ok {
            println!("   - Memory allocation overhead is high ({}μs)", alloc_avg.as_micros());
            println!("     → Pre-allocate memory pools");
        }
        if !checksum_ok {
            println!("   - Checksum calculation is slow ({}μs)", checksum_avg.as_micros());
            println!("     → Use hardware CRC32 instructions");
        }
        if !compare_ok {
            println!("   - Change detection is slow ({}ms)", compare_avg.as_millis());
            println!("     → Implement incremental comparison");
        }
    }
    
    // Estimate real-world performance
    println!("\n🎯 Real-world Performance Estimates:");
    println!("   📄 Small document (1MB):   {:?}", capture_avg);
    println!("   📚 Medium document (10MB): {:?}", capture_avg * 10);
    println!("   📖 Large document (100MB): {:?}", capture_avg * 100);
    
    if capture_avg.as_millis() * 100 > 10 {
        println!("   ⚠️  Large documents may exceed 10ms target");
        println!("   💡 Implement size-based optimization strategies");
    }
}