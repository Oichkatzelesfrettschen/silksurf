#include <stdlib.h>
#include <string.h>
#include "silksurf/damage_tracker.h"

/* Damage tracking - efficiently identify screen regions needing redraw */

#define MAX_DAMAGE_RECTS 256

struct silk_damage_tracker {
    int screen_width;
    int screen_height;
    silk_rect_t rects[MAX_DAMAGE_RECTS];
    int rect_count;
    silk_rect_t bounding_box;
    int has_damage;
};

silk_damage_tracker_t *silk_damage_tracker_create(int screen_width,
                                                   int screen_height) {
    if (screen_width <= 0 || screen_height <= 0)
        return NULL;

    silk_damage_tracker_t *dt = malloc(sizeof(silk_damage_tracker_t));
    if (!dt)
        return NULL;

    dt->screen_width = screen_width;
    dt->screen_height = screen_height;
    dt->rect_count = 0;
    dt->has_damage = 0;
    memset(&dt->bounding_box, 0, sizeof(silk_rect_t));

    return dt;
}

void silk_damage_tracker_destroy(silk_damage_tracker_t *dt) {
    if (dt)
        free(dt);
}

/* Clamp rectangle to screen bounds */
static silk_rect_t clamp_rect(silk_damage_tracker_t *dt, int x, int y,
                               int width, int height) {
    silk_rect_t r;
    r.x = x < 0 ? 0 : x;
    r.y = y < 0 ? 0 : y;
    r.width = x + width > dt->screen_width ?
        dt->screen_width - r.x : width;
    r.height = y + height > dt->screen_height ?
        dt->screen_height - r.y : height;
    return r;
}

/* Check if two rectangles overlap */
static int rects_overlap(const silk_rect_t *a, const silk_rect_t *b) {
    return !(a->x + a->width <= b->x ||
             b->x + b->width <= a->x ||
             a->y + a->height <= b->y ||
             b->y + b->height <= a->y);
}

/* Merge bounding box */
static void merge_bounding_box(silk_damage_tracker_t *dt,
                                const silk_rect_t *rect) {
    if (!dt->has_damage) {
        dt->bounding_box = *rect;
        dt->has_damage = 1;
    } else {
        int x2 = dt->bounding_box.x + dt->bounding_box.width;
        int y2 = dt->bounding_box.y + dt->bounding_box.height;
        int rx2 = rect->x + rect->width;
        int ry2 = rect->y + rect->height;

        dt->bounding_box.x = dt->bounding_box.x < rect->x ?
            dt->bounding_box.x : rect->x;
        dt->bounding_box.y = dt->bounding_box.y < rect->y ?
            dt->bounding_box.y : rect->y;
        x2 = x2 > rx2 ? x2 : rx2;
        y2 = y2 > ry2 ? y2 : ry2;

        dt->bounding_box.width = x2 - dt->bounding_box.x;
        dt->bounding_box.height = y2 - dt->bounding_box.y;
    }
}

void silk_damage_add_rect(silk_damage_tracker_t *dt, int x, int y,
                           int width, int height) {
    if (!dt || width <= 0 || height <= 0)
        return;

    silk_rect_t rect = clamp_rect(dt, x, y, width, height);

    if (rect.width <= 0 || rect.height <= 0)
        return;

    /* Add to list if space available */
    if (dt->rect_count < MAX_DAMAGE_RECTS) {
        dt->rects[dt->rect_count++] = rect;
    }

    merge_bounding_box(dt, &rect);
}

void silk_damage_add_region(silk_damage_tracker_t *dt,
                             const silk_rect_t *rects, int count) {
    if (!dt || !rects || count <= 0)
        return;

    for (int i = 0; i < count; i++) {
        silk_rect_t rect = clamp_rect(dt, rects[i].x, rects[i].y,
                                       rects[i].width, rects[i].height);
        if (rect.width > 0 && rect.height > 0) {
            if (dt->rect_count < MAX_DAMAGE_RECTS) {
                dt->rects[dt->rect_count++] = rect;
            }
            merge_bounding_box(dt, &rect);
        }
    }
}

int silk_damage_get_rects(silk_damage_tracker_t *dt, silk_rect_t **rects) {
    if (!dt || !rects)
        return 0;

    *rects = dt->rects;
    return dt->rect_count;
}

silk_rect_t silk_damage_get_bounding_box(silk_damage_tracker_t *dt) {
    if (!dt || !dt->has_damage) {
        silk_rect_t empty = {0, 0, 0, 0};
        return empty;
    }
    return dt->bounding_box;
}

int silk_damage_is_dirty(silk_damage_tracker_t *dt, int x, int y,
                         int width, int height) {
    if (!dt || !dt->has_damage)
        return 0;

    silk_rect_t query = {x, y, width, height};

    for (int i = 0; i < dt->rect_count; i++) {
        if (rects_overlap(&query, &dt->rects[i]))
            return 1;
    }

    return 0;
}

void silk_damage_clear(silk_damage_tracker_t *dt) {
    if (!dt)
        return;

    dt->rect_count = 0;
    dt->has_damage = 0;
    memset(&dt->bounding_box, 0, sizeof(silk_rect_t));
}

int silk_damage_rect_count(silk_damage_tracker_t *dt) {
    return dt ? dt->rect_count : 0;
}

int silk_damage_coverage_percent(silk_damage_tracker_t *dt) {
    if (!dt || !dt->has_damage)
        return 0;

    unsigned long total_pixels = dt->screen_width * dt->screen_height;
    unsigned long damaged_pixels = 0;

    for (int i = 0; i < dt->rect_count; i++) {
        damaged_pixels += dt->rects[i].width * dt->rects[i].height;
    }

    return (int)(100 * damaged_pixels / total_pixels);
}
