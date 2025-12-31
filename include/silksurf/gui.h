#ifndef SILKSURF_GUI_H
#define SILKSURF_GUI_H

#include <stdint.h>

/* XCB GUI framework - minimal, optimized */

typedef struct silk_display silk_display_t;
typedef struct silk_pixmap silk_pixmap_t;
typedef struct silk_event silk_event_t;

/* Event types */
typedef enum {
    SILK_EVENT_EXPOSE,
    SILK_EVENT_KEY_PRESS,
    SILK_EVENT_KEY_RELEASE,
    SILK_EVENT_BUTTON_PRESS,
    SILK_EVENT_BUTTON_RELEASE,
    SILK_EVENT_MOTION,
    SILK_EVENT_CONFIGURE,
    SILK_EVENT_QUIT
} silk_event_type_t;

struct silk_event {
    silk_event_type_t type;
    union {
        struct {
            int x, y, width, height;
        } expose;
        struct {
            int x, y;
            int button;
        } button;
        struct {
            int keycode;
        } key;
    } data;
};

/* Display management */
silk_display_t *silk_display_create(int width, int height, const char *title);
void silk_display_destroy(silk_display_t *d);
void silk_display_clear(silk_display_t *d, uint32_t color);
void silk_display_flush(silk_display_t *d);

/* Drawing primitives */
void silk_draw_rect(silk_display_t *d, int x, int y, int w, int h, uint32_t color);
void silk_draw_line(silk_display_t *d, int x1, int y1, int x2, int y2, uint32_t color);
void silk_draw_image(silk_display_t *d, int x, int y, silk_pixmap_t *pm);

/* Pixmap operations */
silk_pixmap_t *silk_pixmap_create(silk_display_t *d, int w, int h);
void silk_pixmap_destroy(silk_display_t *d, silk_pixmap_t *pm);
void silk_pixmap_put_pixels(silk_display_t *d, silk_pixmap_t *pm, 
                             const uint32_t *data, int w, int h);

/* Event handling */
silk_event_t *silk_event_get(silk_display_t *d);
void silk_event_free(silk_event_t *e);

#endif
