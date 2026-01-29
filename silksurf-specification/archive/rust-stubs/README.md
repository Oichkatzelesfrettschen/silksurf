# Rust Tokenizer Stubs - Future Implementation

**Status**: ARCHIVED (Future Work)
**Date**: 2026-01-29

## Overview

These stub specifications were created for a potential future Rust-based HTML/CSS tokenizer implementation. They are currently **not in use** as SilkSurf uses the proven NetSurf libraries (libdom, libcss, libhubbub) written in C.

## Archived Files

1. **SILKSURF-CSS-DESIGN.md** (39 lines)
   - Rust CSS tokenizer design
   - Streaming input, 40+ MB/s target
   - Token model and API surface

2. **SILKSURF-HTML-DESIGN.md** (51 lines)
   - Rust HTML5 tokenizer design
   - Zero-copy tokenization, 50+ MB/s target
   - State machine outline

3. **SILKSURF-DEPENDENCY-STRATEGY.md** (28 lines)
   - Rust workspace dependency strategy
   - Arena allocators, string interning, SIMD scanning

## Current Implementation

SilkSurf currently uses:
- **libdom** - C-based DOM tree implementation
- **libcss** - C-based CSS parsing and selection
- **libhubbub** - C-based HTML5 parser

These libraries are mature, well-tested, and provide excellent performance.

## Future Considerations

If SilkSurf later implements custom tokenizers in Rust:
1. These specs provide a starting point
2. Performance targets: 40-50 MB/s streaming
3. Zero-copy and arena allocation strategies
4. Cleanroom implementation guidelines

## Why Archived

**Reason**: Focus on completing C-based implementation first
- Current: HTML/CSS parsing with proven C libraries ✓
- Current: CSS cascade and layout engine (in progress)
- Current: Rendering pipeline integration (planned)
- Future: Consider custom Rust tokenizers if performance bottlenecks appear

## References

**Active Specifications:**
- `../SILKSURF-C-CORE-DESIGN.md` - Current C implementation design
- `../SILKSURF-BUILD-SYSTEM-DESIGN.md` - CMake + Cargo integration
- `../SILKSURF-JS-DESIGN.md` - JavaScript engine (Rust)
