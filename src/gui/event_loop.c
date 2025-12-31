#include <stdlib.h>
#include <xcb/xcb.h>
#include "silksurf/event_loop.h"
#include "silksurf/xcb_wrapper.h"

/* Event loop - translates XCB events to application events */

struct silk_event_loop {
    xcb_connection_t *conn;
    silk_display_t *display;
    silk_event_queue_t *queue;
    int running;
};

silk_event_loop_t *silk_event_loop_create(silk_display_t *display,
                                           size_t queue_capacity) {
    if (!display)
        return NULL;

    silk_event_loop_t *loop = malloc(sizeof(silk_event_loop_t));
    if (!loop)
        return NULL;

    loop->display = display;
    loop->conn = silk_display_get_conn(display);
    loop->queue = silk_event_queue_create(queue_capacity);
    loop->running = 1;

    if (!loop->queue) {
        free(loop);
        return NULL;
    }

    return loop;
}

void silk_event_loop_destroy(silk_event_loop_t *loop) {
    if (!loop)
        return;
    if (loop->queue)
        silk_event_queue_destroy(loop->queue);
    free(loop);
}

void silk_event_loop_stop(silk_event_loop_t *loop) {
    if (loop)
        loop->running = 0;
}

int silk_event_loop_is_running(silk_event_loop_t *loop) {
    return loop ? loop->running : 0;
}

/* Process a single XCB event and convert to silk event */
static int silk_event_from_xcb(xcb_generic_event_t *xcb_event,
                                silk_event_t *silk_event) {
    if (!xcb_event || !silk_event)
        return 0;

    uint8_t type = xcb_event->response_type & ~0x80;

    switch (type) {
    case XCB_EXPOSE: {
        xcb_expose_event_t *exp = (xcb_expose_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_EXPOSE;
        silk_event->data.expose.x = exp->x;
        silk_event->data.expose.y = exp->y;
        silk_event->data.expose.width = exp->width;
        silk_event->data.expose.height = exp->height;
        return 1;
    }

    case XCB_KEY_PRESS: {
        xcb_key_press_event_t *key = (xcb_key_press_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_KEY_PRESS;
        silk_event->data.key.keycode = key->detail;
        silk_event->data.key.modifiers = key->state;
        return 1;
    }

    case XCB_KEY_RELEASE: {
        xcb_key_release_event_t *key = (xcb_key_release_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_KEY_RELEASE;
        silk_event->data.key.keycode = key->detail;
        silk_event->data.key.modifiers = key->state;
        return 1;
    }

    case XCB_BUTTON_PRESS: {
        xcb_button_press_event_t *btn = (xcb_button_press_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_BUTTON_PRESS;
        silk_event->data.button.x = btn->event_x;
        silk_event->data.button.y = btn->event_y;
        silk_event->data.button.button = btn->detail;
        silk_event->data.button.modifiers = btn->state;
        return 1;
    }

    case XCB_BUTTON_RELEASE: {
        xcb_button_release_event_t *btn = (xcb_button_release_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_BUTTON_RELEASE;
        silk_event->data.button.x = btn->event_x;
        silk_event->data.button.y = btn->event_y;
        silk_event->data.button.button = btn->detail;
        silk_event->data.button.modifiers = btn->state;
        return 1;
    }

    case XCB_MOTION_NOTIFY: {
        xcb_motion_notify_event_t *mot = (xcb_motion_notify_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_MOTION;
        silk_event->data.motion.x = mot->event_x;
        silk_event->data.motion.y = mot->event_y;
        silk_event->data.motion.modifiers = mot->state;
        return 1;
    }

    case XCB_CONFIGURE_NOTIFY: {
        xcb_configure_notify_event_t *cfg = (xcb_configure_notify_event_t *)xcb_event;
        silk_event->type = SILK_EVENT_CONFIGURE;
        silk_event->data.configure.width = cfg->width;
        silk_event->data.configure.height = cfg->height;
        return 1;
    }

    case XCB_FOCUS_IN: {
        silk_event->type = SILK_EVENT_FOCUS_IN;
        return 1;
    }

    case XCB_FOCUS_OUT: {
        silk_event->type = SILK_EVENT_FOCUS_OUT;
        return 1;
    }

    case XCB_CLIENT_MESSAGE: {
        xcb_client_message_event_t *msg = (xcb_client_message_event_t *)xcb_event;
        /* Check for WM_DELETE_WINDOW */
        if (msg->data.data32[0] == XCB_ATOM_NONE) {
            silk_event->type = SILK_EVENT_QUIT;
            return 1;
        }
        break;
    }

    default:
        break;
    }

    return 0;  /* Event not handled */
}

/* Poll for events - non-blocking */
int silk_event_loop_poll(silk_event_loop_t *loop) {
    if (!loop || !loop->conn)
        return 0;

    xcb_generic_event_t *xcb_event;
    int event_count = 0;

    while ((xcb_event = xcb_poll_for_event(loop->conn)) != NULL) {
        silk_event_t silk_event;

        if (silk_event_from_xcb(xcb_event, &silk_event)) {
            if (silk_event_queue_push(loop->queue, &silk_event))
                event_count++;
        }

        free(xcb_event);
    }

    return event_count;
}

/* Get next event from queue */
int silk_event_loop_get_event(silk_event_loop_t *loop, silk_event_t *event) {
    if (!loop || !event)
        return 0;
    return silk_event_queue_pop(loop->queue, event);
}

/* Peek if events are available */
int silk_event_loop_has_events(silk_event_loop_t *loop) {
    if (!loop || !loop->queue)
        return 0;
    return !silk_event_queue_empty(loop->queue);
}
