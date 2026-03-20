//! VM throughput benchmark
//!
//! Measures bytecode execution speed including:
//! - Arithmetic operations
//! - Control flow (jumps, conditionals)
//! - NaN-boxed value operations
//! - Property access

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use silksurf_js::bytecode::{Chunk, Instruction, Opcode};
use silksurf_js::vm::Vm;

/// Create a chunk that does N iterations of arithmetic
fn create_loop_chunk(iterations: u16) -> Chunk {
    let mut chunk = Chunk::new();

    // r0 = counter (start at iterations)
    chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, iterations as i16));
    // r1 = accumulator (start at 0)
    chunk.emit(Instruction::new_r(Opcode::LoadZero, 1));
    // r2 = 1 (for decrement)
    chunk.emit(Instruction::new_r(Opcode::LoadOne, 2));

    // Loop start (instruction 3):
    // r1 = r1 + r0
    chunk.emit(Instruction::new_rrr(Opcode::Add, 1, 1, 0));
    // r0 = r0 - 1
    chunk.emit(Instruction::new_rrr(Opcode::Sub, 0, 0, 2));
    // if r0 > 0, jump back to loop start (offset -2)
    chunk.emit(Instruction::new_r_offset(Opcode::JmpTrue, 0, -2));

    // return r1
    chunk.emit(Instruction::new_r(Opcode::Ret, 1));

    chunk
}

/// Create a chunk with intensive arithmetic (no loops, straight-line)
fn create_arithmetic_chunk(ops: usize) -> Chunk {
    let mut chunk = Chunk::new();

    // r0 = 1, r1 = 2
    chunk.emit(Instruction::new_r(Opcode::LoadOne, 0));
    chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 2));

    // Alternate between add/sub/mul operations
    for i in 0..ops {
        let dst = ((i % 8) + 2) as u8;
        let src1 = (i % 2) as u8;
        let src2 = ((i + 1) % 2) as u8;

        match i % 4 {
            0 => chunk.emit(Instruction::new_rrr(Opcode::Add, dst, src1, src2)),
            1 => chunk.emit(Instruction::new_rrr(Opcode::Sub, dst, src1, src2)),
            2 => chunk.emit(Instruction::new_rrr(Opcode::Mul, dst, src1, src2)),
            3 => chunk.emit(Instruction::new_rrr(Opcode::BitXor, dst, src1, src2)),
            _ => unreachable!(),
        };
    }

    // return r2
    chunk.emit(Instruction::new_r(Opcode::Ret, 2));
    chunk
}

/// Create a chunk with comparison operations
fn create_comparison_chunk(ops: usize) -> Chunk {
    let mut chunk = Chunk::new();

    // r0 = 10, r1 = 5
    chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, 10));
    chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 5));

    for i in 0..ops {
        let dst = ((i % 6) + 2) as u8;

        match i % 6 {
            0 => chunk.emit(Instruction::new_rrr(Opcode::Lt, dst, 0, 1)),
            1 => chunk.emit(Instruction::new_rrr(Opcode::Le, dst, 0, 1)),
            2 => chunk.emit(Instruction::new_rrr(Opcode::Gt, dst, 0, 1)),
            3 => chunk.emit(Instruction::new_rrr(Opcode::Ge, dst, 0, 1)),
            4 => chunk.emit(Instruction::new_rrr(Opcode::Eq, dst, 0, 1)),
            5 => chunk.emit(Instruction::new_rrr(Opcode::StrictEq, dst, 0, 1)),
            _ => unreachable!(),
        };
    }

    chunk.emit(Instruction::new_r(Opcode::Ret, 2));
    chunk
}

fn vm_loop_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_loop");

    for &iterations in &[100u16, 1000, 10000] {
        let chunk = create_loop_chunk(iterations);
        let ops = (iterations as u64) * 3; // 3 ops per iteration

        group.throughput(Throughput::Elements(ops));
        group.bench_with_input(BenchmarkId::new("iterations", iterations), &chunk, |b, chunk| {
            b.iter(|| {
                let mut vm = Vm::new();
                let idx = vm.add_chunk(chunk.clone());
                black_box(vm.execute(idx))
            });
        });
    }

    group.finish();
}

fn vm_arithmetic_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_arithmetic");

    for &ops in &[100usize, 1000, 10000] {
        let chunk = create_arithmetic_chunk(ops);

        group.throughput(Throughput::Elements(ops as u64));
        group.bench_with_input(BenchmarkId::new("ops", ops), &chunk, |b, chunk| {
            b.iter(|| {
                let mut vm = Vm::new();
                let idx = vm.add_chunk(chunk.clone());
                black_box(vm.execute(idx))
            });
        });
    }

    group.finish();
}

fn vm_comparison_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_comparison");

    for &ops in &[100usize, 1000, 10000] {
        let chunk = create_comparison_chunk(ops);

        group.throughput(Throughput::Elements(ops as u64));
        group.bench_with_input(BenchmarkId::new("ops", ops), &chunk, |b, chunk| {
            b.iter(|| {
                let mut vm = Vm::new();
                let idx = vm.add_chunk(chunk.clone());
                black_box(vm.execute(idx))
            });
        });
    }

    group.finish();
}

fn vm_dispatch_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_dispatch");

    // Measure pure dispatch overhead with NOPs
    for &ops in &[1000usize, 10000, 100000] {
        let mut chunk = Chunk::new();
        for _ in 0..ops {
            chunk.emit(Instruction::new(Opcode::Nop));
        }
        chunk.emit(Instruction::new_r(Opcode::LoadOne, 0));
        chunk.emit(Instruction::new_r(Opcode::Ret, 0));

        group.throughput(Throughput::Elements(ops as u64));
        group.bench_with_input(BenchmarkId::new("nops", ops), &chunk, |b, chunk| {
            b.iter(|| {
                let mut vm = Vm::new();
                let idx = vm.add_chunk(chunk.clone());
                black_box(vm.execute(idx))
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    vm_loop_benchmark,
    vm_arithmetic_benchmark,
    vm_comparison_benchmark,
    vm_dispatch_benchmark
);
criterion_main!(benches);
