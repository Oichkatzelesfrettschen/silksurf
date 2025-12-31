#ifndef SILKSURF_RENDERER_H
#define SILKSURF_RENDERER_H

#include <stdint.h>
#include <stddef.h>
#include "silksurf/window.h"
#include "silksurf/damage_tracker.h"
#include "silksurf/pixmap_cache.h"
#include "silksurf/pixel_ops.h"

/* Command types for batch rendering */
typedef struct {
    uint32_t color;
    int x, y, w, h;
} silk_draw_rect_cmd_t;

#define SILK_RENDER_QUEUE_MAX 4096

typedef struct {
    silk_draw_rect_cmd_t commands[SILK_RENDER_QUEUE_MAX];
    int count;
} silk_render_queue_t;

/* Renderer - integrates damage tracking, caching, and pixel operations */

typedef struct silk_renderer silk_renderer_t;

/* Create and destroy */
silk_renderer_t *silk_renderer_create(silk_window_mgr_t *win_mgr,
                                       silk_app_window_t *window,
                                       size_t cache_size_bytes);
void silk_renderer_destroy(silk_renderer_t *renderer);

/* Queue management */
void silk_render_queue_init(silk_render_queue_t *queue);
void silk_render_queue_push_rect(silk_render_queue_t *queue, int x, int y, int w, int h, uint32_t color);

/* High-level painting */
struct silk_dom_node;
void silk_paint_node(struct silk_dom_node *node, silk_render_queue_t *queue);

/* Frame lifecycle */
void silk_renderer_begin_frame(silk_renderer_t *renderer);
void silk_renderer_end_frame(silk_renderer_t *renderer);

/* Rendering operations */
void silk_renderer_clear(silk_renderer_t *renderer, silk_color_t color);
void silk_renderer_fill_rect(silk_renderer_t *renderer, int x, int y,
                              int width, int height, silk_color_t color);
void silk_renderer_copy_pixels(silk_renderer_t *renderer,
                                const silk_color_t *src, int src_width,
                                int x, int y, int width, int height);
void silk_renderer_blend_pixels(silk_renderer_t *renderer,
                                 const silk_color_t *src, int src_width,
                                 int x, int y, int width, int height,
                                 uint8_t alpha);

/* Present to screen */
void silk_renderer_present(silk_renderer_t *renderer);

/* Statistics */
int silk_renderer_damage_coverage_percent(silk_renderer_t *renderer);
int silk_renderer_cache_hit_rate(silk_renderer_t *renderer);
size_t silk_renderer_cache_used(silk_renderer_t *renderer);
const char *silk_renderer_backend(silk_renderer_t *renderer);

#endif
