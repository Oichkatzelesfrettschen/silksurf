# Archived Documentation

**Status**: READ-ONLY
**Updated**: 2026-01-29

This directory contains historical documentation that has been superseded by current specifications and roadmaps.

---

## Archive Structure

### `roadmaps/`

Historical planning documents consolidated into `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`:

- **PHASE-3-5-MILESTONES.md** (416 lines)
  - Original Phase 3-5 weekly breakdown
  - Team structure and acceptance criteria
  - Superseded by: `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

- **PHASE-3-SCOPE-AND-READINESS.md** (756 lines)
  - Phase 2 completion declaration
  - Architecture freeze documentation
  - Go/No-Go checklist
  - Superseded by: Current status in `/README.md` and `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

- **WEEK-1-PLAN.md** (36 lines)
  - Initial Week 1 task breakdown
  - Superseded by: Current status in `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

**Why archived:**
These documents represented the initial planning phase. As implementation progressed, a consolidated roadmap with current status updates became more useful than multiple historical planning documents.

---

## Current Documentation

For up-to-date information, see:

- `/DOCUMENTATION-INDEX.md` - Complete documentation map
- `/README.md` - Project overview and current implementation status
- `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` - Active roadmap with weekly milestones
- `/CLAUDE.md` - Engineering standards and policies

---

## Accessing Archived Content

All archived files remain in git history. To view:

```bash
# View file history
git log -- docs/archive/roadmaps/PHASE-3-5-MILESTONES.md

# View file at specific commit
git show <commit-hash>:PHASE-3-5-MILESTONES.md
```

---

## Archive Policy

Files are archived (not deleted) when:
1. Content is superseded by newer documentation
2. Multiple documents are consolidated into one
3. Planning documents are replaced by implementation reality
4. Historical context may be valuable for future reference

Files are kept in git for full historical traceability per cleanroom development principles.
