#ifndef SILK_CSS_TOKENIZER_H
#define SILK_CSS_TOKENIZER_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include "silksurf/allocator.h"

typedef enum {
    CSS_TOK_IDENT,
    CSS_TOK_FUNCTION,
    CSS_TOK_AT_KEYWORD,
    CSS_TOK_HASH,
    CSS_TOK_STRING,
    CSS_TOK_BAD_STRING,
    CSS_TOK_URL,
    CSS_TOK_BAD_URL,
    CSS_TOK_DELIM,
    CSS_TOK_NUMBER,
    CSS_TOK_PERCENTAGE,
    CSS_TOK_DIMENSION,
    CSS_TOK_WHITESPACE,
    CSS_TOK_CDO,
    CSS_TOK_CDC,
    CSS_TOK_COLON,
    CSS_TOK_SEMICOLON,
    CSS_TOK_COMMA,
    CSS_TOK_LEFT_SQUARE,
    CSS_TOK_RIGHT_SQUARE,
    CSS_TOK_LEFT_PAREN,
    CSS_TOK_RIGHT_PAREN,
    CSS_TOK_LEFT_CURLY,
    CSS_TOK_RIGHT_CURLY,
    CSS_TOK_EOF,
    CSS_TOK_COMMENT,
} silk_css_token_type_t;

typedef struct {
    silk_css_token_type_t type;
    char *value;
    size_t value_len;
    /* For numbers/dimensions */
    double numeric_value;
    char *unit;
} silk_css_token_t;

typedef struct {
    silk_arena_t *arena;
    const char *input;
    size_t input_len;
    size_t pos;
} silk_css_tokenizer_t;

silk_css_tokenizer_t *silk_css_tokenizer_create(silk_arena_t *arena, const char *input, size_t input_len);
silk_css_token_t *silk_css_tokenizer_next_token(silk_css_tokenizer_t *tok);

#endif
