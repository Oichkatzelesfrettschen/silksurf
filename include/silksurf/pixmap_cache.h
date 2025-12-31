#ifndef SILKSURF_PIXMAP_CACHE_H
#define SILKSURF_PIXMAP_CACHE_H

#include <stdint.h>
#include <stddef.h>

/* Pixmap cache - LRU-based VRAM reuse */

typedef struct silk_pixmap_cache silk_pixmap_cache_t;
typedef struct silk_cached_pixmap silk_cached_pixmap_t;

/* Pixmap cache key (for caching rendered content) */
typedef struct {
    uint64_t hash;          /* Content hash */
    int width, height;
    uint8_t depth;
} silk_pixmap_key_t;

/* Create and destroy */
silk_pixmap_cache_t *silk_pixmap_cache_create(size_t max_vram_bytes);
void silk_pixmap_cache_destroy(silk_pixmap_cache_t *cache);

/* Lookup cached pixmap */
silk_cached_pixmap_t *silk_pixmap_cache_lookup(silk_pixmap_cache_t *cache,
                                                const silk_pixmap_key_t *key);

/* Add pixmap to cache */
int silk_pixmap_cache_insert(silk_pixmap_cache_t *cache,
                              const silk_pixmap_key_t *key,
                              void *pixmap_data, size_t data_size);

/* Mark pixmap as recently used */
void silk_pixmap_cache_touch(silk_pixmap_cache_t *cache,
                              silk_cached_pixmap_t *pixmap);

/* Clear cache (evict all entries) */
void silk_pixmap_cache_clear(silk_pixmap_cache_t *cache);

/* Statistics */
size_t silk_pixmap_cache_used(silk_pixmap_cache_t *cache);
size_t silk_pixmap_cache_capacity(silk_pixmap_cache_t *cache);
int silk_pixmap_cache_entry_count(silk_pixmap_cache_t *cache);
int silk_pixmap_cache_hit_rate(silk_pixmap_cache_t *cache);

/* Cached pixmap access */
void *silk_cached_pixmap_get_data(silk_cached_pixmap_t *pixmap);
size_t silk_cached_pixmap_get_size(silk_cached_pixmap_t *pixmap);

#endif
