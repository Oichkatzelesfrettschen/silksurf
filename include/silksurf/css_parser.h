#ifndef SILKSURF_CSS_PARSER_H
#define SILKSURF_CSS_PARSER_H

#include <stdint.h>
#include <stddef.h>
#include "silksurf/allocator.h"

/* Forward declarations */
struct silk_document;
struct silk_dom_node;

/* CSS Property value types */
typedef enum {
    CSS_VALUE_LENGTH,      /* Pixels, ems, etc. */
    CSS_VALUE_COLOR,       /* RGB color */
    CSS_VALUE_KEYWORD,     /* auto, inherit, none, etc. */
    CSS_VALUE_PERCENTAGE,  /* % values */
    CSS_VALUE_STRING,      /* String values */
} css_value_type_t;

/* CSS Property value */
typedef struct {
    css_value_type_t type;
    union {
        int length;        /* Pixel length */
        uint32_t color;    /* ARGB color */
        int keyword;       /* Keyword ID */
        int percentage;    /* % as integer (100 = 100%) */
        char *string;      /* String value */
    } value;
} css_property_value_t;

/* CSS Property definition */
typedef struct {
    const char *name;          /* Property name (e.g., "color", "width") */
    css_property_value_t value; /* Property value */
    uint8_t important;         /* !important flag */
} css_property_t;

/* CSS Rule representation */
typedef struct {
    char *selector;      /* CSS selector string */
    uint32_t specificity; /* Specificity score */
    css_property_t *properties;
    int property_count;
} css_rule_t;

/* CSS Style Sheet */
typedef struct {
    css_rule_t *rules;
    int rule_count;
    int rule_capacity;
    silk_arena_t *arena;
} silk_css_sheet_t;

/* Computed styles for an element */
typedef struct {
    /* Layout properties */
    int width, height;
    int margin_top, margin_right, margin_bottom, margin_left;
    int padding_top, padding_right, padding_bottom, padding_left;
    int border_top, border_right, border_bottom, border_left;

    /* Display and positioning */
    uint32_t display;      /* CSS display value */
    uint32_t position;     /* CSS position value */
    int z_index;

    /* Visual properties */
    uint32_t color;        /* Text color (ARGB) */
    uint32_t background_color; /* Background color (ARGB) */
    uint32_t border_color;

    /* Font properties */
    int font_size;
    const char *font_family;
    uint32_t font_weight;
    uint8_t font_style;

    /* Text properties */
    uint32_t text_align;
    int line_height;

    /* Other properties */
    uint32_t overflow;
    uint32_t visibility;

    /* Computed internally */
    uint8_t display_computed;
} silk_computed_style_t;

/* CSS engine - manages stylesheets and style resolution */
typedef struct silk_css_engine silk_css_engine_t;

/* Initialize CSS engine */
silk_css_engine_t *silk_css_engine_create(silk_arena_t *arena);

/* Destroy CSS engine */
void silk_css_engine_destroy(silk_css_engine_t *engine);

/* Parse CSS from string and add to engine */
int silk_css_parse_string(silk_css_engine_t *engine, const char *css, size_t css_len);

/* Add a stylesheet from a DOM style element */
int silk_css_parse_style_element(silk_css_engine_t *engine, struct silk_dom_node *style_elem);

/* Get computed styles for an element */
int silk_css_get_computed_style(silk_css_engine_t *engine,
                                 struct silk_dom_node *element,
                                 silk_computed_style_t *out_style);

/* Apply styles from document's <style> tags */
int silk_css_apply_document_styles(silk_css_engine_t *engine,
                                    struct silk_document *doc);

#endif
