#include <string.h>
#include <ctype.h>
#include "silksurf/css_tokenizer.h"

silk_css_tokenizer_t *silk_css_tokenizer_create(silk_arena_t *arena, const char *input, size_t input_len) {
    silk_css_tokenizer_t *tok = silk_arena_alloc(arena, sizeof(silk_css_tokenizer_t));
    if (!tok) return NULL;
    tok->arena = arena;
    tok->input = input;
    tok->input_len = input_len;
    tok->pos = 0;
    return tok;
}

static silk_css_token_t *create_token(silk_arena_t *arena, silk_css_token_type_t type) {
    silk_css_token_t *token = silk_arena_alloc(arena, sizeof(silk_css_token_t));
    if (token) {
        memset(token, 0, sizeof(*token));
        token->type = type;
    }
    return token;
}

silk_css_token_t *silk_css_tokenizer_next_token(silk_css_tokenizer_t *tok) {
    if (tok->pos >= tok->input_len) {
        return create_token(tok->arena, CSS_TOK_EOF);
    }

    size_t start_pos = tok->pos;
    char c = tok->input[tok->pos];

    /* 1. Consume Whitespace */
    if (isspace(c)) {
        while (tok->pos < tok->input_len && isspace(tok->input[tok->pos])) {
            tok->pos++;
        }
        return create_token(tok->arena, CSS_TOK_WHITESPACE);
    }

    /* 2. Simple Delimiters */
    switch (c) {
        case '{': tok->pos++; return create_token(tok->arena, CSS_TOK_LEFT_CURLY);
        case '}': tok->pos++; return create_token(tok->arena, CSS_TOK_RIGHT_CURLY);
        case ':': tok->pos++; return create_token(tok->arena, CSS_TOK_COLON);
        case ';': tok->pos++; return create_token(tok->arena, CSS_TOK_SEMICOLON);
        case ',': tok->pos++; return create_token(tok->arena, CSS_TOK_COMMA);
        case '(': tok->pos++; return create_token(tok->arena, CSS_TOK_LEFT_PAREN);
        case ')': tok->pos++; return create_token(tok->arena, CSS_TOK_RIGHT_PAREN);
        case '[': tok->pos++; return create_token(tok->arena, CSS_TOK_LEFT_SQUARE);
        case ']': tok->pos++; return create_token(tok->arena, CSS_TOK_RIGHT_SQUARE);
    }

    /* 3. At-keyword or Delim */
    if (c == '@') {
        tok->pos++;
        if (tok->pos < tok->input_len && isalpha(tok->input[tok->pos])) {
            while (tok->pos < tok->input_len && (isalnum(tok->input[tok->pos]) || tok->input[tok->pos] == '-')) {
                tok->pos++;
            }
            return create_token(tok->arena, CSS_TOK_AT_KEYWORD);
        }
        return create_token(tok->arena, CSS_TOK_DELIM);
    }

    /* 4. Idents and Tags (Simplified) */
    if (isalpha(c) || c == '-' || c == '_') {
        while (tok->pos < tok->input_len && (isalnum(tok->input[tok->pos]) || tok->input[tok->pos] == '-' || tok->input[tok->pos] == '_')) {
            tok->pos++;
        }
        return create_token(tok->arena, CSS_TOK_IDENT);
    }

    /* 5. FORCE CONSUMPTION: If no other rule matched, consume one byte to prevent livelock */
    if (tok->pos == start_pos) {
        tok->pos++;
        return create_token(tok->arena, CSS_TOK_DELIM);
    }

    return create_token(tok->arena, CSS_TOK_EOF);
}
