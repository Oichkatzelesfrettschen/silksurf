#include <stdlib.h>
#include <string.h>
#include "silksurf/renderer.h"

/* Main renderer - integrates all rendering subsystems */

struct silk_renderer {
    silk_window_mgr_t *win_mgr;
    silk_app_window_t *window;
    silk_damage_tracker_t *damage;
    silk_pixmap_cache_t *pixmap_cache;
    silk_color_t *backbuffer;
    int width;
    int height;
    int frame_count;
};

silk_renderer_t *silk_renderer_create(silk_window_mgr_t *win_mgr,
                                       silk_app_window_t *window,
                                       size_t cache_size_bytes) {
    if (!win_mgr || !window)
        return NULL;

    silk_renderer_t *renderer = malloc(sizeof(silk_renderer_t));
    if (!renderer)
        return NULL;

    renderer->win_mgr = win_mgr;
    renderer->window = window;

    /* Get window dimensions */
    silk_window_get_size(window, &renderer->width, &renderer->height);

    /* Get backbuffer - updated each frame */
    renderer->backbuffer = silk_window_get_backbuffer(window);
    renderer->frame_count = 0;

    /* Create damage tracker for partial screen updates */
    renderer->damage = silk_damage_tracker_create(renderer->width,
                                                  renderer->height);
    if (!renderer->damage) {
        free(renderer);
        return NULL;
    }

    /* Create pixmap cache for VRAM reuse */
    renderer->pixmap_cache = silk_pixmap_cache_create(cache_size_bytes);
    if (!renderer->pixmap_cache) {
        silk_damage_tracker_destroy(renderer->damage);
        free(renderer);
        return NULL;
    }

    return renderer;
}

void silk_renderer_destroy(silk_renderer_t *renderer) {
    if (!renderer)
        return;

    if (renderer->damage)
        silk_damage_tracker_destroy(renderer->damage);
    if (renderer->pixmap_cache)
        silk_pixmap_cache_destroy(renderer->pixmap_cache);

    free(renderer);
}

void silk_renderer_begin_frame(silk_renderer_t *renderer) {
    if (!renderer || !renderer->damage)
        return;

    /* Start fresh damage tracking for this frame */
    silk_damage_clear(renderer->damage);
}

void silk_renderer_end_frame(silk_renderer_t *renderer) {
    if (!renderer)
        return;

    renderer->frame_count++;
    /* Damage is accumulated during frame; ready for presentation */
}

void silk_renderer_clear(silk_renderer_t *renderer, silk_color_t color) {
    if (!renderer || !renderer->backbuffer || !renderer->damage)
        return;

    /* Mark entire screen as damaged */
    silk_damage_add_rect(renderer->damage, 0, 0,
                         renderer->width, renderer->height);

    /* Clear the backbuffer */
    silk_clear_buffer(renderer->backbuffer,
                     renderer->width * renderer->height,
                     color);
}

void silk_renderer_fill_rect(silk_renderer_t *renderer, int x, int y,
                              int width, int height, silk_color_t color) {
    if (!renderer || !renderer->backbuffer || !renderer->damage)
        return;

    if (width <= 0 || height <= 0)
        return;

    /* Track damage region */
    silk_damage_add_rect(renderer->damage, x, y, width, height);

    /* Fill the rectangle */
    silk_fill_rect(renderer->backbuffer, renderer->width,
                   x, y, width, height, color);
}

void silk_renderer_copy_pixels(silk_renderer_t *renderer,
                                const silk_color_t *src, int src_width,
                                int x, int y, int width, int height) {
    if (!renderer || !renderer->backbuffer || !renderer->damage || !src)
        return;

    if (width <= 0 || height <= 0)
        return;

    /* Track damage region */
    silk_damage_add_rect(renderer->damage, x, y, width, height);

    /* Copy pixels */
    silk_copy_pixels(src, src_width,
                     renderer->backbuffer, renderer->width,
                     x, y, width, height);
}

void silk_renderer_blend_pixels(silk_renderer_t *renderer,
                                 const silk_color_t *src, int src_width,
                                 int x, int y, int width, int height,
                                 uint8_t alpha) {
    if (!renderer || !renderer->backbuffer || !renderer->damage || !src)
        return;

    if (width <= 0 || height <= 0)
        return;

    /* Track damage region */
    silk_damage_add_rect(renderer->damage, x, y, width, height);

    /* Blend pixels */
    silk_blend_pixels(src, src_width,
                      renderer->backbuffer, renderer->width,
                      x, y, width, height, alpha);
}

void silk_renderer_present(silk_renderer_t *renderer) {
    if (!renderer || !renderer->window || !renderer->damage)
        return;

    /* For now: present entire backbuffer
       In production: only update damaged regions via XDamage extension */
    silk_window_present(renderer->win_mgr, renderer->window);

    /* Clear damage tracking for next frame */
    silk_damage_clear(renderer->damage);
}

int silk_renderer_damage_coverage_percent(silk_renderer_t *renderer) {
    if (!renderer || !renderer->damage)
        return 0;

    return silk_damage_coverage_percent(renderer->damage);
}

int silk_renderer_cache_hit_rate(silk_renderer_t *renderer) {
    if (!renderer || !renderer->pixmap_cache)
        return 0;

    return silk_pixmap_cache_hit_rate(renderer->pixmap_cache);
}

size_t silk_renderer_cache_used(silk_renderer_t *renderer) {
    if (!renderer || !renderer->pixmap_cache)
        return 0;

    return silk_pixmap_cache_used(renderer->pixmap_cache);
}

const char *silk_renderer_backend(silk_renderer_t *renderer) {
    if (!renderer)
        return "none";

    return silk_pixel_ops_backend();
}
