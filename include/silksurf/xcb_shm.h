/**
 * \file xcb_shm.h
 * \brief XCB Shared Memory Extension Interface
 *
 * Zero-copy image upload to X server using SHM extension.
 * Provides 10x performance improvement over socket-based transfer.
 */

#ifndef SILK_XCB_SHM_H
#define SILK_XCB_SHM_H

#include <stdbool.h>
#include <stdint.h>

/* Opaque types */
typedef struct silk_display silk_display_t;
typedef struct silk_window silk_window_t;
typedef struct silk_gc silk_gc_t;
typedef struct {
    /* Hidden members - implementation detail */
    void *_internal;
} silk_shm_segment_t;

/**
 * Create a shared memory segment
 *
 * Allocates System V shared memory and registers with X server.
 * Returns NULL if XCB-SHM extension unavailable (fallback to socket).
 *
 * \param dpy Display connection
 * \param width Image width in pixels
 * \param height Image height in pixels
 * \param depth Color depth (must be 32 for RGBA)
 * \return Allocated segment, or NULL on failure/unavailable
 */
silk_shm_segment_t *silk_xcb_shm_create_segment(silk_display_t *dpy,
                                                 int width, int height,
                                                 uint8_t depth);

/**
 * Destroy a shared memory segment
 *
 * Cleans up both local and X server resources.
 *
 * \param dpy Display connection
 * \param seg Segment to destroy (pointer invalid after call)
 */
void silk_xcb_shm_destroy_segment(silk_display_t *dpy,
                                   silk_shm_segment_t *seg);

/**
 * Get writeable memory pointer
 *
 * Returns address where pixel data can be written.
 * Data is automatically visible to X server (zero-copy).
 *
 * \param seg Shared memory segment
 * \return Writeable buffer, or NULL if invalid
 */
void *silk_xcb_shm_get_data(silk_shm_segment_t *seg);

/**
 * Upload shared memory image to window
 *
 * Sends XCB command to copy SHM image to window.
 * Much faster than socket transport.
 *
 * \param dpy Display connection
 * \param win Target window
 * \param gc Graphics context
 * \param seg Source SHM segment (created by silk_xcb_shm_create_segment)
 * \param dst_x Destination X coordinate
 * \param dst_y Destination Y coordinate
 * \return true on success, false on failure
 */
bool silk_xcb_shm_put_image(silk_display_t *dpy, silk_window_t *win,
                             silk_gc_t *gc, silk_shm_segment_t *seg,
                             int dst_x, int dst_y);

/**
 * Detect image transfer backend
 *
 * Returns which transfer method is being used.
 * Useful for performance monitoring.
 *
 * \param dpy Display connection
 * \return "XShm" if SHM available, "Socket" as fallback
 */
const char *silk_xcb_image_backend(silk_display_t *dpy);

#endif  /* SILK_XCB_SHM_H */
