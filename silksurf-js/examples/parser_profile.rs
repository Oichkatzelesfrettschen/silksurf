use std::time::Instant;

use silksurf_js::parser::{AstArena, Parser};

fn main() {
    let source = r#"
function test(x, y) {
    let z = x + y * 2;
    return z;
}
const a = 1;
let b = 2;
class Foo {
    constructor(name) {
        this.name = name;
    }
    greet() {
        return "Hello, " + this.name;
    }
}
if (true) {
    let x = 10;
    while (x > 0) {
        x = x - 1;
    }
}
"#;

    // Parse larger source for profiling
    let large_source: String = source.repeat(2000);
    let bytes = large_source.len();
    println!(
        "Source size: {} bytes ({:.2} KB)",
        bytes,
        bytes as f64 / 1024.0
    );

    let arena = AstArena::new();

    // Warm up
    for _ in 0..10 {
        {
            let parser = Parser::new(&large_source, &arena);
            let _ = parser.parse();
        }
        arena.reset();
    }

    // Profile run
    let iterations = 100;
    let start = Instant::now();

    for _ in 0..iterations {
        {
            let parser = Parser::new(&large_source, &arena);
            let (program, errors) = parser.parse();
            std::hint::black_box(&program);
            std::hint::black_box(&errors);
        }
        arena.reset();
    }

    let elapsed = start.elapsed();
    let total_bytes = bytes * iterations;
    let throughput = total_bytes as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0;

    println!("Parsed {} iterations in {:?}", iterations, elapsed);
    println!("Average: {:?}/parse", elapsed / iterations as u32);
    println!("Throughput: {:.2} MB/s", throughput);
}
