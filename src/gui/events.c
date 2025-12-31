#include <stdlib.h>
#include <string.h>
#include "silksurf/events.h"

/* Event queue - circular buffer for low-latency event handling */

struct silk_event_queue {
    silk_event_t *events;
    size_t capacity;
    size_t head;
    size_t tail;
    size_t count;
};

silk_event_queue_t *silk_event_queue_create(size_t capacity) {
    if (!capacity)
        return NULL;

    silk_event_queue_t *queue = malloc(sizeof(silk_event_queue_t));
    if (!queue)
        return NULL;

    queue->events = malloc(capacity * sizeof(silk_event_t));
    if (!queue->events) {
        free(queue);
        return NULL;
    }

    queue->capacity = capacity;
    queue->head = 0;
    queue->tail = 0;
    queue->count = 0;

    return queue;
}

void silk_event_queue_destroy(silk_event_queue_t *queue) {
    if (!queue)
        return;
    if (queue->events)
        free(queue->events);
    free(queue);
}

int silk_event_queue_push(silk_event_queue_t *queue, const silk_event_t *ev) {
    if (!queue || !ev || queue->count >= queue->capacity)
        return 0;  /* Queue full */

    queue->events[queue->tail] = *ev;
    queue->tail = (queue->tail + 1) % queue->capacity;
    queue->count++;

    return 1;  /* Success */
}

int silk_event_queue_pop(silk_event_queue_t *queue, silk_event_t *ev) {
    if (!queue || !ev || queue->count == 0)
        return 0;  /* Queue empty */

    *ev = queue->events[queue->head];
    queue->head = (queue->head + 1) % queue->capacity;
    queue->count--;

    return 1;  /* Success */
}

int silk_event_queue_empty(silk_event_queue_t *queue) {
    return !queue || queue->count == 0;
}

int silk_event_queue_full(silk_event_queue_t *queue) {
    return queue && queue->count >= queue->capacity;
}
