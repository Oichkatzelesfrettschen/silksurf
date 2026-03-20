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
#include "silksurf/document.h"
#include "silksurf/dom_node.h"
#include "silksurf/css_parser.h"

/* SilkSurf Browser - Phase 3 rendering pipeline */

static const char *test_html =
    "<html><body style=\"background: red\">"
    "<p>Hello, SilkSurf!</p>"
    "</body></html>";

static const char *test_css =
    "body { background-color: #cc3333; margin: 0px; padding: 20px; }\n"
    "p { color: white; background-color: #336699; padding: 10px; margin: 10px; width: 300px; height: 40px; }\n";

static void silk_main_handle_event(silk_event_t *event) {
    switch (event->type) {
    case SILK_EVENT_QUIT:
        break;
    case SILK_EVENT_KEY_PRESS:
        printf("Key: %u\n", event->data.key.keycode);
        break;
    default:
        break;
    }
}

int main(int argc, char *argv[]) {
    (void)argc; (void)argv;

    printf("SilkSurf Browser - Phase 3 (Native CSS Pipeline)\n");
    printf("=================================================\n\n");

    /* Initialize arena */
    silk_arena_t *arena = silk_arena_create(SILK_ARENA_SIZE);
    if (!arena) { fprintf(stderr, "Failed to create arena\n"); return 1; }

    /* Initialize JS Engine */
    silk_js_context_t js_ctx = silk_js_init();
    if (js_ctx) {
        printf("JS Engine initialized\n");
        silk_js_eval(js_ctx, "1 + 2");
    }

    /* Create window */
    silk_window_mgr_t *win_mgr = silk_window_mgr_create(NULL);
    if (!win_mgr) { fprintf(stderr, "Failed to create window manager\n"); return 1; }

    silk_app_window_t *window = silk_window_mgr_create_window(
        win_mgr, "SilkSurf", SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);
    if (!window) { fprintf(stderr, "Failed to create window\n"); return 1; }

    silk_renderer_t *renderer = silk_renderer_create(
        win_mgr, window, 16 * 1024 * 1024);
    if (!renderer) { fprintf(stderr, "Failed to create renderer\n"); return 1; }

    silk_window_show(window);

    /* Load document */
    silk_document_t *doc = silk_document_create(4 * 1024 * 1024);
    if (!doc) { fprintf(stderr, "Failed to create document\n"); return 1; }

    silk_document_set_renderer(doc, renderer);

    if (silk_document_load_html(doc, test_html, strlen(test_html)) < 0) {
        fprintf(stderr, "Failed to load HTML\n");
        return 1;
    }

    /* Apply CSS */
    silk_arena_t *css_arena = silk_document_get_arena(doc);
    silk_css_engine_t *engine = silk_css_engine_create(css_arena);
    if (engine) {
        silk_css_parse_string(engine, test_css, strlen(test_css));
        silk_css_apply_document_styles(engine, doc);
    }

    /* Layout the document */
    silk_document_layout(doc, SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);

    printf("Document loaded and laid out\n");

    /* Create event loop */
    silk_event_loop_t *event_loop = silk_event_loop_create(
        silk_display_open(NULL), 64);
    if (!event_loop) { fprintf(stderr, "Failed to create event loop\n"); return 1; }

    /* Main event loop */
    int running = 1;
    int frame_count = 0;
    printf("Event loop running...\n");

    while (running && silk_event_loop_is_running(event_loop)) {
        silk_event_loop_poll(event_loop);

        silk_event_t event;
        while (silk_event_loop_get_event(event_loop, &event)) {
            silk_main_handle_event(&event);
            if (event.type == SILK_EVENT_QUIT) running = 0;
        }

        /* Render document */
        silk_document_render(doc);
        frame_count++;

        usleep(16666);

        if (frame_count % 300 == 0) {
            printf("Frames: %d, Memory: %zu KB\n",
                   frame_count, silk_arena_used(arena) / 1024);
        }
    }

    /* Cleanup */
    printf("\nShutting down...\n");
    if (engine) silk_css_engine_destroy(engine);
    if (js_ctx) silk_js_destroy(js_ctx);
    silk_event_loop_destroy(event_loop);
    silk_document_destroy(doc);
    silk_renderer_destroy(renderer);
    silk_window_mgr_close_window(win_mgr, window);
    silk_window_mgr_destroy(win_mgr);
    silk_arena_destroy(arena);

    printf("SilkSurf shutdown complete.\n");
    return 0;
}
