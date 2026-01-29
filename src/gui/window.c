#include <stdlib.h>
#include <string.h>
#include "silksurf/window.h"
#include "silksurf/xcb_wrapper.h"

/* High-level window management built on XCB */

struct silk_window_mgr {
    silk_display_t *display;
};

struct silk_app_window {
    silk_window_t *xcb_window;
    silk_gc_t *gc;
    uint32_t *backbuffer;
    int width;
    int height;
    int visible;
};

silk_window_mgr_t *silk_window_mgr_create(const char *display) {
    silk_window_mgr_t *mgr = malloc(sizeof(silk_window_mgr_t));
    if (!mgr)
        return NULL;

    mgr->display = silk_display_open(display);
    if (!mgr->display) {
        free(mgr);
        return NULL;
    }

    return mgr;
}

void silk_window_mgr_destroy(silk_window_mgr_t *mgr) {
    if (!mgr)
        return;
    if (mgr->display)
        silk_display_close(mgr->display);
    free(mgr);
}

silk_app_window_t *silk_window_mgr_create_window(silk_window_mgr_t *mgr,
                                                   const char *title,
                                                   int width, int height) {
    if (!mgr || !title || width <= 0 || height <= 0)
        return NULL;

    silk_app_window_t *win = malloc(sizeof(silk_app_window_t));
    if (!win)
        return NULL;

    /* Create XCB window */
    win->xcb_window = silk_window_create(mgr->display, 0, 0, width, height);
    if (!win->xcb_window) {
        free(win);
        return NULL;
    }

    /* Create graphics context */
    win->gc = silk_gc_create(mgr->display, win->xcb_window);
    if (!win->gc) {
        silk_window_destroy(mgr->display, win->xcb_window);
        free(win);
        return NULL;
    }

    /* Allocate backbuffer (RGBA32) */
    size_t buffer_size = width * height * sizeof(uint32_t);
    win->backbuffer = malloc(buffer_size);
    if (!win->backbuffer) {
        silk_gc_destroy(mgr->display, win->gc);
        silk_window_destroy(mgr->display, win->xcb_window);
        free(win);
        return NULL;
    }

    memset(win->backbuffer, 0, buffer_size);

    win->width = width;
    win->height = height;
    win->visible = 0;

    /* TODO: Set window title via silk_display_t */
    /* Title setting deferred - requires proper XCB atom handling */

    return win;
}

void silk_window_mgr_close_window(silk_window_mgr_t *mgr,
                                   silk_app_window_t *win) {
    if (!mgr || !win)
        return;

    if (win->backbuffer)
        free(win->backbuffer);
    if (win->gc)
        silk_gc_destroy(mgr->display, win->gc);
    if (win->xcb_window)
        silk_window_destroy(mgr->display, win->xcb_window);

    free(win);
}

void silk_window_show(silk_app_window_t *win) {
    if (win)
        win->visible = 1;
}

void silk_window_hide(silk_app_window_t *win) {
    if (win)
        win->visible = 0;
}

void silk_window_set_title(silk_app_window_t *win, const char *title) {
    (void)win;   /* Unused - pending implementation */
    (void)title; /* Unused - pending implementation */
    /* Title was already set at creation; would need display reference
       to change it. This is a placeholder. */
}

void silk_window_get_size(silk_app_window_t *win, int *w, int *h) {
    if (win) {
        if (w) *w = win->width;
        if (h) *h = win->height;
    }
}

void silk_window_get_position(silk_app_window_t *win, int *x, int *y) {
    (void)win; /* Unused - position not currently tracked */
    if (x) *x = 0;  /* Not tracked */
    if (y) *y = 0;
}

uint32_t *silk_window_get_backbuffer(silk_app_window_t *win) {
    return win ? win->backbuffer : NULL;
}

void silk_window_present(silk_window_mgr_t *mgr, silk_app_window_t *win) {
    if (!mgr || !win || !win->visible)
        return;

    /* TODO: Implement XShm or pixmap-based image transfer
       For now, this is a placeholder that would copy backbuffer to window */
}

void silk_window_clear(silk_app_window_t *win, uint32_t color) {
    if (!win || !win->backbuffer)
        return;

    size_t pixel_count = win->width * win->height;
    for (size_t i = 0; i < pixel_count; i++)
        win->backbuffer[i] = color;
}

void silk_window_mgr_flush(silk_window_mgr_t *mgr) {
    if (mgr && mgr->display)
        silk_display_flush(mgr->display);
}

struct silk_display *silk_window_mgr_get_display(silk_window_mgr_t *mgr) {
    return mgr ? mgr->display : NULL;
}

struct silk_window *silk_window_get_xcb_handle(silk_app_window_t *win) {
    return win ? win->xcb_window : NULL;
}

struct silk_gc *silk_window_get_gc(silk_app_window_t *win) {
    return win ? win->gc : NULL;
}
