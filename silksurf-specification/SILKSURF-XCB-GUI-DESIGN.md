================================================================================
SILKSURF XCB GUI FRAMEWORK DETAILED DESIGN SPECIFICATION
================================================================================
Version: 1.0
Date: 2025-12-31
Audience: Graphics/UI implementation teams (Phase 2-3)
Status: Architecture Freeze

EXECUTIVE SUMMARY
================================================================================

The SilkSurf XCB GUI Framework is a minimal, high-performance graphics layer
built on pure XCB (no GTK dependencies). It provides:

- **Window management**: Window creation, event handling, lifecycle
- **Double-buffering**: Pixmap-based back/front buffers with copy-on-write
- **Damage tracking**: Incremental rendering (only repaint changed regions)
- **Widget system**: Base widget class, buttons, text inputs, scrollbars
- **Event dispatch**: Mouse, keyboard, window events with hit testing
- **Acceleration**: XShm for 10x faster image blits, future DRI3 support

Key design goals:
- Zero GTK overhead (50MB library eliminated)
- 60 FPS target (16.67ms per frame budget)
- Support multiple interfaces (CLI, TUI, Curses, XCB) via CMake feature flags
- Direct integration with C core rendering pipeline
- Deterministic performance (no GC pauses during frame rendering)

================================================================================
PART 1: XCB WINDOW MANAGEMENT
================================================================================

### 1.1 Window Initialization

```c
// silksurf-gui/xcb/window.h

#ifndef XCB_WINDOW_H
#define XCB_WINDOW_H

#include <xcb/xcb.h>
#include <xcb/xcb_keysyms.h>
#include <xcb/shm.h>
#include <stdint.h>

typedef struct {
    xcb_connection_t *conn;
    xcb_window_t window;
    xcb_screen_t *screen;
    uint32_t visual_id;

    int width;
    int height;

    // Atom caching (for WM_PROTOCOLS, WM_DELETE_WINDOW, etc.)
    xcb_atom_t wm_protocols;
    xcb_atom_t wm_delete_window;

    // Double-buffering
    xcb_pixmap_t back_buffer;
    xcb_gcontext_t gc;
    uint32_t *pixel_data;

    // Event handling
    int running;
    uint32_t last_configure_time;  // Debounce window resize

    // XShm extension
    int shm_available;
    xcb_shm_segment_info_t shm_info;

    // Damage tracking
    struct DamageTracker *damage;

    // Input state
    struct {
        int mouse_x, mouse_y;
        int button_pressed;
        int key_state[256];  // Simple key state array
    } input;
} XcbWindow;

// Window creation
XcbWindow *xcb_window_create(const char *title, int width, int height);

// Window destruction
void xcb_window_destroy(XcbWindow *win);

// Event loop
void xcb_window_run_event_loop(XcbWindow *win, void (*on_frame)(XcbWindow *win));

// Buffer management
void xcb_window_swap_buffers(XcbWindow *win);

// Rendering
void xcb_window_paint_pixel(XcbWindow *win, int x, int y, uint32_t color);
void xcb_window_paint_rect(XcbWindow *win, int x, int y, int w, int h, uint32_t color);
void xcb_window_paint_text(XcbWindow *win, int x, int y, const char *text, uint32_t color);

#endif
```

### 1.2 Window Creation Implementation

```c
// silksurf-gui/xcb/window.c

#include "window.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <sys/shm.h>

static xcb_atom_t xcb_get_atom(xcb_connection_t *conn, const char *atom_name) {
    xcb_intern_atom_reply_t *reply = xcb_intern_atom_reply(
        conn,
        xcb_intern_atom(conn, 0, strlen(atom_name), atom_name),
        NULL
    );
    if (reply == NULL) return 0;
    xcb_atom_t atom = reply->atom;
    free(reply);
    return atom;
}

XcbWindow *xcb_window_create(const char *title, int width, int height) {
    XcbWindow *win = calloc(1, sizeof(XcbWindow));
    if (win == NULL) return NULL;

    // Connect to X server
    win->conn = xcb_connect(NULL, NULL);
    if (xcb_connection_has_error(win->conn)) {
        fprintf(stderr, "Failed to connect to X server\n");
        free(win);
        return NULL;
    }

    // Get screen
    const xcb_setup_t *setup = xcb_get_setup(win->conn);
    xcb_screen_iterator_t iter = xcb_setup_roots_iterator(setup);
    win->screen = iter.data;

    win->width = width;
    win->height = height;

    // Create window
    win->window = xcb_generate_id(win->conn);

    uint32_t mask = XCB_CW_BACK_PIXEL | XCB_CW_EVENT_MASK;
    uint32_t values[2] = {
        win->screen->white_pixel,  // Background color
        XCB_EVENT_MASK_EXPOSURE |       // Redraw events
        XCB_EVENT_MASK_BUTTON_PRESS |   // Mouse buttons
        XCB_EVENT_MASK_BUTTON_RELEASE |
        XCB_EVENT_MASK_MOTION_NOTIFY |  // Mouse movement
        XCB_EVENT_MASK_KEY_PRESS |      // Keyboard
        XCB_EVENT_MASK_KEY_RELEASE |
        XCB_EVENT_MASK_STRUCTURE_NOTIFY // Window resizing
    };

    xcb_create_window(
        win->conn,
        XCB_COPY_FROM_PARENT,           // Depth
        win->window,
        win->screen->root,              // Parent window
        0, 0,                           // x, y
        width, height,                  // Width, height
        0,                              // Border width
        XCB_WINDOW_CLASS_INPUT_OUTPUT,
        win->screen->root_visual,
        mask,
        values
    );

    // Set window title
    xcb_change_property(
        win->conn,
        XCB_PROP_MODE_REPLACE,
        win->window,
        XCB_ATOM_WM_NAME,
        XCB_ATOM_STRING,
        8,
        strlen(title),
        (const uint8_t *)title
    );

    // Setup WM_PROTOCOLS for graceful window close
    win->wm_protocols = xcb_get_atom(win->conn, "WM_PROTOCOLS");
    win->wm_delete_window = xcb_get_atom(win->conn, "WM_DELETE_WINDOW");

    xcb_change_property(
        win->conn,
        XCB_PROP_MODE_REPLACE,
        win->window,
        win->wm_protocols,
        XCB_ATOM_ATOM,
        32,
        1,
        (const uint8_t *)&win->wm_delete_window
    );

    // Create graphics context
    win->gc = xcb_generate_id(win->conn);
    xcb_create_gc(
        win->conn,
        win->gc,
        win->window,
        0,
        NULL
    );

    // Allocate pixel data for back buffer
    win->pixel_data = calloc(width * height, sizeof(uint32_t));

    // Initialize XShm extension
    xcb_shm_query_version_reply_t *shm_reply = xcb_shm_query_version_reply(
        win->conn,
        xcb_shm_query_version(win->conn),
        NULL
    );
    win->shm_available = (shm_reply != NULL);
    free(shm_reply);

    // Create damage tracker
    win->damage = damage_tracker_new(width, height);

    // Display window
    xcb_map_window(win->conn, win->window);
    xcb_flush(win->conn);

    return win;
}

void xcb_window_destroy(XcbWindow *win) {
    if (win == NULL) return;

    if (win->damage) damage_tracker_free(win->damage);
    free(win->pixel_data);

    xcb_free_gc(win->conn, win->gc);
    xcb_destroy_window(win->conn, win->window);
    xcb_disconnect(win->conn);

    free(win);
}
```

### 1.3 Event Loop

```c
// silksurf-gui/xcb/event_loop.c

void xcb_window_run_event_loop(XcbWindow *win, void (*on_frame)(XcbWindow *win)) {
    win->running = 1;
    struct timespec last_frame_time = { 0 };
    clock_gettime(CLOCK_MONOTONIC, &last_frame_time);

    while (win->running) {
        // Process all pending events (non-blocking)
        xcb_generic_event_t *event;
        while ((event = xcb_poll_for_event(win->conn)) != NULL) {
            xcb_window_handle_event(win, event);
            free(event);
        }

        // Frame callback
        if (on_frame) {
            on_frame(win);
        }

        // Swap buffers (only damaged regions)
        xcb_window_swap_buffers(win);

        // Frame rate limiting (60 FPS = 16.67ms per frame)
        struct timespec now;
        clock_gettime(CLOCK_MONOTONIC, &now);
        uint64_t elapsed_us =
            (now.tv_sec - last_frame_time.tv_sec) * 1000000 +
            (now.tv_nsec - last_frame_time.tv_nsec) / 1000;

        uint64_t target_us = 1000000 / 60;  // ~16,667 us
        if (elapsed_us < target_us) {
            usleep(target_us - elapsed_us);
        }

        clock_gettime(CLOCK_MONOTONIC, &last_frame_time);
    }
}

static void xcb_window_handle_event(XcbWindow *win, xcb_generic_event_t *event) {
    uint8_t event_type = event->response_type & ~0x80;

    switch (event_type) {
        case XCB_EXPOSE: {
            // Window needs redraw
            xcb_expose_event_t *expose = (xcb_expose_event_t *)event;
            damage_track_rect(win->damage, expose->x, expose->y, expose->width, expose->height);
            break;
        }

        case XCB_BUTTON_PRESS: {
            xcb_button_press_event_t *bp = (xcb_button_press_event_t *)event;
            win->input.mouse_x = bp->event_x;
            win->input.mouse_y = bp->event_y;
            win->input.button_pressed = bp->detail;
            // Hit test and dispatch to widgets
            break;
        }

        case XCB_BUTTON_RELEASE: {
            xcb_button_release_event_t *br = (xcb_button_release_event_t *)event;
            win->input.button_pressed = 0;
            break;
        }

        case XCB_MOTION_NOTIFY: {
            xcb_motion_notify_event_t *mn = (xcb_motion_notify_event_t *)event;
            win->input.mouse_x = mn->event_x;
            win->input.mouse_y = mn->event_y;
            // Update hover state, dispatch to widgets
            break;
        }

        case XCB_KEY_PRESS: {
            xcb_key_press_event_t *kp = (xcb_key_press_event_t *)event;
            win->input.key_state[kp->detail] = 1;
            // Dispatch key event to focused widget
            break;
        }

        case XCB_KEY_RELEASE: {
            xcb_key_release_event_t *kr = (xcb_key_release_event_t *)event;
            win->input.key_state[kr->detail] = 0;
            break;
        }

        case XCB_CONFIGURE_NOTIFY: {
            xcb_configure_notify_event_t *cn = (xcb_configure_notify_event_t *)event;
            if (cn->width != win->width || cn->height != win->height) {
                // Window resized
                win->width = cn->width;
                win->height = cn->height;
                // Reallocate buffers, trigger layout
                free(win->pixel_data);
                win->pixel_data = calloc(win->width * win->height, sizeof(uint32_t));
                damage_track_rect(win->damage, 0, 0, win->width, win->height);
            }
            break;
        }

        case XCB_CLIENT_MESSAGE: {
            xcb_client_message_event_t *cm = (xcb_client_message_event_t *)event;
            if (cm->type == win->wm_protocols &&
                cm->data.data32[0] == win->wm_delete_window) {
                // User clicked close button
                win->running = 0;
            }
            break;
        }
    }
}
```

================================================================================
PART 2: DOUBLE-BUFFERING & PIXMAP MANAGEMENT
================================================================================

### 2.1 Buffer Swapping

```c
// silksurf-gui/xcb/buffer.h

#ifndef XCB_BUFFER_H
#define XCB_BUFFER_H

#include <xcb/xcb.h>
#include <stdint.h>

typedef struct {
    uint32_t *pixels;
    int width;
    int height;
    int pitch;  // Bytes per row
} PixelBuffer;

typedef struct {
    xcb_connection_t *conn;
    xcb_window_t window;
    xcb_gcontext_t gc;

    PixelBuffer back_buffer;
    PixelBuffer front_buffer;

    // For double-buffering with XShm
    int use_shm;
    xcb_pixmap_t shm_pixmap;
    void *shm_data;
} DoubleBuffer;

DoubleBuffer *double_buffer_create(xcb_connection_t *conn, xcb_window_t window, int width, int height);
void double_buffer_destroy(DoubleBuffer *buf);

// Swap buffers and blit to X11 (only damaged regions)
void double_buffer_swap(DoubleBuffer *buf, const DamageRect *rects, size_t rect_count);

#endif
```

### 2.2 Buffer Implementation

```c
// silksurf-gui/xcb/buffer.c

#include "buffer.h"
#include <stdlib.h>
#include <string.h>
#include <sys/shm.h>

DoubleBuffer *double_buffer_create(xcb_connection_t *conn, xcb_window_t window,
                                    int width, int height) {
    DoubleBuffer *buf = calloc(1, sizeof(DoubleBuffer));

    buf->conn = conn;
    buf->window = window;
    buf->gc = xcb_generate_id(conn);

    // Create GC
    xcb_create_gc(conn, buf->gc, window, 0, NULL);

    // Allocate back buffer
    buf->back_buffer.width = width;
    buf->back_buffer.height = height;
    buf->back_buffer.pitch = width * 4;  // 32-bit ARGB
    buf->back_buffer.pixels = calloc(width * height, sizeof(uint32_t));

    // Allocate front buffer
    buf->front_buffer.width = width;
    buf->front_buffer.height = height;
    buf->front_buffer.pitch = width * 4;
    buf->front_buffer.pixels = calloc(width * height, sizeof(uint32_t));

    // Check for XShm support
    xcb_shm_query_version_reply_t *reply = xcb_shm_query_version_reply(
        conn,
        xcb_shm_query_version(conn),
        NULL
    );

    if (reply) {
        buf->use_shm = 1;
        free(reply);

        // Create SHM pixmap
        int shm_size = width * height * 4;
        int shm_id = shmget(IPC_PRIVATE, shm_size, IPC_CREAT | 0600);
        buf->shm_data = shmat(shm_id, NULL, 0);

        xcb_shm_segment_info_t *shminfo = &buf->shm_info;
        shminfo->shmid = shm_id;
        shminfo->shmaddr = buf->shm_data;
        shminfo->shmseg = xcb_generate_id(conn);

        xcb_shm_attach(conn, shminfo->shmseg, shminfo->shmid, 0);

        buf->shm_pixmap = xcb_generate_id(conn);
        xcb_shm_create_pixmap(
            conn,
            buf->shm_pixmap,
            window,
            width,
            height,
            24,  // Depth
            shminfo->shmseg,
            0    // Offset
        );
    } else {
        buf->use_shm = 0;
    }

    return buf;
}

void double_buffer_swap(DoubleBuffer *buf, const DamageRect *rects, size_t rect_count) {
    if (rect_count == 0) return;  // Nothing to blit

    // Swap buffer pointers
    PixelBuffer tmp = buf->back_buffer;
    buf->back_buffer = buf->front_buffer;
    buf->front_buffer = tmp;

    // Blit only damaged regions to X11
    if (buf->use_shm) {
        // Copy damaged regions to SHM pixmap
        for (size_t i = 0; i < rect_count; i++) {
            const DamageRect *rect = &rects[i];

            // Copy from front buffer to SHM
            for (int y = rect->y; y < rect->y + rect->height && y < buf->front_buffer.height; y++) {
                int src_offset = y * (buf->front_buffer.pitch / 4) + rect->x;
                int dst_offset = y * (buf->shm_info.shmaddr ? (buf->front_buffer.pitch / 4) : 0);
                memcpy(
                    (uint32_t *)buf->shm_data + dst_offset,
                    buf->front_buffer.pixels + src_offset,
                    rect->width * 4
                );
            }
        }

        // Blit SHM pixmap regions to window
        for (size_t i = 0; i < rect_count; i++) {
            const DamageRect *rect = &rects[i];
            xcb_copy_area(
                buf->conn,
                buf->shm_pixmap,
                buf->window,
                buf->gc,
                rect->x, rect->y,        // Source offset
                rect->x, rect->y,        // Destination offset
                rect->width,
                rect->height
            );
        }
    } else {
        // Fallback: slow socket-based copy (for systems without XShm)
        for (size_t i = 0; i < rect_count; i++) {
            const DamageRect *rect = &rects[i];

            uint8_t *image_data = malloc(rect->width * rect->height * 4);
            for (int y = rect->y; y < rect->y + rect->height; y++) {
                memcpy(
                    image_data + (y - rect->y) * rect->width * 4,
                    buf->front_buffer.pixels + y * buf->front_buffer.width + rect->x,
                    rect->width * 4
                );
            }

            xcb_put_image(
                buf->conn,
                XCB_IMAGE_FORMAT_Z_PIXMAP,
                buf->window,
                buf->gc,
                rect->width,
                rect->height,
                rect->x,
                rect->y,
                0,         // Left pad
                24,        // Depth
                rect->width * rect->height * 4,
                (uint8_t *)image_data
            );

            free(image_data);
        }
    }

    xcb_flush(buf->conn);
}

void double_buffer_destroy(DoubleBuffer *buf) {
    if (buf == NULL) return;

    free(buf->back_buffer.pixels);
    free(buf->front_buffer.pixels);

    if (buf->use_shm) {
        xcb_shm_detach(buf->conn, buf->shm_info.shmseg);
        shmdt(buf->shm_data);
        xcb_free_pixmap(buf->conn, buf->shm_pixmap);
    }

    xcb_free_gc(buf->conn, buf->gc);
    free(buf);
}
```

================================================================================
PART 3: WIDGET ARCHITECTURE
================================================================================

### 3.1 Base Widget Class

```c
// silksurf-gui/widget/widget.h

#ifndef WIDGET_H
#define WIDGET_H

#include <stdint.h>
#include <stdbool.h>

typedef struct Widget Widget;

typedef void (*WidgetDrawFn)(Widget *widget, uint32_t *pixels, int pitch);
typedef bool (*WidgetHandleEventFn)(Widget *widget, const struct XcbEvent *event);
typedef void (*WidgetDestroyFn)(Widget *widget);

typedef struct {
    int x, y;
    int width, height;
} WidgetRect;

typedef struct {
    int margin_top, margin_right, margin_bottom, margin_left;
    int padding_top, padding_right, padding_bottom, padding_left;
} WidgetSpacing;

struct Widget {
    // Geometry
    WidgetRect bounds;
    WidgetSpacing spacing;

    // State
    bool visible;
    bool enabled;
    bool hovered;
    bool focused;

    // Hierarchy
    Widget *parent;
    Widget *first_child;
    Widget *last_child;
    Widget *next_sibling;

    // Rendering & event handling
    WidgetDrawFn draw;
    WidgetHandleEventFn handle_event;
    WidgetDestroyFn destroy;

    // Custom data
    void *user_data;

    // Dirty state
    bool dirty;
};

// Widget lifecycle
Widget *widget_new(void);
void widget_free(Widget *widget);

// Hierarchy
void widget_add_child(Widget *parent, Widget *child);
void widget_remove_child(Widget *parent, Widget *child);

// Geometry
void widget_set_bounds(Widget *widget, int x, int y, int width, int height);
void widget_layout(Widget *widget);

// Rendering
void widget_draw(Widget *widget, uint32_t *pixels, int pitch, int viewport_width, int viewport_height);

// Events
bool widget_handle_event(Widget *widget, const struct XcbEvent *event);

// State
void widget_set_visible(Widget *widget, bool visible);
void widget_set_enabled(Widget *widget, bool enabled);
void widget_set_focused(Widget *widget, bool focused);

// Hit testing
Widget *widget_hit_test(Widget *widget, int x, int y);

// Dirty state
void widget_mark_dirty(Widget *widget);
void widget_clear_dirty(Widget *widget);

#endif
```

### 3.2 Standard Widgets

```c
// silksurf-gui/widget/button.h

#ifndef WIDGET_BUTTON_H
#define WIDGET_BUTTON_H

#include "widget.h"

typedef struct {
    char *label;
    void (*on_click)(Widget *button);
    uint32_t bg_color;
    uint32_t text_color;
    uint32_t hover_color;
} ButtonData;

Widget *widget_button_new(const char *label, void (*on_click)(Widget *));
void widget_button_set_label(Widget *button, const char *label);

#endif

// silksurf-gui/widget/textinput.h

#ifndef WIDGET_TEXTINPUT_H
#define WIDGET_TEXTINPUT_H

#include "widget.h"

typedef struct {
    char *text;
    size_t text_len;
    size_t text_capacity;
    int cursor_pos;
    char placeholder[256];
    void (*on_change)(Widget *input);
} TextInputData;

Widget *widget_textinput_new(void);
const char *widget_textinput_get_text(Widget *input);
void widget_textinput_set_text(Widget *input, const char *text);

#endif

// silksurf-gui/widget/label.h

#ifndef WIDGET_LABEL_H
#define WIDGET_LABEL_H

#include "widget.h"

typedef struct {
    char *text;
    uint32_t text_color;
    int text_size;  // Font size in pixels
} LabelData;

Widget *widget_label_new(const char *text);
void widget_label_set_text(Widget *label, const char *text);

#endif

// silksurf-gui/widget/scrollbar.h

#ifndef WIDGET_SCROLLBAR_H
#define WIDGET_SCROLLBAR_H

#include "widget.h"

typedef struct {
    float scroll_pos;     // 0.0 to 1.0
    float viewport_ratio; // Height of viewport / total height
    void (*on_scroll)(Widget *scrollbar, float pos);
} ScrollBarData;

Widget *widget_scrollbar_new(float initial_pos, float viewport_ratio);
float widget_scrollbar_get_position(Widget *scrollbar);

#endif
```

### 3.3 Widget Implementation

```c
// silksurf-gui/widget/button.c

#include "button.h"
#include <stdlib.h>
#include <string.h>

static void button_draw(Widget *widget, uint32_t *pixels, int pitch) {
    ButtonData *data = (ButtonData *)widget->user_data;

    // Draw button background
    uint32_t bg = data->hovered ? data->hover_color : data->bg_color;
    for (int y = widget->bounds.y; y < widget->bounds.y + widget->bounds.height; y++) {
        for (int x = widget->bounds.x; x < widget->bounds.x + widget->bounds.width; x++) {
            pixels[y * (pitch / 4) + x] = bg;
        }
    }

    // Draw border
    for (int x = widget->bounds.x; x < widget->bounds.x + widget->bounds.width; x++) {
        pixels[widget->bounds.y * (pitch / 4) + x] = 0xFF000000;  // Top
        pixels[(widget->bounds.y + widget->bounds.height - 1) * (pitch / 4) + x] = 0xFF000000;  // Bottom
    }
    for (int y = widget->bounds.y; y < widget->bounds.y + widget->bounds.height; y++) {
        pixels[y * (pitch / 4) + widget->bounds.x] = 0xFF000000;  // Left
        pixels[y * (pitch / 4) + widget->bounds.x + widget->bounds.width - 1] = 0xFF000000;  // Right
    }

    // Draw label (simplified text rendering)
    // In real implementation, use freetype or bitmap fonts
    if (data->label) {
        int text_x = widget->bounds.x + 8;
        int text_y = widget->bounds.y + (widget->bounds.height - 16) / 2;
        // Paint text at (text_x, text_y) with color data->text_color
    }
}

static bool button_handle_event(Widget *widget, const struct XcbEvent *event) {
    ButtonData *data = (ButtonData *)widget->user_data;

    // Simple click handling
    if (event->type == XCB_BUTTON_PRESS) {
        if (data->on_click) {
            data->on_click(widget);
            return true;
        }
    }

    return false;
}

static void button_destroy(Widget *widget) {
    ButtonData *data = (ButtonData *)widget->user_data;
    if (data) {
        free(data->label);
        free(data);
    }
}

Widget *widget_button_new(const char *label, void (*on_click)(Widget *)) {
    Widget *button = widget_new();

    ButtonData *data = calloc(1, sizeof(ButtonData));
    data->label = strdup(label);
    data->on_click = on_click;
    data->bg_color = 0xFFCCCCCC;
    data->text_color = 0xFF000000;
    data->hover_color = 0xFFAAAAAA;

    button->user_data = data;
    button->draw = button_draw;
    button->handle_event = button_handle_event;
    button->destroy = button_destroy;

    button->bounds.width = 100;
    button->bounds.height = 30;

    return button;
}
```

================================================================================
PART 4: DAMAGE RECT MERGING ALGORITHM
================================================================================

### 4.1 Damage Tracking

```c
// silksurf-gui/xcb/damage.h

#ifndef DAMAGE_TRACKING_H
#define DAMAGE_TRACKING_H

#include <stdint.h>

typedef struct {
    int32_t x, y;
    int32_t width, height;
} DamageRect;

typedef struct {
    DamageRect *rects;
    size_t count;
    size_t capacity;
} DamageList;

typedef struct {
    DamageList current;
    DamageList accumulated;
    int viewport_width;
    int viewport_height;
} DamageTracker;

DamageTracker *damage_tracker_new(int width, int height);
void damage_tracker_free(DamageTracker *tracker);

void damage_track_rect(DamageTracker *tracker, int x, int y, int width, int height);
void damage_track_element(DamageTracker *tracker, const Widget *widget);

// Merge overlapping and adjacent rects
void damage_merge_rects(DamageList *list);

// Get bounding rect of all damage
DamageRect damage_get_union(const DamageTracker *tracker);

// Check intersection
bool damage_rect_intersect(const DamageRect *a, const DamageRect *b);

#endif
```

### 4.2 Merging Algorithm

```c
// silksurf-gui/xcb/damage.c

#include "damage.h"
#include <stdlib.h>
#include <string.h>

static int rect_compare(const void *a, const void *b) {
    const DamageRect *ra = (const DamageRect *)a;
    const DamageRect *rb = (const DamageRect *)b;
    // Sort by x, then y
    if (ra->x != rb->x) return ra->x - rb->x;
    return ra->y - rb->y;
}

void damage_merge_rects(DamageList *list) {
    if (list->count <= 1) return;

    // Sort rects by position
    qsort(list->rects, list->count, sizeof(DamageRect), rect_compare);

    // Merge overlapping and adjacent rects
    DamageList merged = { 0 };
    merged.rects = calloc(list->count, sizeof(DamageRect));
    merged.capacity = list->count;

    for (size_t i = 0; i < list->count; i++) {
        DamageRect current = list->rects[i];
        int merged_idx = -1;

        // Try to merge with existing merged rect
        for (size_t j = 0; j < merged.count; j++) {
            DamageRect *candidate = &merged.rects[j];

            // Check for overlap or adjacency
            if (!(current.x + current.width < candidate->x ||
                  candidate->x + candidate->width < current.x ||
                  current.y + current.height < candidate->y ||
                  candidate->y + candidate->height < current.y)) {

                // Merge: take union
                int left = current.x < candidate->x ? current.x : candidate->x;
                int right = (current.x + current.width) > (candidate->x + candidate->width) ?
                           (current.x + current.width) : (candidate->x + candidate->width);
                int top = current.y < candidate->y ? current.y : candidate->y;
                int bottom = (current.y + current.height) > (candidate->y + candidate->height) ?
                           (current.y + current.height) : (candidate->y + candidate->height);

                candidate->x = left;
                candidate->y = top;
                candidate->width = right - left;
                candidate->height = bottom - top;

                merged_idx = j;
                break;
            }
        }

        // No merge, add as new rect
        if (merged_idx == -1) {
            if (merged.count < merged.capacity) {
                merged.rects[merged.count++] = current;
            }
        }
    }

    // Swap lists
    free(list->rects);
    list->rects = merged.rects;
    list->count = merged.count;
    list->capacity = merged.capacity;
}

bool damage_rect_intersect(const DamageRect *a, const DamageRect *b) {
    return !(a->x + a->width <= b->x ||
             b->x + b->width <= a->x ||
             a->y + a->height <= b->y ||
             b->y + b->height <= a->y);
}
```

================================================================================
PART 5: DRI3 & GPU ACCELERATION (FUTURE)
================================================================================

### 5.1 DRI3 Integration (Phase 3+)

```c
// silksurf-gui/xcb/dri3.h

#ifndef DRI3_H
#define DRI3_H

#include <xcb/xcb.h>
#include <xcb/dri3.h>

typedef struct {
    xcb_connection_t *conn;
    xcb_window_t window;

    int drm_fd;
    struct gbm_device *gbm_dev;
    struct gbm_surface *gbm_surf;

    EGLDisplay egl_display;
    EGLContext egl_context;
    EGLSurface egl_surface;

    // Buffers
    struct gbm_bo *buffers[2];
    int current_buffer;
} Dri3Context;

// Future: GPU-accelerated rendering
Dri3Context *dri3_context_create(xcb_connection_t *conn, xcb_window_t window);
void dri3_context_destroy(Dri3Context *ctx);

void dri3_render_begin(Dri3Context *ctx);
void dri3_render_end(Dri3Context *ctx);

#endif
```

**Status**: Planned for Phase 3 (GPU acceleration layer)

================================================================================
END OF SILKSURF XCB GUI FRAMEWORK DESIGN DOCUMENT
================================================================================

**Status**: Complete (All major sections documented)
**Integration**: XCB GUI integrates with C Core rendering via damage tracking
**Next**: Modular CMake build system design (SILKSURF-BUILD-SYSTEM-DESIGN.md)
**Future**: DRI3/GPU acceleration (Phase 3+), Wayland support (Phase 4+)
