#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>
#include "silksurf/pool.h"

/* Object pooling: O(1) acquire/release with free-list */

typedef struct pool_entry {
    struct pool_entry *next;
    uint8_t data[];
} pool_entry_t;

struct silk_pool {
    size_t object_size;      /* Size of each object */
    size_t capacity;         /* Max objects in pool */
    size_t available;        /* Free objects in pool */
    size_t used;             /* Allocated objects in use */
    pool_entry_t *free_list; /* Head of free list */
    uint8_t *storage;        /* Contiguous allocation */
};

silk_pool_t *silk_pool_create(size_t object_size, size_t capacity) {
    if (!object_size || !capacity)
        return NULL;

    silk_pool_t *pool = malloc(sizeof(silk_pool_t));
    if (!pool)
        return NULL;

    /* Allocate contiguous storage for all objects
       Each object has a header (next pointer) + data */
    size_t entry_size = sizeof(pool_entry_t) + object_size;
    size_t total_size = entry_size * capacity;

    pool->storage = malloc(total_size);
    if (!pool->storage) {
        free(pool);
        return NULL;
    }

    pool->object_size = object_size;
    pool->capacity = capacity;
    pool->available = capacity;
    pool->used = 0;

    /* Build free list linking all entries */
    pool->free_list = (pool_entry_t *)pool->storage;
    pool_entry_t *current = pool->free_list;

    for (size_t i = 0; i < capacity - 1; i++) {
        pool_entry_t *next = (pool_entry_t *)
            (pool->storage + (i + 1) * entry_size);
        current->next = next;
        current = next;
    }
    current->next = NULL;  /* Last entry points to NULL */

    return pool;
}

void silk_pool_destroy(silk_pool_t *pool) {
    if (!pool)
        return;
    if (pool->storage)
        free(pool->storage);
    free(pool);
}

void *silk_pool_acquire(silk_pool_t *pool) {
    if (!pool || pool->available == 0)
        return NULL;

    /* Pop from free list */
    pool_entry_t *entry = pool->free_list;
    pool->free_list = entry->next;
    pool->available--;
    pool->used++;

    return entry->data;
}

void silk_pool_release(silk_pool_t *pool, void *obj) {
    if (!pool || !obj)
        return;

    /* Convert data pointer back to entry header */
    pool_entry_t *entry = (pool_entry_t *)
        ((uint8_t *)obj - sizeof(pool_entry_t));

    /* Push back to free list */
    entry->next = pool->free_list;
    pool->free_list = entry;
    pool->available++;
    pool->used--;
}

size_t silk_pool_available(silk_pool_t *pool) {
    return pool ? pool->available : 0;
}

size_t silk_pool_used(silk_pool_t *pool) {
    return pool ? pool->used : 0;
}

void silk_pool_reset(silk_pool_t *pool) {
    if (!pool)
        return;

    /* Rebuild free list */
    size_t entry_size = sizeof(pool_entry_t) + pool->object_size;
    pool->free_list = (pool_entry_t *)pool->storage;
    pool_entry_t *current = pool->free_list;

    for (size_t i = 0; i < pool->capacity - 1; i++) {
        pool_entry_t *next = (pool_entry_t *)
            (pool->storage + (i + 1) * entry_size);
        current->next = next;
        current = next;
    }
    current->next = NULL;

    pool->available = pool->capacity;
    pool->used = 0;
}

void silk_pool_stats(silk_pool_t *pool) {
    if (!pool)
        return;

    double usage_pct = (100.0 * pool->used) / pool->capacity;
    size_t total_bytes = (sizeof(pool_entry_t) + pool->object_size) * pool->capacity;
    size_t total_mb = total_bytes / (1024 * 1024);

    printf("Pool Stats (object size: %zu):\n", pool->object_size);
    printf("  Capacity:  %zu\n", pool->capacity);
    printf("  Used:      %zu (%.1f%%)\n", pool->used, usage_pct);
    printf("  Available: %zu\n", pool->available);
    printf("  Total:     %zu MB\n", total_mb);
}
