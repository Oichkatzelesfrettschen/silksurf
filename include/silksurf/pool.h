#ifndef SILKSURF_POOL_H
#define SILKSURF_POOL_H

#include <stddef.h>

/* Object pooling - reuse frequently allocated objects */

typedef struct silk_pool silk_pool_t;

/* Create pool with fixed object size and capacity */
silk_pool_t *silk_pool_create(size_t object_size, size_t capacity);
void silk_pool_destroy(silk_pool_t *pool);

/* Acquire object from pool (reuses existing or allocates new) */
void *silk_pool_acquire(silk_pool_t *pool);

/* Release object back to pool for reuse */
void silk_pool_release(silk_pool_t *pool, void *obj);

/* Statistics and control */
size_t silk_pool_available(silk_pool_t *pool);
size_t silk_pool_used(silk_pool_t *pool);
void silk_pool_reset(silk_pool_t *pool);
void silk_pool_stats(silk_pool_t *pool);

#endif
