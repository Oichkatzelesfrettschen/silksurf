/*
 * silksurf-webview -- fetch a URL and render it through SilkSurf's pipeline
 *
 * Why:  Integration test and milestone marker for SilkSurf. Fetches real
 *       web pages via libcurl and renders them through the HTML/CSS/layout
 *       pipeline. Shows exactly where the engine stands against real-world
 *       content (e.g., chatgpt.com).
 *
 * What: URL fetcher (libcurl) -> HTML parser (libhubbub/libdom) -> CSS
 *       engine (libcss) -> layout -> XCB renderer. Displays whatever
 *       SilkSurf can currently handle; gracefully degrades on unsupported
 *       features.
 *
 * How:  cmake --build build --target silksurf-webview
 *       ./build/silksurf-webview https://chatgpt.com
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <curl/curl.h>

#include "silksurf/config.h"
#include "silksurf/allocator.h"
#include "silksurf/window.h"
#include "silksurf/events.h"
#include "silksurf/event_loop.h"
#include "silksurf/xcb_wrapper.h"
#include "silksurf/renderer.h"
#include "silksurf/document.h"
#include "silksurf/css_parser.h"

#define WEBVIEW_DEFAULT_URL "https://chatgpt.com"
#define WEBVIEW_MAX_RESPONSE (16 * 1024 * 1024) /* 16 MB max page size */
#define WEBVIEW_USER_AGENT  "SilkSurf/0.1 (X11; Linux x86_64)"

/* -- HTTP fetch ----------------------------------------------------------- */

typedef struct {
    char  *data;
    size_t len;
    size_t cap;
} fetch_buf_t;

static size_t
curl_write_cb(void *ptr, size_t size, size_t nmemb, void *userdata)
{
    fetch_buf_t *buf = userdata;
    size_t chunk = size * nmemb;

    if (buf->len + chunk > WEBVIEW_MAX_RESPONSE)
        return 0; /* abort: page too large */

    if (buf->len + chunk >= buf->cap) {
        size_t newcap = buf->cap * 2;
        if (newcap < buf->len + chunk + 1)
            newcap = buf->len + chunk + 1;
        char *tmp = realloc(buf->data, newcap);
        if (!tmp) return 0;
        buf->data = tmp;
        buf->cap = newcap;
    }

    memcpy(buf->data + buf->len, ptr, chunk);
    buf->len += chunk;
    buf->data[buf->len] = '\0';
    return chunk;
}

static int
fetch_url(const char *url, fetch_buf_t *out)
{
    CURL *curl = curl_easy_init();
    if (!curl) {
        fprintf(stderr, "curl_easy_init failed\n");
        return -1;
    }

    out->len = 0;
    out->cap = 64 * 1024; /* 64 KB initial */
    out->data = malloc(out->cap);
    if (!out->data) {
        curl_easy_cleanup(curl);
        return -1;
    }
    out->data[0] = '\0';

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curl_write_cb);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, out);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, WEBVIEW_USER_AGENT);
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(curl, CURLOPT_MAXREDIRS, 5L);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 10L);
    /* Accept gzip/deflate for smaller transfers */
    curl_easy_setopt(curl, CURLOPT_ACCEPT_ENCODING, "");

    CURLcode res = curl_easy_perform(curl);
    if (res != CURLE_OK) {
        fprintf(stderr, "fetch failed: %s\n", curl_easy_strerror(res));
        free(out->data);
        out->data = NULL;
        out->len = 0;
        curl_easy_cleanup(curl);
        return -1;
    }

    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
    printf("HTTP %ld, %zu bytes fetched\n", http_code, out->len);

    curl_easy_cleanup(curl);
    return 0;
}

/* -- event handler -------------------------------------------------------- */

static void
handle_event(silk_event_t *event)
{
    switch (event->type) {
    case SILK_EVENT_KEY_PRESS:
        printf("Key: %u\n", event->data.key.keycode);
        break;
    default:
        break;
    }
}

/* -- main ----------------------------------------------------------------- */

int
main(int argc, char *argv[])
{
    const char *url = WEBVIEW_DEFAULT_URL;
    if (argc > 1)
        url = argv[1];

    printf("SilkSurf WebView - rendering pipeline test\n");
    printf("URL: %s\n", url);
    printf("===========================================\n\n");

    /* Fetch the page */
    curl_global_init(CURL_GLOBAL_DEFAULT);
    fetch_buf_t page = {0};
    if (fetch_url(url, &page) < 0) {
        fprintf(stderr, "Failed to fetch %s\n", url);
        curl_global_cleanup();
        return 1;
    }
    printf("Page fetched: %zu bytes\n\n", page.len);

    /* Initialize arena */
    silk_arena_t *arena = silk_arena_create(SILK_ARENA_SIZE);
    if (!arena) {
        fprintf(stderr, "Failed to create arena\n");
        free(page.data);
        curl_global_cleanup();
        return 1;
    }

    /* Create window */
    silk_window_mgr_t *win_mgr = silk_window_mgr_create(NULL);
    if (!win_mgr) {
        fprintf(stderr, "Failed to create window manager\n");
        goto cleanup_arena;
    }

    char title[256];
    snprintf(title, sizeof(title), "SilkSurf - %s", url);
    silk_app_window_t *window = silk_window_mgr_create_window(
        win_mgr, title, SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);
    if (!window) {
        fprintf(stderr, "Failed to create window\n");
        goto cleanup_win_mgr;
    }

    silk_renderer_t *renderer = silk_renderer_create(
        win_mgr, window, 16 * 1024 * 1024);
    if (!renderer) {
        fprintf(stderr, "Failed to create renderer\n");
        goto cleanup_window;
    }

    silk_window_show(window);

    /* Load document */
    silk_document_t *doc = silk_document_create(4 * 1024 * 1024);
    if (!doc) {
        fprintf(stderr, "Failed to create document\n");
        goto cleanup_renderer;
    }

    silk_document_set_renderer(doc, renderer);

    printf("Parsing HTML...\n");
    int parse_rc = silk_document_load_html(doc, page.data, page.len);
    if (parse_rc < 0) {
        fprintf(stderr, "HTML parse failed (rc=%d) -- page may use "
                "unsupported features\n", parse_rc);
        /* Continue anyway: render whatever we got */
    } else {
        const char *doc_title = silk_document_get_title(doc);
        if (doc_title)
            printf("Document title: %s\n", doc_title);
    }

    /* Apply CSS (extract inline styles from the parsed document) */
    silk_arena_t *css_arena = silk_document_get_arena(doc);
    silk_css_engine_t *engine = silk_css_engine_create(css_arena);
    if (engine) {
        printf("CSS engine initialized\n");
        silk_css_apply_document_styles(engine, doc);
    }

    /* Layout */
    printf("Running layout...\n");
    silk_document_layout(doc, SILK_SCREEN_WIDTH, SILK_SCREEN_HEIGHT);
    printf("Layout complete\n\n");

    /* Create event loop */
    silk_event_loop_t *event_loop = silk_event_loop_create(
        silk_display_open(NULL), 64);
    if (!event_loop) {
        fprintf(stderr, "Failed to create event loop\n");
        goto cleanup_doc;
    }

    /* Main render loop */
    int running = 1;
    int frame_count = 0;
    printf("Rendering... (press any key to see keycode, close window to quit)\n");

    while (running && silk_event_loop_is_running(event_loop)) {
        silk_event_loop_poll(event_loop);

        silk_event_t event;
        while (silk_event_loop_get_event(event_loop, &event)) {
            handle_event(&event);
            if (event.type == SILK_EVENT_QUIT)
                running = 0;
        }

        silk_document_render(doc);
        frame_count++;

        usleep(16666); /* ~60 FPS cap */

        if (frame_count % 300 == 0) {
            printf("Frames: %d, Arena: %zu KB\n",
                   frame_count, silk_arena_used(arena) / 1024);
        }
    }

    printf("\nRendered %d frames\n", frame_count);

    /* Cleanup (reverse order) */
    if (engine) silk_css_engine_destroy(engine);
    silk_event_loop_destroy(event_loop);

cleanup_doc:
    silk_document_destroy(doc);
cleanup_renderer:
    silk_renderer_destroy(renderer);
cleanup_window:
    silk_window_mgr_close_window(win_mgr, window);
cleanup_win_mgr:
    silk_window_mgr_destroy(win_mgr);
cleanup_arena:
    silk_arena_destroy(arena);
    free(page.data);
    curl_global_cleanup();

    printf("SilkSurf WebView shutdown complete.\n");
    return 0;
}
