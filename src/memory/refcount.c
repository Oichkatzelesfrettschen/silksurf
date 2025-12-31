#include <stddef.h>
#include "silksurf/refcount.h"

/* Reference counting utilities for typed objects */

/* Generic refcount acquire - increments count and returns the object */
void *silk_refcount_acquire(void *obj, silk_refcount_t *rc) {
    if (obj && rc)
        silk_refcount_inc(rc);
    return obj;
}

/* Generic refcount release - decrements and returns the new count */
uint32_t silk_refcount_release(void *obj, silk_refcount_t *rc) {
    if (!obj || !rc)
        return 0;
    return silk_refcount_dec(rc);
}

/* Check if object is last reference (ready to free) */
int silk_refcount_is_last(silk_refcount_t *rc) {
    return rc && rc->count == 1;
}

/* Check if object has multiple references (shared) */
int silk_refcount_is_shared(silk_refcount_t *rc) {
    return rc && rc->count > 1;
}
