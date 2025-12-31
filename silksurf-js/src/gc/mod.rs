//! Memory management for `SilkSurfJS`
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    MEMORY SUBSYSTEMS                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Arena:      Bump allocator for short-lived data (AST)      │
//! │  Heap:       Mark-sweep GC for runtime objects              │
//! │  Generation: Safe indices with ABA protection               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! Performance target: 99% fewer allocations vs traditional GC

mod arena;
mod generation;
mod heap;
mod trace;
mod weakref;

pub use arena::{Arena, Generation};
pub use generation::GenerationalIndex;
pub use heap::{Color, GcHeader, GcRef, GcStats, Heap, TypeTag};
pub use trace::{CountingTracer, RootSet, Trace, Tracer};
pub use weakref::{
    FinalizationRegistry, FinalizerQueue, HeldValue, UnregisterToken, WeakEntry, WeakTable,
};
