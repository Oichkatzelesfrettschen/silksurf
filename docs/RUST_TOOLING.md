# Rust Tooling and Support Crates

## Performance and Analysis (cargo subcommands)
- `cargo-valgrind`: run Valgrind via Cargo.
- `flamegraph`: generate flamegraphs (cargo subcommand).
- `cargo-bloat`: find binary size hotspots.
- `cargo-llvm-lines`: report LLVM IR line counts for size/opt analysis.
- `cargo-asm`: inspect generated assembly for hot functions.
- `cargo-udeps`: detect unused dependencies.
- `cargo-nextest`: faster, parallel test runner.

## Benchmarks
- `criterion`: statistics-driven micro-benchmarks.
- `iai-callgrind`: instruction-precise benchmarking via Callgrind.

## HTML/CSS/TLS Support (not full engines)
- `html5ever`: HTML5 tokenizer/parser.
- `cssparser`: CSS syntax parsing.
- `selectors`: CSS selector matching.
- `rustls`: TLS implementation.

These crates are optional; use only if they align with cleanroom goals.
