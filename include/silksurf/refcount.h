#ifndef SILKSURF_REFCOUNT_H
#define SILKSURF_REFCOUNT_H

#include <stdint.h>

/* Reference counting for shared objects */

typedef struct {
    uint32_t count;
} silk_refcount_t;

/* Initialize reference count to 1 (owned by creator) */
static inline void silk_refcount_init(silk_refcount_t *rc) {
    rc->count = 1;
}

/* Increment reference count (acquire) */
static inline uint32_t silk_refcount_inc(silk_refcount_t *rc) {
    return ++rc->count;
}

/* Decrement reference count (release) */
static inline uint32_t silk_refcount_dec(silk_refcount_t *rc) {
    return --rc->count;
}

/* Get current count */
static inline uint32_t silk_refcount_get(silk_refcount_t *rc) {
    return rc->count;
}

#endif
