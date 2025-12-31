#ifndef SILKSURF_WINDOW_H
#define SILKSURF_WINDOW_H

#include <stdint.h>

/* High-level window management */

/* Forward declarations */
struct silk_window_mgr;
struct silk_app_window;

typedef struct silk_window_mgr silk_window_mgr_t;
typedef struct silk_app_window silk_app_window_t;

/* Window manager creation */
silk_window_mgr_t *silk_window_mgr_create(const char *display);
void silk_window_mgr_destroy(silk_window_mgr_t *mgr);

/* Window creation with automatic setup */
silk_app_window_t *silk_window_mgr_create_window(silk_window_mgr_t *mgr,
                                                   const char *title,
                                                   int width, int height);
void silk_window_mgr_close_window(silk_window_mgr_t *mgr,
                                   silk_app_window_t *win);

/* Window control */
void silk_window_show(silk_app_window_t *win);
void silk_window_hide(silk_app_window_t *win);
void silk_window_set_title(silk_app_window_t *win, const char *title);
void silk_window_get_size(silk_app_window_t *win, int *w, int *h);
void silk_window_get_position(silk_app_window_t *win, int *x, int *y);

/* Drawing surface */
uint32_t *silk_window_get_backbuffer(silk_app_window_t *win);
void silk_window_present(silk_window_mgr_t *mgr, silk_app_window_t *win);
void silk_window_mgr_flush(silk_window_mgr_t *mgr);

/* Handle accessors */
struct silk_display *silk_window_mgr_get_display(silk_window_mgr_t *mgr);
struct silk_window *silk_window_get_xcb_handle(silk_app_window_t *win);
struct silk_gc *silk_window_get_gc(silk_app_window_t *win);

#endif

