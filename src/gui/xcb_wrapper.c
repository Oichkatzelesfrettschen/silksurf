#include <stdlib.h>
#include <string.h>
#include <xcb/xcb.h>
#include "silksurf/xcb_wrapper.h"

/* Minimal XCB wrapper for ultra-lightweight GUI */

struct silk_display {
    xcb_connection_t *conn;
    xcb_screen_t *screen;
    int screen_num;
};

struct silk_window {
    xcb_window_t window;
};

struct silk_gc {
    xcb_gcontext_t gc;
};

struct silk_pixmap {
    xcb_pixmap_t pixmap;
    int width;
    int height;
    uint8_t depth;
};

/* Display management */
silk_display_t *silk_display_open(const char *display_name) {
    int screen_num;
    xcb_connection_t *conn = xcb_connect(display_name, &screen_num);

    if (xcb_connection_has_error(conn)) {
        xcb_disconnect(conn);
        return NULL;
    }

    silk_display_t *dpy = malloc(sizeof(silk_display_t));
    if (!dpy) {
        xcb_disconnect(conn);
        return NULL;
    }

    dpy->conn = conn;
    dpy->screen_num = screen_num;

    /* Get screen */
    const xcb_setup_t *setup = xcb_get_setup(conn);
    xcb_screen_iterator_t iter = xcb_setup_roots_iterator(setup);

    for (int i = 0; i < screen_num; i++)
        xcb_screen_next(&iter);

    dpy->screen = iter.data;
    if (!dpy->screen) {
        xcb_disconnect(conn);
        free(dpy);
        return NULL;
    }

    return dpy;
}

void silk_display_close(silk_display_t *dpy) {
    if (!dpy)
        return;
    if (dpy->conn)
        xcb_disconnect(dpy->conn);
    free(dpy);
}

xcb_connection_t *silk_display_get_conn(silk_display_t *dpy) {
    return dpy ? dpy->conn : NULL;
}

xcb_screen_t *silk_display_get_screen(silk_display_t *dpy) {
    return dpy ? dpy->screen : NULL;
}

int silk_display_get_fd(silk_display_t *dpy) {
    return dpy ? xcb_get_file_descriptor(dpy->conn) : -1;
}

void silk_display_flush(silk_display_t *dpy) {
    if (dpy)
        xcb_flush(dpy->conn);
}

/* Window creation */
silk_window_t *silk_window_create(silk_display_t *dpy,
                                   int x, int y, int width, int height) {
    if (!dpy)
        return NULL;

    xcb_window_t window = xcb_generate_id(dpy->conn);
    uint32_t mask = XCB_CW_BACK_PIXEL | XCB_CW_EVENT_MASK;
    uint32_t values[2] = {
        dpy->screen->white_pixel,  /* Background */
        XCB_EVENT_MASK_EXPOSURE | XCB_EVENT_MASK_KEY_PRESS |
        XCB_EVENT_MASK_BUTTON_PRESS | XCB_EVENT_MASK_BUTTON_RELEASE |
        XCB_EVENT_MASK_POINTER_MOTION | XCB_EVENT_MASK_STRUCTURE_NOTIFY
    };

    xcb_create_window(dpy->conn, dpy->screen->root_depth,
                      window, dpy->screen->root,
                      x, y, width, height, 0,
                      XCB_WINDOW_CLASS_INPUT_OUTPUT,
                      dpy->screen->root_visual,
                      mask, values);

    silk_window_t *win = malloc(sizeof(silk_window_t));
    if (!win) {
        xcb_destroy_window(dpy->conn, window);
        return NULL;
    }

    win->window = window;
    return win;
}

void silk_window_destroy(silk_display_t *dpy, silk_window_t *win) {
    if (!dpy || !win)
        return;
    xcb_destroy_window(dpy->conn, win->window);
    free(win);
}

xcb_window_t silk_window_get_id(silk_window_t *win) {
    return win ? win->window : 0;
}

/* Graphics context */
silk_gc_t *silk_gc_create(silk_display_t *dpy, silk_window_t *win) {
    if (!dpy || !win)
        return NULL;

    xcb_gcontext_t gid = xcb_generate_id(dpy->conn);
    uint32_t mask = XCB_GC_FOREGROUND | XCB_GC_BACKGROUND;
    uint32_t values[] = {dpy->screen->black_pixel, dpy->screen->white_pixel};

    xcb_create_gc(dpy->conn, gid, win->window, mask, values);

    silk_gc_t *gc = malloc(sizeof(silk_gc_t));
    if (!gc) {
        xcb_free_gc(dpy->conn, gid);
        return NULL;
    }

    gc->gc = gid;
    return gc;
}

void silk_gc_destroy(silk_display_t *dpy, silk_gc_t *gc) {
    if (!dpy || !gc)
        return;
    xcb_free_gc(dpy->conn, gc->gc);
    free(gc);
}

xcb_gcontext_t silk_gc_get_id(silk_gc_t *gc) {
    return gc ? gc->gc : 0;
}

void silk_gc_set_foreground(silk_display_t *dpy, silk_gc_t *gc,
                             uint32_t color) {
    if (!dpy || !gc)
        return;
    uint32_t mask = XCB_GC_FOREGROUND;
    xcb_change_gc(dpy->conn, gc->gc, mask, &color);
}

void silk_gc_set_background(silk_display_t *dpy, silk_gc_t *gc,
                             uint32_t color) {
    if (!dpy || !gc)
        return;
    uint32_t mask = XCB_GC_BACKGROUND;
    xcb_change_gc(dpy->conn, gc->gc, mask, &color);
}

/* Drawing primitives */
void silk_draw_rectangle(silk_display_t *dpy, silk_window_t *win,
                          silk_gc_t *gc, int x, int y,
                          int width, int height) {
    if (!dpy || !win || !gc)
        return;

    xcb_rectangle_t rect = {x, y, width, height};
    xcb_poly_fill_rectangle(dpy->conn, win->window, gc->gc, 1, &rect);
}

void silk_draw_line(silk_display_t *dpy, silk_window_t *win,
                     silk_gc_t *gc, int x1, int y1, int x2, int y2) {
    if (!dpy || !win || !gc)
        return;

    xcb_point_t points[2] = {{x1, y1}, {x2, y2}};
    xcb_poly_line(dpy->conn, XCB_COORD_MODE_ORIGIN, win->window,
                   gc->gc, 2, points);
}

void silk_draw_point(silk_display_t *dpy, silk_window_t *win,
                      silk_gc_t *gc, int x, int y) {
    if (!dpy || !win || !gc)
        return;

    xcb_point_t point = {x, y};
    xcb_poly_point(dpy->conn, XCB_COORD_MODE_ORIGIN, win->window,
                    gc->gc, 1, &point);
}

/* Pixmap operations */
silk_pixmap_t *silk_pixmap_create(silk_display_t *dpy,
                                   int width, int height, uint8_t depth) {
    if (!dpy)
        return NULL;

    xcb_pixmap_t pm = xcb_generate_id(dpy->conn);
    xcb_create_pixmap(dpy->conn, depth, pm, dpy->screen->root,
                      width, height);

    silk_pixmap_t *pixmap = malloc(sizeof(silk_pixmap_t));
    if (!pixmap) {
        xcb_free_pixmap(dpy->conn, pm);
        return NULL;
    }

    pixmap->pixmap = pm;
    pixmap->width = width;
    pixmap->height = height;
    pixmap->depth = depth;
    return pixmap;
}

void silk_pixmap_destroy(silk_display_t *dpy, silk_pixmap_t *pm) {
    if (!dpy || !pm)
        return;
    xcb_free_pixmap(dpy->conn, pm->pixmap);
    free(pm);
}

xcb_pixmap_t silk_pixmap_get_id(silk_pixmap_t *pm) {
    return pm ? pm->pixmap : 0;
}

void silk_pixmap_put_image(silk_display_t *dpy, silk_window_t *win,
                            silk_gc_t *gc, silk_pixmap_t *pm,
                            const uint8_t *data, int x, int y,
                            int width, int height) {
    (void)win;  /* Unused - reserved for future XShm integration */
    (void)gc;   /* Unused - reserved for future XShm integration */
    (void)x;    /* Unused - pending implementation */
    (void)y;    /* Unused - pending implementation */
    (void)width;  /* Unused - pending implementation */
    (void)height; /* Unused - pending implementation */

    if (!dpy || !pm || !data)
        return;

    /* TODO: Implement proper image transfer with XShm optimization */
}

void silk_pixmap_copy_to_window(silk_display_t *dpy, silk_window_t *win,
                                 silk_gc_t *gc, silk_pixmap_t *pm,
                                 int src_x, int src_y,
                                 int dst_x, int dst_y,
                                 int width, int height) {
    if (!dpy || !win || !gc || !pm)
        return;

    xcb_copy_area(dpy->conn, pm->pixmap, win->window, gc->gc,
                  src_x, src_y, dst_x, dst_y, width, height);
}

/* Atom management */
xcb_atom_t silk_atom_get(silk_display_t *dpy, const char *name) {
    if (!dpy || !name)
        return XCB_ATOM_NONE;

    xcb_intern_atom_cookie_t cookie = xcb_intern_atom(dpy->conn, 0,
                                                       strlen(name), name);
    xcb_intern_atom_reply_t *reply = xcb_intern_atom_reply(dpy->conn,
                                                            cookie, NULL);

    if (!reply)
        return XCB_ATOM_NONE;

    xcb_atom_t atom = reply->atom;
    free(reply);
    return atom;
}

/* Batch rendering */
#include "silksurf/renderer.h"

void silk_xcb_flush_commands(silk_display_t *dpy, silk_window_t *win, silk_gc_t *gc, silk_render_queue_t *queue) {
    if (!dpy || !win || !gc || !queue || queue->count == 0)
        return;

    for (int i = 0; i < queue->count; i++) {
        silk_draw_rect_cmd_t *cmd = &queue->commands[i];
        
        /* 1. Set color */
        silk_gc_set_foreground(dpy, gc, cmd->color);
        
        /* 2. Fill rectangle */
        xcb_rectangle_t r = { (int16_t)cmd->x, (int16_t)cmd->y, (uint16_t)cmd->w, (uint16_t)cmd->h };
        xcb_poly_fill_rectangle(dpy->conn, win->window, gc->gc, 1, &r);
    }
    
    silk_display_flush(dpy);
}
