use silksurf_js::parser::Parser;
use std::time::Instant;

fn main() {
    let source = r#"
function test(x, y) {
    let z = x + y * 2;
    return z;
}
const a = 1;
let b = 2;
"#;
    
    // Parse larger source
    let large_source: String = source.repeat(1000);
    let bytes = large_source.len();
    println!("Source size: {} bytes ({:.2} KB)", bytes, bytes as f64 / 1024.0);
    
    // Warm up
    for _ in 0..5 {
        let parser = Parser::new(&large_source);
        let _ = parser.parse();
    }
    
    // Benchmark
    let iterations = 20;
    let start = Instant::now();
    
    for _ in 0..iterations {
        let parser = Parser::new(&large_source);
        let (program, errors) = parser.parse();
        std::hint::black_box(&program);
        std::hint::black_box(&errors);
    }
    
    let elapsed = start.elapsed();
    let total_bytes = bytes * iterations;
    let throughput = total_bytes as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0;
    
    println!("Parsed {} iterations in {:?}", iterations, elapsed);
    println!("Average: {:?}/parse", elapsed / iterations as u32);
    println!("Throughput: {:.2} MB/s", throughput);
}
