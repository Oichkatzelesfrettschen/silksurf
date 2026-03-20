/* CSS Engine - Native pipeline with libcss parsing support
 *
 * Style computation uses the native CSS cascade engine exclusively.
 * LibCSS is kept only for its CSS parsing capabilities (converting CSS text
 * to structured data). The native pipeline handles:
 * - CSS tokenization + parsing (css_parser.c)
 * - Selector matching (css_selector_match.c via bridge)
 * - Cascade computation (css_cascade.c via bridge)
 */
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <libcss/libcss.h>
#include "silksurf/css_parser.h"
#include "silksurf/css_native_parser.h"
#include "silksurf/dom_node.h"
#include "silksurf/document.h"
#include "silksurf/allocator.h"
#include "css_native_bridge.h"

/* Forward declarations - defined in css_select_handler.c */
extern css_select_handler *silk_css_get_select_handler(void);
extern void silk_css_handler_reset(void);

struct silk_css_engine {
    silk_arena_t *arena;

    /* Native parsed stylesheets (primary) */
    css_parsed_stylesheet_t **native_sheets;
    int native_sheet_count;
    int native_sheet_capacity;

    /* LibCSS stylesheets (kept for backward compat in tests) */
    css_stylesheet **sheets;
    int sheet_count;
    int sheet_capacity;
    css_select_ctx *select_ctx;
};

/* ========== LIBCSS CALLBACKS ========== */

static css_error resolve_url(void *pw, const char *base,
                              lwc_string *rel, lwc_string **abs) {
    (void)pw; (void)base; (void)rel;
    *abs = NULL;
    return CSS_OK;
}

static css_error import_style(void *pw, css_stylesheet *parent, lwc_string *url) {
    (void)pw; (void)parent; (void)url;
    return CSS_OK;
}

static css_error font_callback(void *pw, lwc_string *name, css_system_font *system_font) {
    (void)pw; (void)name; (void)system_font;
    return CSS_OK;
}

static css_stylesheet_params sheet_params = {
    .params_version = CSS_STYLESHEET_PARAMS_VERSION_1,
    .level = CSS_LEVEL_DEFAULT,
    .charset = "UTF-8",
    .url = "silksurf://document",
    .title = NULL,
    .allow_quirks = false,
    .inline_style = false,
    .resolve = resolve_url,
    .resolve_pw = NULL,
    .import = import_style,
    .import_pw = NULL,
    .color = NULL,
    .color_pw = NULL,
    .font = font_callback,
    .font_pw = NULL
};

/* ========== UA STYLESHEET ========== */

static const char *ua_stylesheet_text =
    "html, body { display: block; margin: 0px; padding: 0px; }\n"
    "div, p, h1, h2, h3, h4, h5, h6, ul, ol, li, header, footer, main, section, article, nav, aside { display: block; }\n"
    "span, a, em, strong, b, i, u { display: inline; }\n"
    "h1 { font-size: 32px; margin-top: 10px; margin-bottom: 10px; }\n"
    "h2 { font-size: 24px; margin-top: 8px; margin-bottom: 8px; }\n"
    "h3 { font-size: 18px; margin-top: 6px; margin-bottom: 6px; }\n"
    "p { margin-top: 8px; margin-bottom: 8px; }\n"
    "body { background-color: white; color: black; font-size: 16px; }\n";

/* ========== CSS ENGINE API ========== */

silk_css_engine_t *silk_css_engine_create(silk_arena_t *arena) {
    if (!arena) return NULL;

    silk_css_engine_t *engine = silk_arena_alloc(arena, sizeof(silk_css_engine_t));
    if (!engine) return NULL;
    memset(engine, 0, sizeof(*engine));
    engine->arena = arena;

    /* Native sheet storage */
    engine->native_sheet_capacity = 8;
    engine->native_sheets = silk_arena_alloc(arena,
        sizeof(css_parsed_stylesheet_t *) * engine->native_sheet_capacity);
    if (!engine->native_sheets) return NULL;

    /* LibCSS storage */
    engine->sheet_capacity = 8;
    engine->sheets = silk_arena_alloc(arena, sizeof(css_stylesheet *) * engine->sheet_capacity);
    if (!engine->sheets) return NULL;

    css_error err = css_select_ctx_create(&engine->select_ctx);
    if (err != CSS_OK) return NULL;

    /* Parse UA stylesheet natively */
    css_parsed_stylesheet_t *ua = css_parse_stylesheet(arena,
        ua_stylesheet_text, strlen(ua_stylesheet_text));
    if (ua) engine->native_sheets[engine->native_sheet_count++] = ua;

    /* LibCSS UA (for backward compat in old tests) */
    css_stylesheet *ua_libcss = NULL;
    err = css_stylesheet_create(&sheet_params, &ua_libcss);
    if (err == CSS_OK) {
        const char *minimal = "html,body{display:block;margin:0;padding:0}"
                               "div,p,h1,h2,h3,h4,h5,h6{display:block}"
                               "span,a{display:inline}";
        err = css_stylesheet_append_data(ua_libcss, (const uint8_t *)minimal, strlen(minimal));
        if (err == CSS_OK) err = css_stylesheet_data_done(ua_libcss);
        if (err == CSS_OK) {
            err = css_select_ctx_append_sheet(engine->select_ctx, ua_libcss, CSS_ORIGIN_UA, "screen");
            if (err == CSS_OK) engine->sheets[engine->sheet_count++] = ua_libcss;
            else css_stylesheet_destroy(ua_libcss);
        } else {
            css_stylesheet_destroy(ua_libcss);
        }
    }

    return engine;
}

void silk_css_engine_destroy(silk_css_engine_t *engine) {
    if (!engine) return;
    silk_css_handler_reset();
    if (engine->select_ctx) { css_select_ctx_destroy(engine->select_ctx); engine->select_ctx = NULL; }
    for (int i = 0; i < engine->sheet_count; i++) {
        if (engine->sheets[i]) css_stylesheet_destroy(engine->sheets[i]);
    }
    engine->sheet_count = 0;
}

int silk_css_parse_string(silk_css_engine_t *engine, const char *css, size_t css_len) {
    if (!engine || !css || css_len == 0) return -1;

    /* Parse natively (primary path) */
    css_parsed_stylesheet_t *native = css_parse_stylesheet(engine->arena, css, css_len);
    if (native) {
        if (engine->native_sheet_count >= engine->native_sheet_capacity) {
            int nc = engine->native_sheet_capacity * 2;
            css_parsed_stylesheet_t **na = silk_arena_alloc(engine->arena,
                sizeof(css_parsed_stylesheet_t *) * nc);
            if (!na) return -1;
            memcpy(na, engine->native_sheets,
                sizeof(css_parsed_stylesheet_t *) * engine->native_sheet_count);
            engine->native_sheets = na;
            engine->native_sheet_capacity = nc;
        }
        engine->native_sheets[engine->native_sheet_count++] = native;
    }

    /* Also parse with libcss for backward compat */
    css_stylesheet *sheet = NULL;
    css_error err = css_stylesheet_create(&sheet_params, &sheet);
    if (err != CSS_OK) return -1;
    err = css_stylesheet_append_data(sheet, (const uint8_t *)css, css_len);
    if (err != CSS_OK && err != CSS_NEEDDATA) { css_stylesheet_destroy(sheet); return -1; }
    err = css_stylesheet_data_done(sheet);
    if (err != CSS_OK) { css_stylesheet_destroy(sheet); return -1; }
    if (engine->sheet_count >= engine->sheet_capacity) {
        int nc = engine->sheet_capacity * 2;
        css_stylesheet **na = silk_arena_alloc(engine->arena, sizeof(css_stylesheet *) * nc);
        if (!na) { css_stylesheet_destroy(sheet); return -1; }
        memcpy(na, engine->sheets, sizeof(css_stylesheet *) * engine->sheet_count);
        engine->sheets = na;
        engine->sheet_capacity = nc;
    }
    engine->sheets[engine->sheet_count++] = sheet;
    css_select_ctx_append_sheet(engine->select_ctx, sheet, CSS_ORIGIN_AUTHOR, "screen");

    return 0;
}

int silk_css_parse_style_element(silk_css_engine_t *engine, silk_dom_node_t *style_elem) {
    if (!engine || !style_elem) return -1;
    silk_dom_node_t *text_node = silk_dom_node_get_first_child(style_elem);
    if (!text_node) return 0;
    const char *css_text = silk_dom_node_get_text_content(text_node);
    if (!css_text || css_text[0] == '\0') return 0;
    return silk_css_parse_string(engine, css_text, strlen(css_text));
}

/* ========== STYLE COMPUTATION (via native bridge) ========== */

int silk_css_get_computed_style(silk_css_engine_t *engine,
                                 silk_dom_node_t *element,
                                 silk_computed_style_t *out_style) {
    if (!engine || !element || !out_style) return -1;
    memset(out_style, 0, sizeof(*out_style));

    void *libdom_node = silk_dom_node_get_libdom_node(element);
    if (!libdom_node) return -1;

    dom_node_type ntype;
    dom_exception derr = dom_node_get_node_type((dom_node *)libdom_node, &ntype);
    if (derr != DOM_NO_ERR || ntype != DOM_ELEMENT_NODE) return -1;

    dom_element *dom_elem = (dom_element *)libdom_node;

    /* Get parent element */
    dom_node *parent_node = NULL;
    dom_node_get_parent_node((dom_node *)libdom_node, &parent_node);
    dom_element *parent_elem = NULL;
    if (parent_node) {
        dom_node_type pt;
        dom_node_get_node_type(parent_node, &pt);
        if (pt == DOM_ELEMENT_NODE) parent_elem = (dom_element *)parent_node;
        dom_node_unref(parent_node);
    }

    /* Get inline style attribute */
    const char *inline_style = silk_dom_node_get_attribute(element, "style");

    /* Use native pipeline via bridge */
    return silk_native_compute_style(
        engine->arena,
        dom_elem, parent_elem,
        engine->native_sheets, engine->native_sheet_count,
        inline_style, out_style
    );
}

int silk_css_apply_document_styles(silk_css_engine_t *engine, silk_document_t *doc) {
    if (!engine || !doc) return -1;
    return 0;
}
