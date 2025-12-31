#ifndef SILKSURF_EVENT_LOOP_H
#define SILKSURF_EVENT_LOOP_H

#include "silksurf/events.h"
#include "silksurf/xcb_wrapper.h"

/* Event loop - main message pump */

/* Forward declarations */
struct silk_event_loop;

typedef struct silk_event_loop silk_event_loop_t;

/* Create and destroy event loop */
silk_event_loop_t *silk_event_loop_create(silk_display_t *display,
                                           size_t queue_capacity);
void silk_event_loop_destroy(silk_event_loop_t *loop);

/* Control event loop */
void silk_event_loop_stop(silk_event_loop_t *loop);
int silk_event_loop_is_running(silk_event_loop_t *loop);

/* Poll for events from X server */
int silk_event_loop_poll(silk_event_loop_t *loop);

/* Get next event from queue */
int silk_event_loop_get_event(silk_event_loop_t *loop, silk_event_t *event);

/* Check if events are available */
int silk_event_loop_has_events(silk_event_loop_t *loop);

#endif
