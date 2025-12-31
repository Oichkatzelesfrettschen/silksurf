/* CSS Engine implementation - lightweight, modern, cleanroom design */
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <libcss/libcss.h>
#include "silksurf/css_parser.h"
#include "silksurf/dom_node.h"
#include "silksurf/document.h"
#include "silksurf/allocator.h"

/* Forward declaration - defined in css_select_handler.c */
extern css_select_handler *silk_css_get_select_handler(void);

/* CSS Engine structure */
struct silk_css_engine {
    silk_arena_t *arena;           /* Arena for allocations */
    css_stylesheet **sheets;       /* Array of libcss stylesheets */
    int sheet_count;
    int sheet_capacity;

    /* Cached selector context */
    css_select_ctx *select_ctx;

    /* Computed style cache (per element) */
    /* TODO: Add hash table for style caching */
};

/* ========== LIBCSS CALLBACKS ========== */

/* Resolve URL callback - required by libcss */
static css_error resolve_url(void *pw, const char *base,
                              lwc_string *rel, lwc_string **abs) {
    /* For now, we don't support external resources */
    /* TODO: Implement URL resolution for @import and url() */
    (void)pw; (void)base; (void)rel;
    *abs = NULL;
    return CSS_OK;
}

/* Import callback - called when CSS contains @import */
static css_error import_style(void *pw, css_stylesheet *parent,
                               lwc_string *url) {
    /* For now, we don't support @import */
    /* TODO: Implement stylesheet importing */
    (void)pw; (void)parent; (void)url;
    return CSS_OK;
}

/* Font callback - called when CSS references fonts */
static css_error font_callback(void *pw, lwc_string *name,
                                css_system_font *system_font) {
    /* For now, use default font */
    /* TODO: Implement font resolution */
    (void)pw; (void)name; (void)system_font;
    return CSS_OK;
}

/* Stylesheet parameters for libcss */
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

/* ========== CSS ENGINE API ========== */

/* Minimal user agent stylesheet */
static const char *ua_stylesheet =
    "html, body { display: block; margin: 0; padding: 0; }"
    "div, p, h1, h2, h3, h4, h5, h6 { display: block; }"
    "span, a { display: inline; }";

/* Create CSS engine */
silk_css_engine_t *silk_css_engine_create(silk_arena_t *arena) {
    if (!arena) {
        fprintf(stderr, "[CSS] ERROR: Invalid arena\n");
        return NULL;
    }

    silk_css_engine_t *engine = silk_arena_alloc(arena, sizeof(silk_css_engine_t));
    if (!engine) {
        fprintf(stderr, "[CSS] ERROR: Failed to allocate engine\n");
        return NULL;
    }

    memset(engine, 0, sizeof(*engine));
    engine->arena = arena;
    engine->sheet_capacity = 8;

    /* Allocate stylesheet array */
    engine->sheets = silk_arena_alloc(arena,
                                      sizeof(css_stylesheet *) * engine->sheet_capacity);
    if (!engine->sheets) {
        fprintf(stderr, "[CSS] ERROR: Failed to allocate sheet array\n");
        return NULL;
    }

    /* Create selection context */
    css_error err = css_select_ctx_create(&engine->select_ctx);
    if (err != CSS_OK) {
        fprintf(stderr, "[CSS] ERROR: Failed to create select context: %d\n", err);
        return NULL;
    }

    /* Add minimal UA stylesheet */
    css_stylesheet *ua_sheet = NULL;
    err = css_stylesheet_create(&sheet_params, &ua_sheet);
    if (err == CSS_OK) {
        err = css_stylesheet_append_data(ua_sheet, (const uint8_t *)ua_stylesheet, strlen(ua_stylesheet));
        if (err == CSS_OK) {
            err = css_stylesheet_data_done(ua_sheet);
            if (err == CSS_OK) {
                err = css_select_ctx_append_sheet(engine->select_ctx, ua_sheet,
                                                   CSS_ORIGIN_UA, NULL);
                if (err == CSS_OK) {
                    engine->sheets[engine->sheet_count++] = ua_sheet;
                    fprintf(stderr, "[CSS] Added UA stylesheet\n");
                } else {
                    css_stylesheet_destroy(ua_sheet);
                }
            } else {
                css_stylesheet_destroy(ua_sheet);
            }
        } else {
            css_stylesheet_destroy(ua_sheet);
        }
    }

    fprintf(stderr, "[CSS] Engine created: %p\n", (void *)engine);
    return engine;
}

/* Destroy CSS engine */
void silk_css_engine_destroy(silk_css_engine_t *engine) {
    if (!engine) {
        return;
    }

    fprintf(stderr, "[CSS] Destroying engine: %p\n", (void *)engine);

    /* Destroy all stylesheets */
    for (int i = 0; i < engine->sheet_count; i++) {
        if (engine->sheets[i]) {
            css_stylesheet_destroy(engine->sheets[i]);
        }
    }

    /* Destroy selection context */
    if (engine->select_ctx) {
        css_select_ctx_destroy(engine->select_ctx);
    }

    /* Engine structure itself is freed by arena */
    fprintf(stderr, "[CSS] Engine destroyed\n");
}

/* Parse CSS from string */
int silk_css_parse_string(silk_css_engine_t *engine, const char *css, size_t css_len) {
    if (!engine || !css || css_len == 0) {
        return -1;
    }

    fprintf(stderr, "[CSS] Parsing CSS string (len=%zu)\n", css_len);

    /* Create new stylesheet */
    css_stylesheet *sheet = NULL;
    css_error err = css_stylesheet_create(&sheet_params, &sheet);
    if (err != CSS_OK) {
        fprintf(stderr, "[CSS] ERROR: Failed to create stylesheet: %d\n", err);
        return -1;
    }

    /* Parse CSS data */
    err = css_stylesheet_append_data(sheet, (const uint8_t *)css, css_len);
    if (err != CSS_OK && err != CSS_NEEDDATA) {
        fprintf(stderr, "[CSS] ERROR: Failed to append CSS data: %d\n", err);
        css_stylesheet_destroy(sheet);
        return -1;
    }

    /* Finalize parsing */
    err = css_stylesheet_data_done(sheet);
    if (err != CSS_OK) {
        fprintf(stderr, "[CSS] ERROR: Failed to finalize stylesheet: %d\n", err);
        css_stylesheet_destroy(sheet);
        return -1;
    }

    /* Expand sheet capacity if needed */
    if (engine->sheet_count >= engine->sheet_capacity) {
        engine->sheet_capacity *= 2;
        css_stylesheet **new_sheets = silk_arena_alloc(engine->arena,
                                                        sizeof(css_stylesheet *) * engine->sheet_capacity);
        if (!new_sheets) {
            fprintf(stderr, "[CSS] ERROR: Failed to expand sheet array\n");
            css_stylesheet_destroy(sheet);
            return -1;
        }
        memcpy(new_sheets, engine->sheets, sizeof(css_stylesheet *) * engine->sheet_count);
        engine->sheets = new_sheets;
    }

    /* Add stylesheet to engine */
    engine->sheets[engine->sheet_count++] = sheet;

    /* Append to selection context */
    err = css_select_ctx_append_sheet(engine->select_ctx, sheet,
                                       CSS_ORIGIN_AUTHOR, NULL);
    if (err != CSS_OK) {
        fprintf(stderr, "[CSS] ERROR: Failed to append sheet to context: %d\n", err);
        return -1;
    }

    fprintf(stderr, "[CSS] Successfully parsed stylesheet (total: %d)\n", engine->sheet_count);
    return 0;
}

/* Parse CSS from DOM style element */
int silk_css_parse_style_element(silk_css_engine_t *engine, silk_dom_node_t *style_elem) {
    if (!engine || !style_elem) {
        return -1;
    }

    /* Get text content from style element */
    silk_dom_node_t *text_node = silk_dom_node_get_first_child(style_elem);
    if (!text_node) {
        fprintf(stderr, "[CSS] WARNING: Style element has no text content\n");
        return 0;  /* Not an error, just empty */
    }

    const char *css_text = silk_dom_node_get_text_content(text_node);
    if (!css_text || css_text[0] == '\0') {
        fprintf(stderr, "[CSS] WARNING: Style element has empty text\n");
        return 0;
    }

    fprintf(stderr, "[CSS] Parsing style element: %zu bytes\n", strlen(css_text));
    return silk_css_parse_string(engine, css_text, strlen(css_text));
}

/* Get computed styles for an element */
int silk_css_get_computed_style(silk_css_engine_t *engine,
                                 silk_dom_node_t *element,
                                 silk_computed_style_t *out_style) {
    if (!engine || !element || !out_style) {
        return -1;
    }

    /* Initialize output style with defaults */
    memset(out_style, 0, sizeof(*out_style));

    /* If no stylesheets loaded, return defaults */
    if (engine->sheet_count == 0) {
        fprintf(stderr, "[CSS] No stylesheets loaded, using defaults\n");
        out_style->display = CSS_DISPLAY_BLOCK;
        out_style->width = -1;   /* auto */
        out_style->height = -1;  /* auto */
        out_style->color = 0xFF000000;  /* black */
        out_style->font_size = 16;
        return 0;
    }

    fprintf(stderr, "[CSS] Computing style for element: %p (sheet_count=%d)\n",
            (void *)element, engine->sheet_count);

    if (!engine->select_ctx) {
        fprintf(stderr, "[CSS] ERROR: select_ctx is NULL!\n");
        return -1;
    }

    /* Set up unit context for DPI calculation */
    css_unit_ctx unit_ctx;
    unit_ctx.device_dpi = 96.0;  /* Standard screen DPI */
    unit_ctx.viewport_width = 1024;
    unit_ctx.viewport_height = 768;
    unit_ctx.root_style = NULL;

    /* Set up media query context (required even if not using media queries) */
    css_media media;
    memset(&media, 0, sizeof(media));
    media.type = CSS_MEDIA_SCREEN;
    media.width = INTTOFIX(1024);   /* Viewport width in CSS pixels */
    media.height = INTTOFIX(768);   /* Viewport height in CSS pixels */

    /* Get underlying libdom node for libcss (it expects raw libdom nodes) */
    void *libdom_node = silk_dom_node_get_libdom_node(element);
    if (!libdom_node) {
        fprintf(stderr, "[CSS] ERROR: Could not unwrap libdom node\n");
        return -1;
    }

    fprintf(stderr, "[CSS] Calling css_select_style with libdom node: %p\n", libdom_node);

    /* Use libcss to compute styles for this element */
    css_select_results *results = NULL;
    css_error err = css_select_style(engine->select_ctx,
                                      libdom_node,  /* raw libdom node */
                                      &unit_ctx,
                                      &media,  /* media context */
                                      NULL,  /* inline_style */
                                      silk_css_get_select_handler(),
                                      NULL,  /* handler private data */
                                      &results);

    fprintf(stderr, "[CSS] css_select_style returned: %d\n", err);

    if (err != CSS_OK || !results) {
        fprintf(stderr, "[CSS] ERROR: Style selection failed: %d (results=%p)\n", err, (void *)results);
        return -1;
    }

    /* Extract computed styles from results */
    css_computed_style *computed = results->styles[CSS_PSEUDO_ELEMENT_NONE];
    if (!computed) {
        fprintf(stderr, "[CSS] ERROR: No computed style\n");
        css_select_results_destroy(results);
        return -1;
    }

    /* Extract display property */
    uint8_t display_type = css_computed_display(computed, NULL);
    out_style->display = display_type;

    /* Extract width and height */
    css_fixed width_val = 0;
    css_unit width_unit = CSS_UNIT_PX;
    uint8_t width_type = css_computed_width(computed, &width_val, &width_unit);

    if (width_type == CSS_WIDTH_SET && width_unit == CSS_UNIT_PX) {
        out_style->width = FIXTOINT(width_val);
    } else {
        out_style->width = -1;  /* auto */
    }

    css_fixed height_val = 0;
    css_unit height_unit = CSS_UNIT_PX;
    uint8_t height_type = css_computed_height(computed, &height_val, &height_unit);

    if (height_type == CSS_HEIGHT_SET && height_unit == CSS_UNIT_PX) {
        out_style->height = FIXTOINT(height_val);
    } else {
        out_style->height = -1;  /* auto */
    }

    /* Extract color */
    css_color color_val = 0;
    uint8_t color_type = css_computed_color(computed, &color_val);
    if (color_type == CSS_COLOR_COLOR) {
        out_style->color = color_val;
    } else {
        out_style->color = 0xFF000000;  /* default black */
    }

    /* Extract background color */
    css_color bg_color_val = 0;
    uint8_t bg_color_type = css_computed_background_color(computed, &bg_color_val);
    if (bg_color_type == CSS_BACKGROUND_COLOR_COLOR) {
        out_style->background_color = bg_color_val;
    } else {
        out_style->background_color = 0x00000000;  /* transparent */
    }

    /* Extract font size */
    css_fixed font_size_val = 0;
    css_unit font_size_unit = CSS_UNIT_PX;
    uint8_t font_size_type = css_computed_font_size(computed, &font_size_val, &font_size_unit);

    if (font_size_type == CSS_FONT_SIZE_DIMENSION && font_size_unit == CSS_UNIT_PX) {
        out_style->font_size = FIXTOINT(font_size_val);
    } else {
        out_style->font_size = 16;  /* default */
    }

    /* Extract margins */
    css_fixed margin_val = 0;
    css_unit margin_unit = CSS_UNIT_PX;

    if (css_computed_margin_top(computed, &margin_val, &margin_unit) == CSS_MARGIN_SET) {
        out_style->margin_top = FIXTOINT(margin_val);
    }
    if (css_computed_margin_right(computed, &margin_val, &margin_unit) == CSS_MARGIN_SET) {
        out_style->margin_right = FIXTOINT(margin_val);
    }
    if (css_computed_margin_bottom(computed, &margin_val, &margin_unit) == CSS_MARGIN_SET) {
        out_style->margin_bottom = FIXTOINT(margin_val);
    }
    if (css_computed_margin_left(computed, &margin_val, &margin_unit) == CSS_MARGIN_SET) {
        out_style->margin_left = FIXTOINT(margin_val);
    }

    /* Extract padding */
    css_fixed padding_val = 0;
    css_unit padding_unit = CSS_UNIT_PX;

    if (css_computed_padding_top(computed, &padding_val, &padding_unit) == CSS_PADDING_SET) {
        out_style->padding_top = FIXTOINT(padding_val);
    }
    if (css_computed_padding_right(computed, &padding_val, &padding_unit) == CSS_PADDING_SET) {
        out_style->padding_right = FIXTOINT(padding_val);
    }
    if (css_computed_padding_bottom(computed, &padding_val, &padding_unit) == CSS_PADDING_SET) {
        out_style->padding_bottom = FIXTOINT(padding_val);
    }
    if (css_computed_padding_left(computed, &padding_val, &padding_unit) == CSS_PADDING_SET) {
        out_style->padding_left = FIXTOINT(padding_val);
    }

    fprintf(stderr, "[CSS] Computed style: display=%u, width=%d, height=%d, color=%08x\n",
            out_style->display, out_style->width, out_style->height, out_style->color);

    /* Clean up */
    css_select_results_destroy(results);

    return 0;
}

/* Apply styles from document's <style> tags */
int silk_css_apply_document_styles(silk_css_engine_t *engine,
                                    silk_document_t *doc) {
    if (!engine || !doc) {
        return -1;
    }

    fprintf(stderr, "[CSS] Applying document styles\n");

    /* Get document root */
    silk_dom_node_t *root = (silk_dom_node_t *)silk_document_get_root_element(doc);
    if (!root) {
        fprintf(stderr, "[CSS] WARNING: No document root\n");
        return 0;
    }

    /* TODO: Traverse DOM to find all <style> elements */
    /* For now, this is a stub that will be implemented when we have:
       1. DOM tree traversal utilities
       2. Element type checking (to find STYLE elements)
       3. Style element content extraction
    */

    fprintf(stderr, "[CSS] TODO: Document style application not yet implemented\n");
    return 0;
}
