// State capture performance benchmark
// Run with: cargo run --bin benchmark_state --no-default-features --features serialization

use std::time::Instant;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone)]
struct MockTeXEngineState {
    memory_snapshot: Vec<u8>,
    eqtb_data: Vec<u8>,
    buffer_data: Vec<u8>,
    font_data: Vec<u8>,
    integer_params: [i32; 1000],
    checksum: u32,
    timestamp: u64,
}

impl MockTeXEngineState {
    fn new_large() -> Self {
        Self {
            memory_snapshot: vec![0u8; 1024 * 1024], // 1MB
            eqtb_data: vec![0u8; 256 * 1024],        // 256KB  
            buffer_data: vec![0u8; 64 * 1024],       // 64KB
            font_data: vec![0u8; 128 * 1024],        // 128KB
            integer_params: [42; 1000],              // 4KB
            checksum: 0x12345678,
            timestamp: 1234567890,
        }
    }
}

fn calculate_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    })
}

fn main() {
    println!("🚀 TeX Engine State Capture Performance Benchmark");
    println!("==================================================");
    
    // Create large mock state
    let large_state = MockTeXEngineState::new_large();
    let state_size_mb = (large_state.memory_snapshot.len() + 
                        large_state.eqtb_data.len() + 
                        large_state.buffer_data.len() + 
                        large_state.font_data.len()) as f64 / 1024.0 / 1024.0;
    
    println!("📊 Test State Size: {:.1} MB", state_size_mb);
    
    // 1. Benchmark JSON serialization
    let iterations = 100;
    println!("\n1️⃣  JSON Serialization Performance:");
    
    let start = Instant::now();
    let mut total_size = 0;
    for _ in 0..iterations {
        let serialized = serde_json::to_vec(&large_state).unwrap();
        total_size = serialized.len();
    }
    let serialize_duration = start.elapsed();
    let serialize_avg = serialize_duration / iterations;
    
    println!("   ⏱️  Average time: {:?}", serialize_avg);
    println!("   📦 Serialized size: {:.1} MB", total_size as f64 / 1024.0 / 1024.0);
    println!("   🎯 Target: <5ms - {}", if serialize_avg.as_millis() < 5 { "✅ PASS" } else { "❌ FAIL" });
    
    // 2. Benchmark JSON deserialization
    let serialized = serde_json::to_vec(&large_state).unwrap();
    println!("\n2️⃣  JSON Deserialization Performance:");
    
    let start = Instant::now();
    for _ in 0..iterations {
        let _: MockTeXEngineState = serde_json::from_slice(&serialized).unwrap();
    }
    let deserialize_duration = start.elapsed();
    let deserialize_avg = deserialize_duration / iterations;
    
    println!("   ⏱️  Average time: {:?}", deserialize_avg);
    println!("   🎯 Target: <5ms - {}", if deserialize_avg.as_millis() < 5 { "✅ PASS" } else { "❌ FAIL" });
    
    // 3. Benchmark full round-trip
    println!("\n3️⃣  Round-trip Performance:");
    
    let start = Instant::now();
    for _ in 0..iterations {
        let serialized = serde_json::to_vec(&large_state).unwrap();
        let _: MockTeXEngineState = serde_json::from_slice(&serialized).unwrap();
    }
    let roundtrip_duration = start.elapsed();
    let roundtrip_avg = roundtrip_duration / iterations;
    
    println!("   ⏱️  Average time: {:?}", roundtrip_avg);
    println!("   🎯 Target: <5ms - {}", if roundtrip_avg.as_millis() < 5 { "✅ PASS" } else { "❌ FAIL" });
    
    // 4. Benchmark individual component capture
    println!("\n4️⃣  Individual Component Performance:");
    
    // Memory snapshot creation
    let start = Instant::now();
    for _ in 0..1000 {
        let _memory = vec![0u8; 512 * 1024]; // 512KB
    }
    let memory_avg = start.elapsed() / 1000;
    println!("   🧠 Memory snapshot: {:?}", memory_avg);
    
    // Checksum calculation
    let test_data = vec![0u8; 1024 * 1024];
    let start = Instant::now();
    for _ in 0..100 {
        let _checksum = calculate_checksum(&test_data);
    }
    let checksum_avg = start.elapsed() / 100;
    println!("   🔍 Checksum calc: {:?}", checksum_avg);
    
    // Memory allocation performance
    let start = Instant::now();
    for _ in 0..1000 {
        let _state = MockTeXEngineState {
            memory_snapshot: vec![0u8; 128 * 1024],
            eqtb_data: vec![0u8; 32 * 1024],
            buffer_data: vec![0u8; 16 * 1024],
            font_data: vec![0u8; 32 * 1024],
            integer_params: [0; 1000],
            checksum: 0,
            timestamp: 0,
        };
    }
    let allocation_avg = start.elapsed() / 1000;
    println!("   💾 State allocation: {:?}", allocation_avg);
    
    // 5. Summary
    println!("\n📋 Performance Summary:");
    println!("   📤 Serialize:     {:>8?} (target: <5ms)", serialize_avg);
    println!("   📥 Deserialize:   {:>8?} (target: <5ms)", deserialize_avg);
    println!("   🔄 Round-trip:    {:>8?} (target: <5ms)", roundtrip_avg);
    println!("   🧠 Memory ops:    {:>8?} (target: <1ms)", memory_avg);
    println!("   🔍 Checksum:      {:>8?} (target: <1ms)", checksum_avg);
    println!("   💾 Allocation:    {:>8?} (target: <1ms)", allocation_avg);
    
    let all_passed = serialize_avg.as_millis() < 5 && 
                     deserialize_avg.as_millis() < 5 && 
                     roundtrip_avg.as_millis() < 5 &&
                     memory_avg.as_micros() < 1000 &&
                     checksum_avg.as_micros() < 1000 &&
                     allocation_avg.as_micros() < 1000;
    
    println!("\n🏁 Overall Result: {}", if all_passed { "✅ ALL TARGETS MET" } else { "❌ SOME TARGETS MISSED" });
    
    if !all_passed {
        println!("\n💡 Optimization suggestions:");
        if serialize_avg.as_millis() >= 5 {
            println!("   - Consider using bincode instead of JSON for faster serialization");
        }
        if deserialize_avg.as_millis() >= 5 {
            println!("   - Implement incremental deserialization for large states");
        }
        if memory_avg.as_micros() >= 1000 {
            println!("   - Pre-allocate memory pools to reduce allocation overhead");
        }
    }
}