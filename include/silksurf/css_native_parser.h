#ifndef SILK_CSS_NATIVE_PARSER_H
#define SILK_CSS_NATIVE_PARSER_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include "silksurf/allocator.h"
#include "silksurf/css_tokenizer.h"

/* Maximum declarations per rule and rules per stylesheet */
#define CSS_MAX_DECLARATIONS 32
#define CSS_MAX_RULES 512

/* ============================================================================
 * Parsed CSS Value
 * ============================================================================ */

typedef enum {
    CSS_VAL_KEYWORD,       /* auto, inherit, none, block, inline, etc. */
    CSS_VAL_LENGTH,        /* 100px, 2em, 1.5rem */
    CSS_VAL_PERCENTAGE,    /* 50% */
    CSS_VAL_COLOR,         /* #ff0000, rgb(r,g,b), named colors */
    CSS_VAL_NUMBER,        /* 0, 1.5 */
    CSS_VAL_STRING,        /* "hello" */
    CSS_VAL_IMPORTANT,     /* !important flag (stored separately) */
} css_parsed_value_type_t;

typedef struct {
    css_parsed_value_type_t type;
    union {
        double number;
        struct { double value; const char *unit; size_t unit_len; } length;
        double percentage;
        uint32_t color;        /* ARGB */
        const char *keyword;
        const char *string;
    } data;
} css_parsed_value_t;

/* ============================================================================
 * Parsed CSS Declaration (property: value)
 * ============================================================================ */

typedef struct {
    const char *property;         /* Property name (arena-allocated) */
    size_t property_len;
    css_parsed_value_t value;
    bool important;               /* !important */
} css_parsed_declaration_t;

/* ============================================================================
 * Parsed CSS Rule (selector { declarations })
 * ============================================================================ */

typedef struct {
    const char *selector_text;    /* Raw selector string (arena-allocated) */
    size_t selector_len;
    css_parsed_declaration_t *declarations;  /* Arena-allocated array */
    uint32_t declaration_count;
    uint32_t declaration_capacity;
    uint32_t source_order;        /* Position in stylesheet for cascade */
} css_parsed_rule_t;

/* ============================================================================
 * Parsed CSS Stylesheet (collection of rules)
 * ============================================================================ */

typedef struct {
    silk_arena_t *arena;
    css_parsed_rule_t *rules;  /* Arena-allocated array */
    uint32_t rule_count;
    uint32_t rule_capacity;
} css_parsed_stylesheet_t;

/* ============================================================================
 * Parser API
 * ============================================================================ */

/* Parse a CSS string into a stylesheet structure */
css_parsed_stylesheet_t *css_parse_stylesheet(
    silk_arena_t *arena,
    const char *css,
    size_t css_len
);

/* Parse a single inline style attribute (e.g. style="color: red; width: 100px") */
uint32_t css_parse_inline_style(
    silk_arena_t *arena,
    const char *style,
    size_t style_len,
    css_parsed_declaration_t *out_decls,
    uint32_t max_decls
);

/* Parse a color value from string (hex, named, rgb()) */
bool css_parse_color(const char *str, size_t len, uint32_t *out_color);

#endif /* SILK_CSS_NATIVE_PARSER_H */
