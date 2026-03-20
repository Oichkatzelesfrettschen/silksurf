/* Native CSS Parser - Parses CSS text into structured rules
 *
 * Converts raw CSS text into css_parsed_stylesheet_t containing:
 * - Selector strings (matched later by selector matching engine)
 * - Declarations (property: value pairs)
 * - Source ordering for cascade
 *
 * Error recovery: Skip malformed rules per CSS spec.
 */

#include <string.h>
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include "silksurf/css_native_parser.h"
#include "silksurf/css_tokenizer.h"

/* ============================================================================
 * Named Color Table
 * ============================================================================ */

typedef struct { const char *name; uint32_t color; } named_color_t;

static const named_color_t named_colors[] = {
    {"black",       0xFF000000}, {"white",       0xFFFFFFFF},
    {"red",         0xFFFF0000}, {"green",       0xFF008000},
    {"blue",        0xFF0000FF}, {"yellow",      0xFFFFFF00},
    {"cyan",        0xFF00FFFF}, {"magenta",     0xFFFF00FF},
    {"gray",        0xFF808080}, {"grey",        0xFF808080},
    {"silver",      0xFFC0C0C0}, {"maroon",      0xFF800000},
    {"olive",       0xFF808000}, {"lime",        0xFF00FF00},
    {"aqua",        0xFF00FFFF}, {"teal",        0xFF008080},
    {"navy",        0xFF000080}, {"fuchsia",     0xFFFF00FF},
    {"purple",      0xFF800080}, {"orange",      0xFFFFA500},
    {"transparent", 0x00000000},
};

#define NUM_NAMED_COLORS (sizeof(named_colors) / sizeof(named_colors[0]))

bool css_parse_color(const char *str, size_t len, uint32_t *out_color) {
    if (!str || len == 0 || !out_color) return false;

    /* Hex color: 3-digit (#RGB) or 6-digit (#RRGGBB) */
    if (str[0] == '#' || (len >= 3 && isxdigit((unsigned char)str[0]))) {
        const char *hex = str;
        size_t hex_len = len;
        if (str[0] == '#') { hex++; hex_len--; }  /* Not used for hash tokens */

        /* For hash tokens, str already excludes the # */
        if (hex_len == 6) {
            unsigned int r, g, b;
            if (sscanf(hex, "%2x%2x%2x", &r, &g, &b) == 3) {
                *out_color = 0xFF000000 | (r << 16) | (g << 8) | b;
                return true;
            }
        } else if (hex_len == 3) {
            unsigned int r, g, b;
            if (sscanf(hex, "%1x%1x%1x", &r, &g, &b) == 3) {
                *out_color = 0xFF000000 | (r*17 << 16) | (g*17 << 8) | (b*17);
                return true;
            }
        }
    }

    /* Named colors */
    for (size_t i = 0; i < NUM_NAMED_COLORS; i++) {
        if (strncasecmp(str, named_colors[i].name, len) == 0 &&
            strlen(named_colors[i].name) == len) {
            *out_color = named_colors[i].color;
            return true;
        }
    }

    return false;
}

/* ============================================================================
 * Helper: Copy arena string
 * ============================================================================ */

static const char *arena_copy(silk_arena_t *arena, const char *src, size_t len) {
    char *dst = silk_arena_alloc(arena, len + 1);
    if (!dst) return NULL;
    memcpy(dst, src, len);
    dst[len] = '\0';
    return dst;
}

/* ============================================================================
 * Declaration Parser
 * ============================================================================
 * Parses: property : value [!important] ;
 */

static void skip_whitespace(silk_css_tokenizer_t *tok) {
    silk_css_token_t *t = silk_css_tokenizer_peek(tok);
    while (t && t->type == CSS_TOK_WHITESPACE) {
        silk_css_tokenizer_next_token(tok);
        t = silk_css_tokenizer_peek(tok);
    }
}

/* Parse a single property value from tokens */
static bool parse_value(silk_css_tokenizer_t *tok, css_parsed_value_t *out, silk_arena_t *arena) {
    skip_whitespace(tok);
    silk_css_token_t *t = silk_css_tokenizer_next_token(tok);
    if (!t) return false;

    switch (t->type) {
        case CSS_TOK_DIMENSION:
            out->type = CSS_VAL_LENGTH;
            out->data.length.value = t->numeric_value;
            out->data.length.unit = t->unit;
            out->data.length.unit_len = t->unit_len;
            return true;

        case CSS_TOK_PERCENTAGE:
            out->type = CSS_VAL_PERCENTAGE;
            out->data.percentage = t->numeric_value;
            return true;

        case CSS_TOK_NUMBER:
            out->type = CSS_VAL_NUMBER;
            out->data.number = t->numeric_value;
            return true;

        case CSS_TOK_HASH:
            out->type = CSS_VAL_COLOR;
            if (t->value && css_parse_color(t->value, t->value_len, &out->data.color)) {
                return true;
            }
            return false;

        case CSS_TOK_IDENT:
            /* Check if it's a named color */
            if (t->value) {
                uint32_t color;
                if (css_parse_color(t->value, t->value_len, &color)) {
                    out->type = CSS_VAL_COLOR;
                    out->data.color = color;
                    return true;
                }
            }
            out->type = CSS_VAL_KEYWORD;
            out->data.keyword = t->value ? arena_copy(arena, t->value, t->value_len) : NULL;
            return true;

        case CSS_TOK_FUNCTION:
            /* Handle rgb(r, g, b) */
            if (t->value && t->value_len == 3 && strncasecmp(t->value, "rgb", 3) == 0) {
                skip_whitespace(tok);
                silk_css_token_t *r_tok = silk_css_tokenizer_next_token(tok);
                skip_whitespace(tok);
                silk_css_tokenizer_next_token(tok); /* comma */
                skip_whitespace(tok);
                silk_css_token_t *g_tok = silk_css_tokenizer_next_token(tok);
                skip_whitespace(tok);
                silk_css_tokenizer_next_token(tok); /* comma */
                skip_whitespace(tok);
                silk_css_token_t *b_tok = silk_css_tokenizer_next_token(tok);
                skip_whitespace(tok);
                silk_css_tokenizer_next_token(tok); /* close paren */

                if (r_tok && g_tok && b_tok &&
                    r_tok->type == CSS_TOK_NUMBER &&
                    g_tok->type == CSS_TOK_NUMBER &&
                    b_tok->type == CSS_TOK_NUMBER) {
                    int r = (int)r_tok->numeric_value;
                    int g = (int)g_tok->numeric_value;
                    int b = (int)b_tok->numeric_value;
                    if (r > 255) r = 255;
                    if (r < 0) r = 0;
                    if (g > 255) g = 255;
                    if (g < 0) g = 0;
                    if (b > 255) b = 255;
                    if (b < 0) b = 0;
                    out->type = CSS_VAL_COLOR;
                    out->data.color = 0xFF000000 | ((uint32_t)r << 16) | ((uint32_t)g << 8) | (uint32_t)b;
                    return true;
                }
            }
            out->type = CSS_VAL_KEYWORD;
            out->data.keyword = t->value ? arena_copy(arena, t->value, t->value_len) : NULL;
            return true;

        case CSS_TOK_STRING:
            out->type = CSS_VAL_STRING;
            out->data.string = t->value ? arena_copy(arena, t->value, t->value_len) : NULL;
            return true;

        default:
            return false;
    }
}

/* Parse a single declaration: property : value [!important] ; */
static bool parse_declaration(silk_css_tokenizer_t *tok, css_parsed_declaration_t *out,
                               silk_arena_t *arena) {
    skip_whitespace(tok);

    /* Property name (IDENT) */
    silk_css_token_t *prop = silk_css_tokenizer_next_token(tok);
    if (!prop || prop->type != CSS_TOK_IDENT) return false;

    out->property = arena_copy(arena, prop->value, prop->value_len);
    out->property_len = prop->value_len;

    /* Colon */
    skip_whitespace(tok);
    silk_css_token_t *colon = silk_css_tokenizer_next_token(tok);
    if (!colon || colon->type != CSS_TOK_COLON) return false;

    /* Value */
    if (!parse_value(tok, &out->value, arena)) return false;

    /* Check for !important */
    skip_whitespace(tok);
    silk_css_token_t *peek = silk_css_tokenizer_peek(tok);
    if (peek && peek->type == CSS_TOK_DELIM && peek->delim == '!') {
        silk_css_tokenizer_next_token(tok); /* consume ! */
        skip_whitespace(tok);
        silk_css_token_t *imp = silk_css_tokenizer_next_token(tok);
        if (imp && imp->type == CSS_TOK_IDENT && imp->value_len == 9 &&
            strncasecmp(imp->value, "important", 9) == 0) {
            out->important = true;
        }
    }

    /* Consume trailing semicolon or peek at } */
    skip_whitespace(tok);
    peek = silk_css_tokenizer_peek(tok);
    if (peek && peek->type == CSS_TOK_SEMICOLON) {
        silk_css_tokenizer_next_token(tok);
    }

    return true;
}

/* ============================================================================
 * Rule Parser
 * ============================================================================
 * Parses: selector { declaration-list }
 */

/* Collect selector text until { */
static const char *parse_selector_text(silk_css_tokenizer_t *tok, size_t *out_len,
                                        silk_arena_t *arena) {
    /* Collect all tokens until { into a string */
    char buf[1024];
    size_t buf_pos = 0;

    while (true) {
        silk_css_token_t *peek = silk_css_tokenizer_peek(tok);
        if (!peek || peek->type == CSS_TOK_LEFT_CURLY || peek->type == CSS_TOK_EOF) {
            break;
        }

        silk_css_token_t *t = silk_css_tokenizer_next_token(tok);
        if (t->type == CSS_TOK_WHITESPACE) {
            if (buf_pos > 0 && buf[buf_pos - 1] != ' ') {
                if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = ' ';
            }
        } else if (t->type == CSS_TOK_HASH && t->value) {
            /* Hash token: prepend # to reconstruct #id */
            if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = '#';
            size_t avail = (buf_pos < sizeof(buf) - 1) ? sizeof(buf) - 1 - buf_pos : 0;
            size_t clen = (t->value_len < avail) ? t->value_len : avail;
            if (clen > 0) { memcpy(buf + buf_pos, t->value, clen); buf_pos += clen; }
        } else if (t->type == CSS_TOK_DELIM) {
            if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = t->delim;
        } else if (t->type == CSS_TOK_COLON) {
            if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = ':';
        } else if (t->type == CSS_TOK_LEFT_SQUARE) {
            if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = '[';
        } else if (t->type == CSS_TOK_RIGHT_SQUARE) {
            if (buf_pos < sizeof(buf) - 1) buf[buf_pos++] = ']';
        } else if (t->value && t->value_len > 0) {
            /* Generic: copy token value text */
            size_t avail = (buf_pos < sizeof(buf) - 1) ? sizeof(buf) - 1 - buf_pos : 0;
            size_t clen = (t->value_len < avail) ? t->value_len : avail;
            if (clen > 0) { memcpy(buf + buf_pos, t->value, clen); buf_pos += clen; }
        }
    }

    /* Trim trailing whitespace */
    while (buf_pos > 0 && buf[buf_pos - 1] == ' ') buf_pos--;

    *out_len = buf_pos;
    return arena_copy(arena, buf, buf_pos);
}

/* Parse one CSS rule */
static bool parse_rule(silk_css_tokenizer_t *tok, css_parsed_rule_t *rule,
                        silk_arena_t *arena) {
    skip_whitespace(tok);

    /* Check for @ rules -- skip them for now */
    silk_css_token_t *peek = silk_css_tokenizer_peek(tok);
    if (peek && peek->type == CSS_TOK_AT_KEYWORD) {
        /* Skip @rule until next ; or {} block */
        while (true) {
            silk_css_token_t *t = silk_css_tokenizer_next_token(tok);
            if (!t || t->type == CSS_TOK_EOF) return false;
            if (t->type == CSS_TOK_SEMICOLON) return false;
            if (t->type == CSS_TOK_LEFT_CURLY) {
                int depth = 1;
                while (depth > 0) {
                    t = silk_css_tokenizer_next_token(tok);
                    if (!t || t->type == CSS_TOK_EOF) return false;
                    if (t->type == CSS_TOK_LEFT_CURLY) depth++;
                    if (t->type == CSS_TOK_RIGHT_CURLY) depth--;
                }
                return false;
            }
        }
    }

    /* Parse selector */
    size_t sel_len;
    rule->selector_text = parse_selector_text(tok, &sel_len, arena);
    rule->selector_len = sel_len;

    if (!rule->selector_text || sel_len == 0) return false;

    /* Expect { */
    silk_css_token_t *lbrace = silk_css_tokenizer_next_token(tok);
    if (!lbrace || lbrace->type != CSS_TOK_LEFT_CURLY) return false;

    /* Allocate declaration array */
    rule->declaration_capacity = 8;
    rule->declarations = silk_arena_alloc(arena,
        sizeof(css_parsed_declaration_t) * rule->declaration_capacity);
    if (!rule->declarations) return false;
    rule->declaration_count = 0;

    /* Parse declarations until } */
    while (true) {
        skip_whitespace(tok);
        peek = silk_css_tokenizer_peek(tok);
        if (!peek || peek->type == CSS_TOK_RIGHT_CURLY || peek->type == CSS_TOK_EOF) {
            break;
        }

        if (rule->declaration_count >= rule->declaration_capacity) {
            /* Grow array */
            uint32_t new_cap = rule->declaration_capacity * 2;
            css_parsed_declaration_t *new_decls = silk_arena_alloc(arena,
                sizeof(css_parsed_declaration_t) * new_cap);
            if (!new_decls) break;
            memcpy(new_decls, rule->declarations,
                sizeof(css_parsed_declaration_t) * rule->declaration_count);
            rule->declarations = new_decls;
            rule->declaration_capacity = new_cap;
        }

        css_parsed_declaration_t *decl = &rule->declarations[rule->declaration_count];
        memset(decl, 0, sizeof(*decl));

        if (parse_declaration(tok, decl, arena)) {
            rule->declaration_count++;
        } else {
            /* Error recovery: skip to next ; or } */
            while (true) {
                peek = silk_css_tokenizer_peek(tok);
                if (!peek || peek->type == CSS_TOK_RIGHT_CURLY || peek->type == CSS_TOK_EOF) break;
                if (peek->type == CSS_TOK_SEMICOLON) {
                    silk_css_tokenizer_next_token(tok);
                    break;
                }
                silk_css_tokenizer_next_token(tok);
            }
        }
    }

    /* Consume } */
    peek = silk_css_tokenizer_peek(tok);
    if (peek && peek->type == CSS_TOK_RIGHT_CURLY) {
        silk_css_tokenizer_next_token(tok);
    }

    return rule->declaration_count > 0;
}

/* ============================================================================
 * Stylesheet Parser
 * ============================================================================ */

css_parsed_stylesheet_t *css_parse_stylesheet(
    silk_arena_t *arena,
    const char *css,
    size_t css_len
) {
    if (!arena || !css || css_len == 0) return NULL;

    css_parsed_stylesheet_t *sheet = silk_arena_alloc(arena, sizeof(css_parsed_stylesheet_t));
    if (!sheet) return NULL;

    memset(sheet, 0, sizeof(*sheet));
    sheet->arena = arena;
    sheet->rule_capacity = 16;
    sheet->rules = silk_arena_alloc(arena, sizeof(css_parsed_rule_t) * sheet->rule_capacity);
    if (!sheet->rules) return NULL;

    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, css, css_len);
    if (!tok) return NULL;

    uint32_t source_order = 0;

    while (sheet->rule_count < CSS_MAX_RULES) {
        skip_whitespace(tok);
        silk_css_token_t *peek = silk_css_tokenizer_peek(tok);
        if (!peek || peek->type == CSS_TOK_EOF) break;

        /* Grow rules array if needed */
        if (sheet->rule_count >= sheet->rule_capacity) {
            uint32_t new_cap = sheet->rule_capacity * 2;
            if (new_cap > CSS_MAX_RULES) new_cap = CSS_MAX_RULES;
            css_parsed_rule_t *new_rules = silk_arena_alloc(arena,
                sizeof(css_parsed_rule_t) * new_cap);
            if (!new_rules) break;
            memcpy(new_rules, sheet->rules,
                sizeof(css_parsed_rule_t) * sheet->rule_count);
            sheet->rules = new_rules;
            sheet->rule_capacity = new_cap;
        }

        css_parsed_rule_t *rule = &sheet->rules[sheet->rule_count];
        memset(rule, 0, sizeof(*rule));

        if (parse_rule(tok, rule, arena)) {
            rule->source_order = source_order++;
            sheet->rule_count++;
        }
    }

    return sheet;
}

/* ============================================================================
 * Inline Style Parser
 * ============================================================================ */

uint32_t css_parse_inline_style(
    silk_arena_t *arena,
    const char *style,
    size_t style_len,
    css_parsed_declaration_t *out_decls,
    uint32_t max_decls
) {
    if (!arena || !style || style_len == 0 || !out_decls || max_decls == 0) return 0;

    silk_css_tokenizer_t *tok = silk_css_tokenizer_create(arena, style, style_len);
    if (!tok) return 0;

    uint32_t count = 0;
    while (count < max_decls) {
        skip_whitespace(tok);
        silk_css_token_t *peek = silk_css_tokenizer_peek(tok);
        if (!peek || peek->type == CSS_TOK_EOF) break;

        css_parsed_declaration_t *decl = &out_decls[count];
        memset(decl, 0, sizeof(*decl));

        if (parse_declaration(tok, decl, arena)) {
            count++;
        } else {
            /* Skip to next ; */
            while (true) {
                peek = silk_css_tokenizer_peek(tok);
                if (!peek || peek->type == CSS_TOK_EOF) goto done;
                if (peek->type == CSS_TOK_SEMICOLON) {
                    silk_css_tokenizer_next_token(tok);
                    break;
                }
                silk_css_tokenizer_next_token(tok);
            }
        }
    }
done:
    return count;
}
