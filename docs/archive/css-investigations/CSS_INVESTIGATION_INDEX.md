# CSS_INVALID Investigation - Complete Documentation Index

**Investigation Date**: 2026-01-29
**Status**: Complete
**Scope**: Root cause analysis, architectural assessment, and design recommendations

## Quick Links to Investigation Documents

### 1. **CSS_INVESTIGATION_SUMMARY.md** - START HERE
**Purpose**: Executive overview and quick reference
**Read time**: 10-15 minutes
**Contains**:
- Quick answer to "what causes CSS_INVALID?"
- Is this a bug? Assessment
- Immediate workarounds
- Long-term solution overview
- File locations and line numbers

**Use case**: Get oriented, understand the problem at high level

---

### 2. **CSS_INVALID_INVESTIGATION.md** - DETAILED ANALYSIS
**Purpose**: Complete root cause analysis with evidence
**Read time**: 30-45 minutes
**Contains**:
- Exact code paths (with line numbers)
- How libcss's cascade algorithm works
- Why NetSurf doesn't have this problem
- LibCSS design limitations
- Architectural implications
- All code evidence
- Forward-looking design discussion

**Use case**: Understand the technical details, see all evidence

---

### 3. **MODERN_CSS_ENGINE_DESIGN.md** - ARCHITECTURAL SPECIFICATION
**Purpose**: Complete design specification for native CSS cascade engine
**Read time**: 45-60 minutes
**Contains**:
- Data structure specifications
- Property metadata system
- Complete cascade algorithm (with pseudocode)
- Integration points with DOM, layout, rendering
- Testing strategy
- Implementation roadmap (phases 1-4)
- Example code for property computation

**Use case**: Reference for building native CSS engine, architecture design

---

### 4. **CSS_FINDINGS_AND_RECOMMENDATIONS.md** - DECISION GUIDE
**Purpose**: Actionable recommendations with pros/cons
**Read time**: 20-30 minutes
**Contains**:
- Three solution options (evaluate all)
- Decision matrix comparing solutions
- Immediate actions (1-2 hours)
- Short-term actions (1-2 weeks)
- Long-term actions (weeks 3-6)
- Detailed implementation timelines
- Design philosophy alignment

**Use case**: Decide on solution path, plan implementation

---

## Investigation Results Summary

### Root Cause (TL;DR)
```
LibCSS's cascade algorithm calls ua_default_for_property() for properties 
that silksurf doesn't handle. Silksurf's handler only covers 4 properties 
(color, display, font-size, font-family) out of 60+. Returning CSS_INVALID 
for unhandled properties tells libcss the cascade failed entirely.

Location: /src/document/css_select_handler.c, line 673
```

### Assessment
**Is this a bug?** No, it's a design mismatch.
- Correct behavior per libcss specification
- NetSurf's own reference implementation also returns CSS_INVALID
- Problem is architectural: libcss designed for complete browsers, silksurf is minimal

### Recommended Solution Path

**Immediate** (1-2 hours):
- Add error recovery wrapper to gracefully handle CSS_INVALID
- Continue with current work unblocked

**Short-term** (optional):
- Expand handler to cover more properties

**Long-term** (weeks 3-6):
- Build native CSS cascade engine
- No external dependencies
- Full spec compliance
- Better error handling

## Reading Guide by Role

### For Managers/Architects
1. Read: CSS_INVESTIGATION_SUMMARY.md
2. Read: CSS_FINDINGS_AND_RECOMMENDATIONS.md
3. Decision: Choose solution path
4. Timeline: Plan phases

### For CSS Engine Developer
1. Read: CSS_INVALID_INVESTIGATION.md (understand problem)
2. Read: MODERN_CSS_ENGINE_DESIGN.md (detailed architecture)
3. Reference: CSS_FINDINGS_AND_RECOMMENDATIONS.md (implementation planning)
4. Start: Phase 1 implementation (property specs + basic cascade)

### For System Integrator
1. Read: CSS_INVESTIGATION_SUMMARY.md
2. Read: MODERN_CSS_ENGINE_DESIGN.md sections 4-5 (Integration Points)
3. Plan: How to hook CSS engine into DOM/layout
4. Test: Validate styles flow through rendering pipeline

### For QA/Tester
1. Read: MODERN_CSS_ENGINE_DESIGN.md section 8 (Testing Strategy)
2. Create: Unit test cases from spec
3. Create: Integration tests against real stylesheets
4. Validate: CSS computed values match expectations

## Key Findings

### Technical
- CSS_INVALID error is correct per libcss spec
- LibCSS assumes complete property coverage
- Silksurf's minimal approach conflicts with libcss architecture
- Native CSS engine would solve root problem

### Architectural
- LibCSS has handler callback model (tightly coupled)
- Modern approach: spec-driven with no external callbacks
- Cascade algorithm should be transparent and testable
- Per-property error handling is better than atomic failure

### Strategic
- Continuing with libcss requires expanding handler (boilerplate)
- Native engine aligns with silksurf's cleanroom design
- 4-6 week effort for complete, spec-compliant engine
- Long-term payoff: self-sufficient CSS implementation

## Code References

### Problem Code
- File: `/home/eirikr/Github/silksurf/src/document/css_select_handler.c`
- Lines: 641-677 (ua_default_for_property function)
- Line 673: `return CSS_INVALID;` (the trigger)

### Error Detection
- File: `/home/eirikr/Github/silksurf/src/document/css_engine.c`
- Lines: 337-350 (css_select_style call and error handling)
- Line 346: Error return logged
- Line 350: -1 returned on CSS_INVALID

### Property Coverage
- LibCSS defines 60+ properties in `/usr/include/libcss/properties.h`
- Silksurf handles only 4 (color, display, font-size, font-family)

## Document Statistics

| Document | Lines | Read Time | Purpose |
|----------|-------|-----------|---------|
| CSS_INVESTIGATION_SUMMARY.md | 250 | 10-15 min | Quick reference |
| CSS_INVALID_INVESTIGATION.md | 650 | 30-45 min | Detailed analysis |
| MODERN_CSS_ENGINE_DESIGN.md | 800 | 45-60 min | Architecture spec |
| CSS_FINDINGS_AND_RECOMMENDATIONS.md | 500 | 20-30 min | Decision guide |
| **Total** | **2200** | **2-3 hours** | Complete understanding |

## Implementation Checklist

### If Choosing Error Recovery (Solution 2)
- [ ] Modify css_engine.c around line 348
- [ ] Add CSS_INVALID handling with fallback style
- [ ] Add logging for transparency
- [ ] Update CLAUDE.md with note
- [ ] Test rendering with fallback
- [ ] Commit with explanation

**Effort**: 1-2 hours
**Blocks**: Nothing - allows continued work

### If Choosing Native Engine (Solution 3)
- [ ] Study MODERN_CSS_ENGINE_DESIGN.md
- [ ] Create property_spec.h with metadata table
- [ ] Implement cascade_core.c with main algorithm
- [ ] Add compute functions for properties
- [ ] Integrate with DOM element style storage
- [ ] Hook into layout engine
- [ ] Write test cases against CSS spec
- [ ] Performance profile and optimize

**Effort**: 4-6 weeks
**Blocks**: Nothing while Solution 2 provides fallback

## References & Sources

### Documents in This Investigation
- [CSS_INVESTIGATION_SUMMARY.md](./CSS_INVESTIGATION_SUMMARY.md)
- [CSS_INVALID_INVESTIGATION.md](./CSS_INVALID_INVESTIGATION.md)
- [MODERN_CSS_ENGINE_DESIGN.md](./MODERN_CSS_ENGINE_DESIGN.md)
- [CSS_FINDINGS_AND_RECOMMENDATIONS.md](./CSS_FINDINGS_AND_RECOMMENDATIONS.md)

### External Resources
- [NetSurf LibCSS Project](https://www.netsurf-browser.org/projects/libcss/)
- [NetSurf LibCSS Example](https://github.com/netsurf-browser/libcss/blob/master/examples/example1.c)
- [CSS Cascading and Inheritance Module Level 3](https://www.w3.org/TR/css-cascade-3/)
- [CSS Computed Values](https://www.w3.org/TR/css-values-3/#computed-value)

## Questions & Clarifications

**Q: Is this a critical bug?**
A: No. It's a design limitation. Rendering proceeds with workaround.

**Q: Should we replace libcss immediately?**
A: No. Use error recovery immediately, plan native engine for next phase.

**Q: How long will native engine take?**
A: 4-6 weeks for complete, spec-compliant implementation.

**Q: Can we use both approaches?**
A: Yes. Use error recovery now, build native engine in parallel.

**Q: What about CSS variables or advanced features?**
A: Native engine design supports them. Not in initial scope.

**Q: Will this affect existing rendered output?**
A: No with error recovery. Fallback provides reasonable defaults.

## Contact & Updates

This investigation is complete as of 2026-01-29.

All documentation is versioned and can be updated as decisions evolve.

To proceed, refer to **CSS_FINDINGS_AND_RECOMMENDATIONS.md** for next steps.
