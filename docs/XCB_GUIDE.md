# XCB Programmer's Guide for SilkSurf

**Purpose**: Low-level X Window System API reference optimized for browser rendering

---

## 1. XCB vs Xlib: Why XCB?

### Comparison

| Aspect | Xlib | XCB |
|--------|------|-----|
| Protocol coverage | 95% | 100% |
| Latency | High (synchronous) | Low (asynchronous) |
| Memory | Large | Small |
| Dependencies | libx11 | libxcb (minimal) |
| Batch requests | Poor | Excellent |
| Thread safety | Poor | Good |
| Learning curve | Steep | Medium |

**For SilkSurf**: XCB chosen because:
1. Minimal dependencies (key to <10MB target)
2. Asynchronous API (allow UI responsiveness during rendering)
3. Better control over protocol batching
4. Explicit memory management (critical for optimization)

---

## 2. XCB Core Concepts

### 2.1 Connection & Screens

```c
#include <xcb/xcb.h>

// Open connection to X server
xcb_connection_t *conn = xcb_connect(NULL, NULL);
if (xcb_connection_has_error(conn)) {
    fprintf(stderr, "Cannot open display\n");
    exit(1);
}

// Get root screen
const xcb_setup_t *setup = xcb_get_setup(conn);
xcb_screen_iterator_t iter = xcb_setup_roots_iterator(setup);
xcb_screen_t *screen = iter.data;
```

**Key structs**:
- `xcb_connection_t` - Connection to X server
- `xcb_setup_t` - Server setup info
- `xcb_screen_t` - Display screen (width, height, depth, root)

### 2.2 Windows

```c
// Create window
uint32_t mask = XCB_CW_BACK_PIXEL | XCB_CW_EVENT_MASK;
uint32_t values[2] = {
    screen->black_pixel,      // Background
    XCB_EVENT_MASK_EXPOSURE | // Redraw on expose
    XCB_EVENT_MASK_KEY_PRESS  // Key events
};

xcb_window_t window = xcb_generate_id(conn);
xcb_create_window(conn,
    XCB_COPY_FROM_PARENT,    // depth
    window,
    screen->root,            // parent
    0, 0, 800, 600,          // x, y, width, height
    10,                       // border width
    XCB_WINDOW_CLASS_INPUT_OUTPUT,
    screen->root_visual,
    mask, values);

xcb_map_window(conn, window);
xcb_flush(conn);             // Flush output buffer to X server
```

**Key functions**:
- `xcb_generate_id()` - Get unique XID
- `xcb_create_window()` - Create window
- `xcb_map_window()` - Make visible
- `xcb_flush()` - Send buffered requests to server

### 2.3 Graphics Context (GC)

```c
// Create graphics context for drawing
xcb_gcontext_t gc = xcb_generate_id(conn);
uint32_t mask = XCB_GC_FOREGROUND | XCB_GC_BACKGROUND;
uint32_t values[2] = {
    screen->black_pixel,  // Foreground (drawing color)
    screen->white_pixel   // Background
};

xcb_create_gc(conn, gc, window, mask, values);
```

**Used for**: Drawing operations (rectangles, lines, text)

### 2.4 Events

```c
while (1) {
    xcb_generic_event_t *event = xcb_wait_for_event(conn);
    if (!event) break;
    
    switch (event->response_type & ~0x80) {
    case XCB_EXPOSE: {
        xcb_expose_event_t *expose = (xcb_expose_event_t *)event;
        // Redraw window region: expose->x, y, width, height
        break;
    }
    case XCB_BUTTON_PRESS: {
        xcb_button_press_event_t *press = (xcb_button_press_event_t *)event;
        handle_click(press->event_x, press->event_y);
        break;
    }
    case XCB_KEY_PRESS: {
        xcb_key_press_event_t *key = (xcb_key_press_event_t *)event;
        handle_key(key->detail);
        break;
    }
    }
    free(event);
}
```

**Event masks**:
- `XCB_EVENT_MASK_EXPOSURE` - Window needs redraw
- `XCB_EVENT_MASK_BUTTON_PRESS/RELEASE` - Mouse buttons
- `XCB_EVENT_MASK_KEY_PRESS/RELEASE` - Keyboard
- `XCB_EVENT_MASK_POINTER_MOTION` - Mouse movement
- `XCB_EVENT_MASK_STRUCTURE_NOTIFY` - Resize, etc.

---

## 3. Drawing Primitives

### 3.1 Basic Shapes

```c
// Draw rectangle
xcb_rectangle_t rect = {x, y, width, height};
xcb_poly_fill_rectangle(conn, window, gc, 1, &rect);

// Draw line
xcb_point_t points[2] = {{x1, y1}, {x2, y2}};
xcb_poly_line(conn, XCB_COORD_MODE_ORIGIN, window, gc, 2, points);

// Draw polygon
xcb_point_t poly[4] = {{0,0}, {10,0}, {10,10}, {0,10}};
xcb_poly_fill_polygon(conn, window, gc, XCB_POLY_SHAPE_CONVEX,
                      XCB_COORD_MODE_ORIGIN, 4, poly);
```

### 3.2 Images (Critical for browser)

```c
// Create pixmap (off-screen image)
xcb_pixmap_t pixmap = xcb_generate_id(conn);
xcb_create_pixmap(conn, screen->root_depth, pixmap, window, 800, 600);

// Put image data into pixmap
uint8_t data[800 * 600 * 4];  // RGBA
xcb_put_image(conn, XCB_IMAGE_FORMAT_Z_PIXMAP,
    pixmap, gc,
    800, 600,  // width, height
    0, 0,      // dst x, y
    24,        // left_pad
    32,        // depth
    800 * 600 * 4, data);

// Copy pixmap to window
xcb_copy_area(conn, pixmap, window, gc, 0, 0, 0, 0, 800, 600);
xcb_flush(conn);
```

---

## 4. Performance Optimization: XShm Extension

**Why XShm?** Shared memory transport avoids copying pixel data.

```c
#include <xcb/shm.h>
#include <sys/ipc.h>
#include <sys/shm.h>

// Check if XShm available
const xcb_query_extension_reply_t *reply =
    xcb_get_extension_data(conn, &xcb_shm_id);
if (!reply->present) {
    fprintf(stderr, "XShm not available\n");
}

// Create shared memory segment
int size = width * height * 4;  // RGBA
int shmid = shmget(IPC_PRIVATE, size, IPC_CREAT | 0600);
uint8_t *data = (uint8_t *)shmat(shmid, NULL, 0);

// Attach to X server
xcb_shm_segment_info_t shminfo;
shminfo.shmseg = xcb_generate_id(conn);
shminfo.shmid = shmid;
shminfo.read_only = 0;
xcb_shm_attach(conn, shminfo.shmseg, shmid, 0);

// Draw directly to shared memory
// (CPU renders here, no copying to X)
for (int i = 0; i < width * height; i++) {
    data[i*4+0] = R;  // Red
    data[i*4+1] = G;  // Green
    data[i*4+2] = B;  // Blue
    data[i*4+3] = 255; // Alpha
}

// Put image using shared memory (fast!)
xcb_shm_put_image(conn, window, gc, width, height,
    0, 0, width, height, 0, 0, shminfo.shmseg, 0);
xcb_flush(conn);
```

---

## 5. Damage Tracking (Novel Optimization)

**Why Damage Tracking?** Only redraw changed regions.

```c
#include <xcb/damage.h>

// Query damage extension
const xcb_query_extension_reply_t *damage_ext =
    xcb_get_extension_data(conn, &xcb_damage_id);

// Create damage region
xcb_damage_damage_t damage = xcb_generate_id(conn);
xcb_damage_create(conn, damage, window, XCB_DAMAGE_REPORT_RAW_RECTANGLES);

// Main loop
while (1) {
    xcb_generic_event_t *event = xcb_wait_for_event(conn);
    
    if (damage_ext->first_event + XCB_DAMAGE_NOTIFY ==
        (event->response_type & 0x7F)) {
        xcb_damage_notify_event_t *damage_event =
            (xcb_damage_notify_event_t *)event;
        
        // Get damaged region
        xcb_rectangle_t area = {
            damage_event->area.x,
            damage_event->area.y,
            damage_event->area.width,
            damage_event->area.height
        };
        
        // Redraw ONLY this region
        redraw_region(area);
        
        // Subtract damage (clear tracking)
        xcb_damage_subtract(conn, damage, XCB_NONE, XCB_NONE);
    }
}
```

---

## 6. Composite Extension (Double Buffering)

**Why Composite?** Atomic updates, flicker-free rendering.

```c
#include <xcb/composite.h>

// Check if composite available
const xcb_query_extension_reply_t *composite_ext =
    xcb_get_extension_data(conn, &xcb_composite_id);

// Redirect window (enable off-screen rendering)
xcb_composite_redirect_window(conn, window, XCB_COMPOSITE_REDIRECT_AUTOMATIC);

// Create pixmap for off-screen rendering
xcb_pixmap_t offscreen = xcb_generate_id(conn);
xcb_create_pixmap(conn, screen->root_depth, offscreen, window, width, height);

// Render to offscreen pixmap
// ... draw operations to offscreen ...

// Copy to window (atomic update)
xcb_gcontext_t gc = xcb_generate_id(conn);
xcb_create_gc(conn, gc, window, 0, NULL);
xcb_copy_area(conn, offscreen, window, gc, 0, 0, 0, 0, width, height);
xcb_flush(conn);
```

---

## 7. Key Optimization Techniques

### 7.1 Request Batching

```c
// BAD: 100 round-trips (slow)
for (int i = 0; i < 100; i++) {
    xcb_rectangle_t rect = {i*10, 0, 10, 10};
    xcb_poly_fill_rectangle(conn, window, gc, 1, &rect);
    xcb_flush(conn);  // FLUSH each time!
}

// GOOD: 1 round-trip (fast)
xcb_rectangle_t rects[100];
for (int i = 0; i < 100; i++) {
    rects[i] = (xcb_rectangle_t){i*10, 0, 10, 10};
}
xcb_poly_fill_rectangle(conn, window, gc, 100, rects);
xcb_flush(conn);  // ONE flush
```

**Key insight**: Every `xcb_flush()` causes round-trip to server. Batch requests!

### 7.2 Pixmap Caching

```c
struct pixmap_cache {
    xcb_pixmap_t pixmap;
    uint32_t hash;        // Content hash
    int width, height;
    int ref_count;        // Reference counting
};

// Store rendered content
cache_pixmap("image.png", pixmap_data, 800, 600);

// Reuse if unchanged
xcb_pixmap_t cached = lookup_pixmap("image.png");
if (cached) {
    xcb_copy_area(conn, cached, window, gc, 0, 0, 0, 0, 800, 600);
}
```

### 7.3 Region-Based Rendering

```c
// Track dirty regions
struct region {
    int x, y, width, height;
};

struct region dirty[MAX_REGIONS];
int dirty_count = 0;

// Mark region as dirty
void mark_dirty(int x, int y, int w, int h) {
    dirty[dirty_count++] = (struct region){x, y, w, h};
}

// Only redraw dirty regions
void flush() {
    for (int i = 0; i < dirty_count; i++) {
        struct region r = dirty[i];
        // Redraw only r.x, r.y, r.width, r.height
        redraw_area(r);
    }
    dirty_count = 0;
}
```

---

## 8. Window Manager Integration

### 8.1 Window Properties

```c
// Set window title
xcb_change_property(conn, XCB_PROP_MODE_REPLACE, window,
    XCB_ATOM_WM_NAME, XCB_ATOM_STRING,
    8, 14, "SilkSurf Browser");

// Set window hints
xcb_size_hints_t hints = {0};
hints.flags = XCB_ICCCM_SIZE_HINT_P_SIZE | XCB_ICCCM_SIZE_HINT_P_POSITION;
hints.x = 100; hints.y = 100;
hints.width = 800; hints.height = 600;
xcb_icccm_set_wm_normal_hints(conn, window, &hints);

// Handle window close
xcb_atom_t protocols = xcb_intern_atom(conn, 0, 12, "WM_PROTOCOLS").atom;
xcb_atom_t del_window = xcb_intern_atom(conn, 0, 16, "WM_DELETE_WINDOW").atom;
xcb_change_property(conn, XCB_PROP_MODE_REPLACE, window,
    protocols, XCB_ATOM_ATOM, 32, 1, &del_window);
```

---

## 9. Error Handling

```c
xcb_generic_error_t *error;

// Asynchronous request with error handling
xcb_void_cookie_t cookie = xcb_create_window_checked(conn, ...);
error = xcb_request_check(conn, cookie);
if (error) {
    fprintf(stderr, "X error: %d\n", error->error_code);
    free(error);
}
```

---

## 10. SilkSurf XCB Wrapper Design

**Goal**: Thin abstraction over XCB for browser use

```c
// silksurf/gui.h
typedef struct {
    xcb_connection_t *conn;
    xcb_window_t win;
    xcb_gcontext_t gc;
    xcb_screen_t *screen;
} silk_display_t;

typedef struct {
    xcb_pixmap_t pixmap;
    int width, height;
} silk_pixmap_t;

// High-level API
silk_display_t *silk_display_create(int width, int height);
void silk_display_draw_rect(silk_display_t *d, int x, int y, int w, int h, uint32_t color);
void silk_display_flush(silk_display_t *d);
void silk_display_destroy(silk_display_t *d);
```

---

## References

1. **X11 Protocol Specification**: https://www.x.org/releases/current/doc/x11proto/x11proto.txt
2. **XCB API Manual**: https://xcb.freedesktop.org/
3. **XCB Examples**: https://github.com/freedesktop/xcb-example
4. **Pixman (image manipulation)**: https://pixman.org/

