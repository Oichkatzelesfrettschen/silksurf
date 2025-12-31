#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "silksurf/allocator.h"

/* Arena allocator: ultra-fast O(1) allocation with batch deallocation */

struct silk_arena {
    uint8_t *base;           /* Start of arena */
    size_t total_size;       /* Total capacity */
    size_t offset;           /* Current allocation offset */
    size_t highwater;        /* Peak usage */
    size_t checkpoint;       /* Saved offset for rollback */
    size_t alloc_count;      /* Number of successful allocations */
};

silk_arena_t *silk_arena_create(size_t size) {
    silk_arena_t *arena = malloc(sizeof(silk_arena_t));
    if (!arena)
        return NULL;

    arena->base = malloc(size);
    if (!arena->base) {
        free(arena);
        return NULL;
    }

    arena->total_size = size;
    arena->offset = 0;
    arena->highwater = 0;
    arena->checkpoint = 0;
    arena->alloc_count = 0;

    return arena;
}

void silk_arena_destroy(silk_arena_t *arena) {
    if (!arena)
        return;
    if (arena->base)
        free(arena->base);
    free(arena);
}

void *silk_arena_alloc(silk_arena_t *arena, size_t size) {
    if (UNLIKELY(!arena || !size))
        return NULL;

    /* Align to 8 bytes using fast bitwise math */
    size_t aligned_offset = (arena->offset + 7) & ~7;

    /* Check bounds with branch hint */
    if (UNLIKELY(aligned_offset + size > arena->total_size))
        return NULL;

    /* Allocate by bumping pointer */
    void *ptr = arena->base + aligned_offset;
    arena->offset = aligned_offset + size;
    arena->alloc_count++;

    /* Prefetch the next block into L1 cache */
    __builtin_prefetch(arena->base + arena->offset, 1, 3);

    /* Track high water mark (optimized out in non-debug builds usually) */
    if (UNLIKELY(arena->offset > arena->highwater))
        arena->highwater = arena->offset;

    return ptr;
}

void *silk_arena_calloc(silk_arena_t *arena, size_t count, size_t size) {
    size_t total = count * size;
    void *ptr = silk_arena_alloc(arena, total);
    if (ptr)
        memset(ptr, 0, total);
    return ptr;
}

/* Aligned allocation for SIMD operations (64-byte typical) */
void *silk_arena_alloc_aligned(silk_arena_t *arena, size_t size, size_t alignment) {
    if (!arena || !size || !alignment)
        return NULL;

    /* Round up offset to alignment boundary */
    size_t aligned_offset = (arena->offset + alignment - 1) & ~(alignment - 1);
    size_t padding = aligned_offset - arena->offset;

    /* Check bounds */
    if (aligned_offset + size > arena->total_size)
        return NULL;

    /* Zero out padding for security/hygiene */
    if (padding > 0) {
        memset(arena->base + arena->offset, 0, padding);
    }

    void *ptr = arena->base + aligned_offset;
    arena->offset = aligned_offset + size;

    /* Track high water mark */
    if (arena->offset > arena->highwater)
        arena->highwater = arena->offset;

    return ptr;
}

/* Save current offset for nested allocations or rollback */
void silk_arena_checkpoint(silk_arena_t *arena) {
    if (!arena)
        return;
    arena->checkpoint = arena->offset;
}

/* Rewind to checkpoint (batch deallocation) */
void silk_arena_rollback(silk_arena_t *arena) {
    if (!arena)
        return;
    arena->offset = arena->checkpoint;
}

/* Clear entire arena (O(1) deallocation of all) */
void silk_arena_reset(silk_arena_t *arena) {
    if (UNLIKELY(!arena))
        return;
    
    __atomic_store_n(&arena->offset, 0, __ATOMIC_RELAXED);
    __atomic_store_n(&arena->checkpoint, 0, __ATOMIC_RELAXED);
    __atomic_store_n(&arena->alloc_count, 0, __ATOMIC_RELAXED);
}

/* Query usage */
size_t silk_arena_used(silk_arena_t *arena) {
    return arena ? arena->offset : 0;
}

size_t silk_arena_available(silk_arena_t *arena) {
    return arena ? (arena->total_size - arena->offset) : 0;
}

size_t silk_arena_highwater(silk_arena_t *arena) {
    return arena ? arena->highwater : 0;
}

/* Print statistics */
void silk_arena_stats(silk_arena_t *arena) {
    if (!arena)
        return;

    size_t used = arena->offset;
    size_t total = arena->total_size;
    size_t peak = arena->highwater;
    double usage_pct = (100.0 * used) / total;

    printf("\n[SILKSURF MEMORY REPORT]\n");
    printf("----------------------------------------\n");
    printf("  Arena Capacity:  %10zu bytes (%zu KB)\n", total, total / 1024);
    printf("  Current Usage:   %10zu bytes (%.2f%%)\n", used, usage_pct);
    printf("  Peak Usage:      %10zu bytes (%zu KB)\n", peak, peak / 1024);
    printf("  Total Allocs:    %10zu\n", arena->alloc_count);
    if (arena->alloc_count > 0) {
        printf("  Avg Alloc Size:  %10zu bytes\n", used / arena->alloc_count);
    }
    printf("----------------------------------------\n\n");
}
