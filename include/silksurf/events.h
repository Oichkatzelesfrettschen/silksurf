#ifndef SILKSURF_EVENTS_H
#define SILKSURF_EVENTS_H

#include <stdint.h>

/* Event system for GUI and input handling */

typedef enum {
    SILK_EVENT_QUIT,
    SILK_EVENT_EXPOSE,
    SILK_EVENT_KEY_PRESS,
    SILK_EVENT_KEY_RELEASE,
    SILK_EVENT_BUTTON_PRESS,
    SILK_EVENT_BUTTON_RELEASE,
    SILK_EVENT_MOTION,
    SILK_EVENT_CONFIGURE,
    SILK_EVENT_FOCUS_IN,
    SILK_EVENT_FOCUS_OUT,
} silk_event_type_t;

/* Button codes */
enum {
    SILK_BUTTON_LEFT = 1,
    SILK_BUTTON_MIDDLE = 2,
    SILK_BUTTON_RIGHT = 3,
    SILK_BUTTON_WHEEL_UP = 4,
    SILK_BUTTON_WHEEL_DOWN = 5,
};

/* Modifier keys */
enum {
    SILK_MOD_SHIFT = (1 << 0),
    SILK_MOD_CTRL = (1 << 2),
    SILK_MOD_ALT = (1 << 3),
    SILK_MOD_SUPER = (1 << 6),
};

typedef struct {
    silk_event_type_t type;
    void *window;
    union {
        struct {
            int x, y;
            int width, height;
        } expose;
        struct {
            uint32_t keysym;
            uint32_t keycode;
            uint32_t modifiers;
        } key;
        struct {
            int x, y;
            uint32_t button;
            uint32_t modifiers;
        } button;
        struct {
            int x, y;
            uint32_t modifiers;
        } motion;
        struct {
            int width, height;
        } configure;
    } data;
} silk_event_t;

/* Event queue */
typedef struct silk_event_queue silk_event_queue_t;

silk_event_queue_t *silk_event_queue_create(size_t capacity);
void silk_event_queue_destroy(silk_event_queue_t *queue);

int silk_event_queue_push(silk_event_queue_t *queue, const silk_event_t *ev);
int silk_event_queue_pop(silk_event_queue_t *queue, silk_event_t *ev);
int silk_event_queue_empty(silk_event_queue_t *queue);
int silk_event_queue_full(silk_event_queue_t *queue);

#endif
