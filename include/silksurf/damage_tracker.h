#ifndef SILKSURF_DAMAGE_TRACKER_H
#define SILKSURF_DAMAGE_TRACKER_H

#include <stdint.h>

/* Damage tracking - efficiently update only changed regions */

typedef struct silk_damage_tracker silk_damage_tracker_t;

/* Region definition */
typedef struct {
    int x, y;
    int width, height;
} silk_rect_t;

/* Create and destroy */
silk_damage_tracker_t *silk_damage_tracker_create(int screen_width,
                                                   int screen_height);
void silk_damage_tracker_destroy(silk_damage_tracker_t *dt);

/* Add damaged region */
void silk_damage_add_rect(silk_damage_tracker_t *dt, int x, int y,
                           int width, int height);
void silk_damage_add_region(silk_damage_tracker_t *dt,
                             const silk_rect_t *rects, int count);

/* Query damaged regions */
int silk_damage_get_rects(silk_damage_tracker_t *dt, silk_rect_t **rects);
silk_rect_t silk_damage_get_bounding_box(silk_damage_tracker_t *dt);

/* Check if region is damaged */
int silk_damage_is_dirty(silk_damage_tracker_t *dt, int x, int y,
                         int width, int height);

/* Clear damage (call after rendering) */
void silk_damage_clear(silk_damage_tracker_t *dt);

/* Statistics */
int silk_damage_rect_count(silk_damage_tracker_t *dt);
int silk_damage_coverage_percent(silk_damage_tracker_t *dt);

#endif
