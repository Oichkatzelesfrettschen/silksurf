#ifndef SILKSURF_ALLOCATOR_H
#define SILKSURF_ALLOCATOR_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

/* Branch Prediction Intrinsics */
#define LIKELY(x)    __builtin_expect(!!(x), 1)
#define UNLIKELY(x)  __builtin_expect(!!(x), 0)

/* Arena allocator - ultra-fast, cache-friendly memory management */

typedef struct silk_arena silk_arena_t;

/* Create and destroy arena */
silk_arena_t *silk_arena_create(size_t size);
void silk_arena_destroy(silk_arena_t *arena);

/* Core allocation */
void *silk_arena_alloc(silk_arena_t *arena, size_t size);
void *silk_arena_calloc(silk_arena_t *arena, size_t count, size_t size);

/* Batch operations (O(1)) */
void silk_arena_reset(silk_arena_t *arena);  /* Clear all allocations */
void silk_arena_checkpoint(silk_arena_t *arena);  /* Save current offset */
void silk_arena_rollback(silk_arena_t *arena);  /* Rewind to checkpoint */

/* Statistics */
size_t silk_arena_used(silk_arena_t *arena);
size_t silk_arena_available(silk_arena_t *arena);
size_t silk_arena_highwater(silk_arena_t *arena);
void silk_arena_stats(silk_arena_t *arena);

/* Aligned allocation for SIMD operations */
void *silk_arena_alloc_aligned(silk_arena_t *arena, size_t size, size_t alignment);

#endif
