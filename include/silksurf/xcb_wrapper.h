#ifndef SILKSURF_XCB_WRAPPER_H
#define SILKSURF_XCB_WRAPPER_H

#include <stdint.h>
#include <xcb/xcb.h>

/* Minimal XCB wrapper - ultra-lightweight abstraction */

/* Forward declarations */
struct silk_display;
struct silk_window;
struct silk_gc;
struct silk_pixmap;

typedef struct silk_display silk_display_t;
typedef struct silk_window silk_window_t;
typedef struct silk_gc silk_gc_t;
typedef struct silk_pixmap silk_pixmap_t;

/* Display/Connection management */
silk_display_t *silk_display_open(const char *display_name);
void silk_display_close(silk_display_t *dpy);
xcb_connection_t *silk_display_get_conn(silk_display_t *dpy);
xcb_screen_t *silk_display_get_screen(silk_display_t *dpy);
int silk_display_get_fd(silk_display_t *dpy);
void silk_display_flush(silk_display_t *dpy);

/* Window management (low-level XCB) */
silk_window_t *silk_window_create(silk_display_t *dpy,
                                   int x, int y, int width, int height);
void silk_window_destroy(silk_display_t *dpy, silk_window_t *win);
xcb_window_t silk_window_get_id(silk_window_t *win);

/* Graphics Context */
silk_gc_t *silk_gc_create(silk_display_t *dpy, silk_window_t *win);
void silk_gc_destroy(silk_display_t *dpy, silk_gc_t *gc);
xcb_gcontext_t silk_gc_get_id(silk_gc_t *gc);
void silk_gc_set_foreground(silk_display_t *dpy, silk_gc_t *gc,
                             uint32_t color);
void silk_gc_set_background(silk_display_t *dpy, silk_gc_t *gc,
                             uint32_t color);

/* Drawing primitives */
void silk_draw_rectangle(silk_display_t *dpy, silk_window_t *win,
                          silk_gc_t *gc, int x, int y,
                          int width, int height);
void silk_draw_line(silk_display_t *dpy, silk_window_t *win,
                     silk_gc_t *gc, int x1, int y1, int x2, int y2);
void silk_draw_point(silk_display_t *dpy, silk_window_t *win,
                      silk_gc_t *gc, int x, int y);

/* Pixmap operations */
silk_pixmap_t *silk_pixmap_create(silk_display_t *dpy,
                                   int width, int height, uint8_t depth);
void silk_pixmap_destroy(silk_display_t *dpy, silk_pixmap_t *pm);
xcb_pixmap_t silk_pixmap_get_id(silk_pixmap_t *pm);
void silk_pixmap_put_image(silk_display_t *dpy, silk_window_t *win,
                            silk_gc_t *gc, silk_pixmap_t *pm,
                            const uint8_t *data, int x, int y,
                            int width, int height);
void silk_pixmap_copy_to_window(silk_display_t *dpy, silk_window_t *win,
                                 silk_gc_t *gc, silk_pixmap_t *pm,
                                 int src_x, int src_y,
                                 int dst_x, int dst_y,
                                 int width, int height);

/* Atom management */
xcb_atom_t silk_atom_get(silk_display_t *dpy, const char *name);

/* Batch rendering */
#include "silksurf/renderer.h"
void silk_xcb_flush_commands(silk_display_t *dpy, silk_window_t *win, silk_gc_t *gc, silk_render_queue_t *queue);

#endif
