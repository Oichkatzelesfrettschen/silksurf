#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include "silksurf/config.h"
#include "silksurf/allocator.h"
#include "silksurf/window.h"
#include "silksurf/events.h"
#include "silksurf/event_loop.h"
#include "silksurf/xcb_wrapper.h"
#include "silksurf/renderer.h"
#include "silksurf/pixel_ops.h"
#include "silksurf/js_engine.h"

/* SilkSurf Browser - Entry point with Phase 3 rendering pipeline */

/* Simple window event handler */
static void handle_event(silk_event_t *event) {
    switch (event->type) {
    case SILK_EVENT_QUIT:
        printf("Quit event\n");
        break;
    case SILK_EVENT_EXPOSE:
        printf("Expose: x=%d y=%d w=%d h=%d\n",
               event->data.expose.x, event->data.expose.y,
               event->data.expose.width, event->data.expose.height);
        break;
    case SILK_EVENT_KEY_PRESS:
        printf("Key press: code=%u mod=%u\n",
               event->data.key.keycode, event->data.key.modifiers);
        break;
    case SILK_EVENT_BUTTON_PRESS:
        printf("Button press: x=%d y=%d btn=%u\n",
               event->data.button.x, event->data.button.y,
               event->data.button.button);
        break;
    case SILK_EVENT_MOTION:
        printf("Mouse motion: x=%d y=%d\n",
               event->data.motion.x, event->data.motion.y);
        break;
    case SILK_EVENT_CONFIGURE:
        printf("Configure: w=%d h=%d\n",
               event->data.configure.width, event->data.configure.height);
        break;
    default:
        break;
    }
}

int main(int argc, char *argv[]) {
    printf("SilkSurf Browser - Phase 3 (Rendering Pipeline)\n");
    printf("================================================\n\n");

    /* Initialize arena allocator */
    printf("Initializing memory system...\n");
    silk_arena_t *arena = silk_arena_create(SILK_ARENA_SIZE);
    if (!arena) {
        fprintf(stderr, "Failed to create arena allocator\n");
        return 1;
    }
    printf("  Arena: %zu MB allocated\n", SILK_ARENA_SIZE / (1024 * 1024));

    /* Initialize JS Engine */
    printf("Initializing Rust JS Engine...\n");
    silk_js_context_t js_ctx = silk_js_init();
    if (js_ctx) {
        printf("  JS Engine initialized\n");
        printf("  Testing JS Eval: '1 + 2' = ");
        silk_js_eval(js_ctx, "1 + 2"); // Should print result to stdout
    } else {
        fprintf(stderr, "  Failed to initialize JS Engine\n");
    }

    /* Create window manager */
    printf("Initializing GUI system...\n");
    silk_window_mgr_t *win_mgr = silk_window_mgr_create(NULL);
    if (!win_mgr) {
        fprintf(stderr, "Failed to create window manager\n");
        silk_arena_destroy(arena);
        return 1;
    }
    printf("  X11 display opened\n");

    /* Create main window */
    silk_app_window_t *window = silk_window_mgr_create_window(
        win_mgr, "SilkSurf", SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);
    if (!window) {
        fprintf(stderr, "Failed to create window\n");
        silk_window_mgr_destroy(win_mgr);
        silk_arena_destroy(arena);
        return 1;
    }
    printf("  Window created: %d x %d\n", SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);

    /* Create renderer with damage tracking and pixmap cache */
    printf("Initializing rendering pipeline...\n");
    silk_renderer_t *renderer = silk_renderer_create(
        win_mgr, window, 16 * 1024 * 1024);  /* 16 MB pixmap cache */
    if (!renderer) {
        fprintf(stderr, "Failed to create renderer\n");
        silk_window_mgr_close_window(win_mgr, window);
        silk_window_mgr_destroy(win_mgr);
        silk_arena_destroy(arena);
        return 1;
    }
    printf("  Renderer initialized (%s backend)\n", silk_renderer_backend(renderer));

    /* Show window */
    silk_window_show(window);
    printf("  Window displayed\n");

    /* Create event loop */
    printf("Starting event loop...\n");
    silk_event_loop_t *event_loop = silk_event_loop_create(
        silk_display_open(NULL), 64);
    if (!event_loop) {
        fprintf(stderr, "Failed to create event loop\n");
        silk_renderer_destroy(renderer);
        silk_window_mgr_close_window(win_mgr, window);
        silk_window_mgr_destroy(win_mgr);
        silk_arena_destroy(arena);
        return 1;
    }

    /* Main event loop */
    int running = 1;
    int frame_count = 0;
    printf("\nEvent loop running (press Ctrl+C to quit, close window to exit):\n");

    while (running && silk_event_loop_is_running(event_loop)) {
        /* Begin rendering frame */
        silk_renderer_begin_frame(renderer);

        /* Poll for XCB events */
        silk_event_loop_poll(event_loop);

        /* Process queued events */
        silk_event_t event;
        while (silk_event_loop_get_event(event_loop, &event)) {
            handle_event(&event);
            if (event.type == SILK_EVENT_QUIT)
                running = 0;
        }

        /* Render frame - demo: fill with gradient effect */
        if (frame_count == 0) {
            /* First frame: clear to dark background */
            silk_renderer_clear(renderer, SILK_COLOR_BLACK);
        } else if (frame_count % 60 == 0) {
            /* Every second: animate a rectangle */
            int x = (frame_count / 60) % SILK_SCREEN_WIDTH;
            silk_color_t color = silk_color(255, 64 + (x % 192), 128, 200);
            silk_renderer_fill_rect(renderer, x, 50, 50, 50, color);
        }

        /* End frame and present to screen */
        silk_renderer_end_frame(renderer);
        silk_renderer_present(renderer);
        frame_count++;

        /* Small sleep to avoid busy-waiting */
        usleep(16666);  /* ~60 FPS target */

        /* Periodically print stats */
        if (frame_count % 300 == 0) {
            printf("Frames: %d, Memory used: %zu KB, "
                   "Damage: %d%%, Cache: %d%% hit rate\n",
                   frame_count, silk_arena_used(arena) / 1024,
                   silk_renderer_damage_coverage_percent(renderer),
                   silk_renderer_cache_hit_rate(renderer));
        }
    }

    /* Cleanup */
    printf("\nShutting down...\n");
    if (js_ctx) {
        silk_js_destroy(js_ctx);
        printf("JS Engine destroyed\n");
    }
    silk_event_loop_destroy(event_loop);
    silk_renderer_destroy(renderer);
    silk_window_mgr_close_window(win_mgr, window);
    silk_window_mgr_destroy(win_mgr);

    /* Print final stats */
    printf("Memory statistics:\n");
    silk_arena_stats(arena);
    printf("\nFrames rendered: %d\n", frame_count);

    silk_arena_destroy(arena);

    printf("\nSilkSurf shutdown complete.\n");
    return 0;
}
