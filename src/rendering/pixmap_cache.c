#include <stdlib.h>
#include <string.h>
#include "silksurf/pixmap_cache.h"

/* Pixmap cache - LRU-based VRAM management */

#define MAX_CACHE_ENTRIES 1024

struct cache_entry {
    struct cache_entry *prev, *next;  /* LRU list */
    silk_pixmap_key_t key;
    void *data;
    size_t data_size;
    uint32_t access_count;            /* For statistics */
};

typedef struct cache_entry cache_entry_t;

struct silk_pixmap_cache {
    cache_entry_t entries[MAX_CACHE_ENTRIES];
    cache_entry_t *lru_head;          /* Least recently used */
    cache_entry_t *lru_tail;          /* Most recently used */
    int entry_count;
    size_t max_vram;
    size_t used_vram;
    int64_t hits;
    int64_t misses;
};

silk_pixmap_cache_t *silk_pixmap_cache_create(size_t max_vram_bytes) {
    if (max_vram_bytes == 0)
        return NULL;

    silk_pixmap_cache_t *cache = malloc(sizeof(silk_pixmap_cache_t));
    if (!cache)
        return NULL;

    memset(cache, 0, sizeof(silk_pixmap_cache_t));
    cache->max_vram = max_vram_bytes;
    cache->lru_head = NULL;
    cache->lru_tail = NULL;

    return cache;
}

void silk_pixmap_cache_destroy(silk_pixmap_cache_t *cache) {
    if (!cache)
        return;

    for (int i = 0; i < cache->entry_count; i++) {
        if (cache->entries[i].data)
            free(cache->entries[i].data);
    }

    free(cache);
}

/* Move entry to tail (most recently used) */
static void move_to_tail(silk_pixmap_cache_t *cache, cache_entry_t *entry) {
    if (!entry || entry == cache->lru_tail)
        return;

    /* Remove from current position */
    if (entry->prev)
        entry->prev->next = entry->next;
    else
        cache->lru_head = entry->next;

    if (entry->next)
        entry->next->prev = entry->prev;
    else
        cache->lru_tail = entry->prev;

    /* Add to tail */
    if (cache->lru_tail)
        cache->lru_tail->next = entry;
    entry->prev = cache->lru_tail;
    entry->next = NULL;
    cache->lru_tail = entry;

    if (!cache->lru_head)
        cache->lru_head = entry;
}

/* Evict LRU entry to make space */
static void evict_lru(silk_pixmap_cache_t *cache) {
    if (!cache->lru_head)
        return;

    cache_entry_t *to_evict = cache->lru_head;

    /* Remove from list */
    if (to_evict->next)
        to_evict->next->prev = NULL;
    else
        cache->lru_tail = NULL;
    cache->lru_head = to_evict->next;

    /* Free data */
    if (to_evict->data) {
        free(to_evict->data);
        cache->used_vram -= to_evict->data_size;
    }

    /* Mark entry as free */
    memset(to_evict, 0, sizeof(cache_entry_t));
}

silk_cached_pixmap_t *silk_pixmap_cache_lookup(silk_pixmap_cache_t *cache,
                                                const silk_pixmap_key_t *key) {
    if (!cache || !key)
        return NULL;

    /* Linear search (could use hash table for better performance) */
    for (int i = 0; i < MAX_CACHE_ENTRIES; i++) {
        cache_entry_t *entry = &cache->entries[i];
        if (entry->data &&
            entry->key.hash == key->hash &&
            entry->key.width == key->width &&
            entry->key.height == key->height &&
            entry->key.depth == key->depth) {

            move_to_tail(cache, entry);
            entry->access_count++;
            cache->hits++;
            return (silk_cached_pixmap_t *)entry;
        }
    }

    cache->misses++;
    return NULL;
}

int silk_pixmap_cache_insert(silk_pixmap_cache_t *cache,
                              const silk_pixmap_key_t *key,
                              void *pixmap_data, size_t data_size) {
    if (!cache || !key || !pixmap_data || data_size == 0)
        return 0;

    /* Find free entry */
    cache_entry_t *entry = NULL;
    for (int i = 0; i < MAX_CACHE_ENTRIES; i++) {
        if (!cache->entries[i].data) {
            entry = &cache->entries[i];
            break;
        }
    }

    /* No free entry, evict LRU */
    if (!entry) {
        if (cache->entry_count >= MAX_CACHE_ENTRIES)
            evict_lru(cache);

        for (int i = 0; i < MAX_CACHE_ENTRIES; i++) {
            if (!cache->entries[i].data) {
                entry = &cache->entries[i];
                break;
            }
        }

        if (!entry)
            return 0;
    }

    /* Evict entries until we have space */
    while (cache->used_vram + data_size > cache->max_vram &&
           cache->entry_count > 0) {
        evict_lru(cache);
    }

    /* Store pixmap */
    entry->key = *key;
    entry->data = pixmap_data;
    entry->data_size = data_size;
    entry->access_count = 1;

    cache->used_vram += data_size;
    if (!entry->prev && !entry->next) {
        cache->entry_count++;
    }

    move_to_tail(cache, entry);
    return 1;
}

void silk_pixmap_cache_touch(silk_pixmap_cache_t *cache,
                              silk_cached_pixmap_t *pixmap) {
    if (!cache || !pixmap)
        return;

    cache_entry_t *entry = (cache_entry_t *)pixmap;
    move_to_tail(cache, entry);
    entry->access_count++;
}

void silk_pixmap_cache_clear(silk_pixmap_cache_t *cache) {
    if (!cache)
        return;

    for (int i = 0; i < MAX_CACHE_ENTRIES; i++) {
        if (cache->entries[i].data) {
            free(cache->entries[i].data);
            cache->entries[i].data = NULL;
        }
    }

    cache->entry_count = 0;
    cache->used_vram = 0;
    cache->lru_head = NULL;
    cache->lru_tail = NULL;
}

size_t silk_pixmap_cache_used(silk_pixmap_cache_t *cache) {
    return cache ? cache->used_vram : 0;
}

size_t silk_pixmap_cache_capacity(silk_pixmap_cache_t *cache) {
    return cache ? cache->max_vram : 0;
}

int silk_pixmap_cache_entry_count(silk_pixmap_cache_t *cache) {
    return cache ? cache->entry_count : 0;
}

int silk_pixmap_cache_hit_rate(silk_pixmap_cache_t *cache) {
    if (!cache || (cache->hits + cache->misses == 0))
        return 0;

    return (int)(100 * cache->hits / (cache->hits + cache->misses));
}

void *silk_cached_pixmap_get_data(silk_cached_pixmap_t *pixmap) {
    if (!pixmap)
        return NULL;
    cache_entry_t *entry = (cache_entry_t *)pixmap;
    return entry->data;
}

size_t silk_cached_pixmap_get_size(silk_cached_pixmap_t *pixmap) {
    if (!pixmap)
        return 0;
    cache_entry_t *entry = (cache_entry_t *)pixmap;
    return entry->data_size;
}
