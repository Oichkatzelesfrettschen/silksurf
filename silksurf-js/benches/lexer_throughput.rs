//! Lexer throughput benchmark
//!
//! Target: 50-100 MB/s
//! Comparison: Boa ~25 MB/s, QuickJS ~40 MB/s

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use silksurf_js::Lexer;

/// Sample JavaScript code for benchmarking
const SAMPLE_JS: &str = r#"
function fibonacci(n) {
    if (n <= 1) return n;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

const result = fibonacci(30);
console.log(`Fibonacci(30) = ${result}`);

class Calculator {
    constructor(initial = 0) {
        this.value = initial;
    }

    add(x) {
        this.value += x;
        return this;
    }

    subtract(x) {
        this.value -= x;
        return this;
    }

    multiply(x) {
        this.value *= x;
        return this;
    }

    divide(x) {
        if (x === 0) throw new Error('Division by zero');
        this.value /= x;
        return this;
    }

    getResult() {
        return this.value;
    }
}

const calc = new Calculator(10);
const answer = calc.add(5).multiply(2).subtract(3).getResult();

// Arrow functions and destructuring
const numbers = [1, 2, 3, 4, 5];
const doubled = numbers.map(n => n * 2);
const [first, second, ...rest] = doubled;

// Async/await
async function fetchData(url) {
    const response = await fetch(url);
    const data = await response.json();
    return data;
}

// Template literals
const name = "World";
const greeting = `Hello, ${name}!`;

// Object spread and optional chaining
const obj = { a: 1, b: 2, c: { d: 3 } };
const newObj = { ...obj, e: 4 };
const value = obj?.c?.d ?? 0;

// Regular expressions
const pattern = /^[a-zA-Z]+$/;
const isValid = pattern.test("Hello");
"#;

fn lexer_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("lexer");

    // Small input
    let small = SAMPLE_JS;
    group.throughput(Throughput::Bytes(small.len() as u64));
    group.bench_with_input(BenchmarkId::new("small", small.len()), small, |b, input| {
        b.iter(|| {
            let lexer = Lexer::new(black_box(input));
            let tokens: Vec<_> = lexer.collect();
            black_box(tokens.len())
        });
    });

    // Medium input (10x)
    let medium = small.repeat(10);
    group.throughput(Throughput::Bytes(medium.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("medium", medium.len()),
        &medium,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Large input (100x)
    let large = small.repeat(100);
    group.throughput(Throughput::Bytes(large.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("large", large.len()),
        &large,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    group.finish();
}

fn token_types_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_types");

    // Keywords
    let keywords =
        "function const let var if else for while return class extends import export async await"
            .repeat(100);
    group.throughput(Throughput::Bytes(keywords.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("keywords", keywords.len()),
        &keywords,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Operators
    let operators =
        "=== !== => && || ?? ?. ... ++ -- += -= *= /= <<= >>= >>>= &= |= ^=".repeat(100);
    group.throughput(Throughput::Bytes(operators.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("operators", operators.len()),
        &operators,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Identifiers
    let identifiers =
        "foo bar baz qux quux corge grault garply waldo fred plugh xyzzy thud ".repeat(100);
    group.throughput(Throughput::Bytes(identifiers.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("identifiers", identifiers.len()),
        &identifiers,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Numbers
    let numbers = "42 3.14159 0xFF 0b1010 0o777 1e10 2.5e-3 123_456_789 0xDEAD_BEEF ".repeat(100);
    group.throughput(Throughput::Bytes(numbers.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("numbers", numbers.len()),
        &numbers,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    group.finish();
}

fn simd_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_optimized");

    // Line comments (SIMD via memchr2 for \n/\r)
    let line_comments = "x // This is a comment that goes to the end of the line\n".repeat(1000);
    group.throughput(Throughput::Bytes(line_comments.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("line_comments", line_comments.len()),
        &line_comments,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Block comments (SIMD via memchr3 for */\n/\r)
    let block_comments = "x /* This is a block comment with some content */ y ".repeat(500);
    group.throughput(Throughput::Bytes(block_comments.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("block_comments", block_comments.len()),
        &block_comments,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Long strings (SIMD via memchr3 for quote/\/\n)
    let long_strings =
        r#"const s = "This is a fairly long string literal with some content";"#.repeat(500);
    group.throughput(Throughput::Bytes(long_strings.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("long_strings", long_strings.len()),
        &long_strings,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Template literals (SIMD via memchr3 for `/\/\$)
    let templates = "const t = `Template string with ${expr} inside`;\n".repeat(500);
    group.throughput(Throughput::Bytes(templates.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("templates", templates.len()),
        &templates,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    // Mixed: heavily commented code
    let heavily_commented = r#"
// This is a heavily commented JavaScript file
// with lots of documentation
function example(x, y) { // inline comment
    /* Block comment explaining
       the logic below */
    const result = x + y; // another inline
    return result;
}
"#
    .repeat(200);
    group.throughput(Throughput::Bytes(heavily_commented.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("heavily_commented", heavily_commented.len()),
        &heavily_commented,
        |b, input| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(input));
                let tokens: Vec<_> = lexer.collect();
                black_box(tokens.len())
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    lexer_benchmark,
    token_types_benchmark,
    simd_benchmark
);
criterion_main!(benches);
