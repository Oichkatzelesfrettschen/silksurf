# CSS_INVALID Investigation: Executive Summary

**Investigation Date**: 2026-01-29
**Status**: Complete - Root Cause Identified and Design Documents Created
**Key Finding**: Not a bug, architectural design mismatch

## Quick Answer

**What causes CSS_INVALID?**
The `ua_default_for_property()` handler in `/home/eirikr/Github/silksurf/src/document/css_select_handler.c` (lines 671-673) returns CSS_INVALID when libcss asks for defaults for CSS properties that silksurf doesn't handle.

**Code location**:
```c
default:
    fprintf(stderr, "[CSS Handler] ua_default_for_property: property %u not handled, returning CSS_INVALID\n", property);
    return CSS_INVALID;  // Line 673
```

**Why it happens**:
- Silksurf handles only 4 properties: color, display, font-size, font-family
- libcss's cascade algorithm requests defaults for 60+ properties during style computation
- When a property isn't in the switch statement, CSS_INVALID is returned
- libcss interprets CSS_INVALID as "cascade failure"

## Is This a Bug?

**No.** This is correct behavior per libcss specification. The problem is **architectural mismatch**:

- **LibCSS was designed for NetSurf** (a complete browser), which provides comprehensive property defaults
- **Silksurf is trying to use libcss minimally**, but libcss assumes complete handler coverage
- **NetSurf's own reference implementation also returns CSS_INVALID** for unhandled properties

## Investigation Deliverables

### 1. CSS_INVALID_INVESTIGATION.md
**Location**: `/home/eirikr/Github/silksurf/CSS_INVALID_INVESTIGATION.md`

Comprehensive root cause analysis covering:
- Exact code paths that trigger the error
- How libcss's cascade algorithm works
- Why NetSurf doesn't have this problem
- Design limitations of libcss's handler callback model
- Immediate workarounds (if staying with libcss)
- Long-term architectural recommendations

**Key insights**:
- LibCSS design assumes atomicity: all-or-nothing cascade (can't return partial results)
- CSS_INVALID means "one property failed, entire cascade fails"
- Modern CSS engines should be error-resilient: compute all properties regardless of individual failures

### 2. MODERN_CSS_ENGINE_DESIGN.md
**Location**: `/home/eirikr/Github/silksurf/MODERN_CSS_ENGINE_DESIGN.md`

Complete specification for a cleanroom CSS cascade engine designed for SilkSurf:

**Core design principles**:
- Every CSS property has explicit initial value from spec
- Cascade algorithm is pure data transformation (no callbacks for property logic)
- Per-property error handling (one property failure doesn't break cascade)
- Full transparency: track where each property value came from

**Architecture**:
- Selector matching (separate module) provides matched rules
- Cascade engine (this component) applies cascade algorithm
- Per-property computation functions handle unit conversion, inheritance, etc.
- Layout and rendering engines use computed styles

**Benefits**:
- No handler callback dependencies
- Spec-compliant CSS Cascading and Inheritance Module Level 3
- Better error handling and debugging
- Easier to test and optimize independently
- Aligns with browser architecture and design goals

**Key sections**:
1. Data structures (css_computed_style, css_cascade_context)
2. Property specification table (metadata for all CSS properties)
3. Cascade algorithm (origin, specificity, inheritance)
4. Integration points (HTML parser, layout, rendering)
5. Testing strategy and implementation roadmap

## Recommended Path Forward

### Short Term (If staying with LibCSS)

1. **Add error recovery to css_select_style() wrapper**:
   ```c
   css_error err = css_select_style(...);
   if (err == CSS_INVALID) {
       // Fall back to minimal style: display: block, default colors, etc.
       return compute_fallback_style(...);
   }
   ```

2. **Expand ua_default_for_property() to handle more properties** (200+ lines of boilerplate, not recommended long-term)

3. **Document the limitation** in CLAUDE.md

### Long Term (Recommended)

**Implement native CSS cascade engine** following the MODERN_CSS_ENGINE_DESIGN.md specification:

**Phase 1: Foundation** (1-2 weeks)
- Implement property specification table
- Core cascade algorithm
- Basic compute functions

**Phase 2: Coverage** (1-2 weeks)
- Add all CSS properties to spec table
- Implement all compute functions
- Unit conversion (em, rem, %, etc.)

**Phase 3: Integration** (1 week)
- Hook into DOM tree style computation
- Cache computed styles
- Integrate with layout engine

**Phase 4: Optimization** (1 week)
- Profile and optimize cascade
- Implement selector matching indexing
- Performance benchmarks

**Total effort**: ~4-6 weeks for a complete, spec-compliant CSS engine

**Payoff**:
- No external CSS library dependency
- Full control over spec compliance
- Better error handling
- Cleaner architecture
- Easier to extend

## Technical Details

### LibCSS Limitations

1. **Handler callback model**
   - Property logic delegated to callbacks
   - Every unhandled property breaks cascade
   - Tight coupling between libcss and handler

2. **Atomic cascade**
   - Can't return partial results
   - One missing property = entire cascade fails
   - No graceful degradation

3. **Black-box algorithm**
   - Cascade computation happens inside libcss
   - Hard to debug when things go wrong
   - Can't customize cascade behavior

4. **NetSurf-centric design**
   - Assumptions about DOM structure
   - Handler must provide comprehensive defaults
   - Not suitable for minimal/embedded use

### Modern CSS Engine Advantages

1. **Spec-driven**
   - Each property defined with initial value, inheritance rule
   - Cascade algorithm explicit in code
   - Testable against CSS spec

2. **Handler-free**
   - No callbacks for property logic
   - DOM traversal done internally
   - No "handler must provide X" assumptions

3. **Error resilient**
   - Per-property error tracking
   - Partial results if some properties fail
   - Never atomic failure

4. **Transparent**
   - Can trace exactly where each property value came from
   - Debug output shows cascade steps
   - Works for all property combinations

5. **Optimizable**
   - Selector matching separate from cascade
   - Can cache intermediate results
   - Can parallelize rule matching
   - Can lazy-evaluate computed values

## Code Evidence

### Evidence 1: The Problematic Function
File: `/home/eirikr/Github/silksurf/src/document/css_select_handler.c` (lines 641-677)
- Only handles 4 properties in switch statement
- Returns CSS_INVALID for all others

### Evidence 2: Error Detection
File: `/home/eirikr/Github/silksurf/src/document/css_engine.c` (lines 337-350)
- Calls css_select_style()
- Treats CSS_INVALID as fatal error
- Returns -1 to caller

### Evidence 3: LibCSS Design
File: `/usr/include/libcss/properties.h`
- Defines 60+ CSS properties (0x000 to 0x3b)
- Handler only covers 4

### Evidence 4: NetSurf Reference
From `examples/example1.c` in libcss source:
- Also returns CSS_INVALID for unhandled properties
- Complete browser implementation doesn't fail because all properties are in UA stylesheet

## Files Created

1. **CSS_INVALID_INVESTIGATION.md** - 600+ lines of detailed root cause analysis
2. **MODERN_CSS_ENGINE_DESIGN.md** - 800+ lines of complete engine specification
3. **CSS_INVESTIGATION_SUMMARY.md** - This document

## References

- [NetSurf LibCSS Project](https://www.netsurf-browser.org/projects/libcss/)
- [NetSurf LibCSS Example](https://github.com/netsurf-browser/libcss/blob/master/examples/example1.c)
- [CSS Cascading and Inheritance Module Level 3](https://www.w3.org/TR/css-cascade-3/)
- [CSS Computed Values](https://www.w3.org/TR/css-values-3/#computed-value)

## Conclusion

The CSS_INVALID error is **not a bug in silksurf's implementation**—it is a **natural consequence of using libcss minimally**. LibCSS was designed for complete browser implementations where the handler provides comprehensive property defaults.

**Two paths forward**:

1. **Quick fix** (1-2 hours): Add error recovery, document limitation
2. **Proper solution** (4-6 weeks): Build native CSS cascade engine aligned with silksurf's cleanroom design philosophy

The proper solution is strongly recommended, as it:
- Aligns with silksurf's design goals
- Provides better control and debugging
- Avoids ongoing maintenance burden of libcss
- Enables custom optimizations
- Ensures spec compliance

All necessary architectural guidance is provided in MODERN_CSS_ENGINE_DESIGN.md.
