#include <string.h>
#include <ctype.h>
#include <stdlib.h>
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

/* Copy a substring into the arena */
static const char *arena_strdup(silk_arena_t *arena, const char *src, size_t len) {
    char *dst = silk_arena_alloc(arena, len + 1);
    if (!dst) return NULL;
    memcpy(dst, src, len);
    dst[len] = '\0';
    return dst;
}

static inline bool is_name_start(char c) {
    return isalpha((unsigned char)c) || c == '_' || c == '-';
}

static inline bool is_name_char(char c) {
    return isalnum((unsigned char)c) || c == '_' || c == '-';
}

static inline char peek(silk_css_tokenizer_t *tok) {
    return (tok->pos < tok->input_len) ? tok->input[tok->pos] : '\0';
}

static inline char peek_at(silk_css_tokenizer_t *tok, size_t offset) {
    size_t idx = tok->pos + offset;
    return (idx < tok->input_len) ? tok->input[idx] : '\0';
}

static inline void advance(silk_css_tokenizer_t *tok) {
    if (tok->pos < tok->input_len) tok->pos++;
}

/* Consume an ident-like sequence, return length */
static size_t consume_name(silk_css_tokenizer_t *tok) {
    size_t start = tok->pos;
    while (tok->pos < tok->input_len && is_name_char(tok->input[tok->pos])) {
        tok->pos++;
    }
    return tok->pos - start;
}

/* Consume digits, return length */
static size_t consume_digits(silk_css_tokenizer_t *tok) {
    size_t start = tok->pos;
    while (tok->pos < tok->input_len && isdigit((unsigned char)tok->input[tok->pos])) {
        tok->pos++;
    }
    return tok->pos - start;
}

/* Consume a numeric value: [+-]?[0-9]*\.?[0-9]+ */
static double consume_number(silk_css_tokenizer_t *tok) {
    size_t start = tok->pos;

    /* Optional sign */
    if (peek(tok) == '+' || peek(tok) == '-') advance(tok);

    /* Integer part */
    consume_digits(tok);

    /* Decimal part */
    if (peek(tok) == '.' && isdigit((unsigned char)peek_at(tok, 1))) {
        advance(tok); /* skip '.' */
        consume_digits(tok);
    }

    /* Parse the number from the consumed range */
    char buf[64];
    size_t len = tok->pos - start;
    if (len >= sizeof(buf)) len = sizeof(buf) - 1;
    memcpy(buf, tok->input + start, len);
    buf[len] = '\0';
    return strtod(buf, NULL);
}

/* Check if next chars start a number: [+-]?[0-9] or [+-]?.[0-9] */
static bool starts_number(silk_css_tokenizer_t *tok) {
    char c = peek(tok);
    if (isdigit((unsigned char)c)) return true;
    if (c == '.' && isdigit((unsigned char)peek_at(tok, 1))) return true;
    if ((c == '+' || c == '-') &&
        (isdigit((unsigned char)peek_at(tok, 1)) ||
         (peek_at(tok, 1) == '.' && isdigit((unsigned char)peek_at(tok, 2))))) return true;
    return false;
}

silk_css_token_t *silk_css_tokenizer_next_token(silk_css_tokenizer_t *tok) {
    if (tok->pos >= tok->input_len) {
        return create_token(tok->arena, CSS_TOK_EOF);
    }

    char c = tok->input[tok->pos];

    /* 1. Whitespace */
    if (isspace((unsigned char)c)) {
        while (tok->pos < tok->input_len && isspace((unsigned char)tok->input[tok->pos])) {
            tok->pos++;
        }
        return create_token(tok->arena, CSS_TOK_WHITESPACE);
    }

    /* 2. Comments */
    if (c == '/' && peek_at(tok, 1) == '*') {
        tok->pos += 2;
        while (tok->pos + 1 < tok->input_len) {
            if (tok->input[tok->pos] == '*' && tok->input[tok->pos + 1] == '/') {
                tok->pos += 2;
                break;
            }
            tok->pos++;
        }
        /* Skip comments, return next token */
        return silk_css_tokenizer_next_token(tok);
    }

    /* 3. Simple delimiters */
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
        default: break;
    }

    /* 4. String tokens ("..." or '...') */
    if (c == '"' || c == '\'') {
        char quote = c;
        tok->pos++; /* skip opening quote */
        size_t start = tok->pos;
        while (tok->pos < tok->input_len && tok->input[tok->pos] != quote) {
            if (tok->input[tok->pos] == '\\') tok->pos++; /* skip escape */
            tok->pos++;
        }
        size_t len = tok->pos - start;
        if (tok->pos < tok->input_len) tok->pos++; /* skip closing quote */

        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_STRING);
        if (token) {
            token->value = arena_strdup(tok->arena, tok->input + start, len);
            token->value_len = len;
        }
        return token;
    }

    /* 5. Hash token (#) */
    if (c == '#') {
        tok->pos++;
        size_t start = tok->pos;
        /* Hash can contain hex digits and name chars */
        while (tok->pos < tok->input_len && is_name_char(tok->input[tok->pos])) {
            tok->pos++;
        }
        size_t len = tok->pos - start;
        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_HASH);
        if (token && len > 0) {
            token->value = arena_strdup(tok->arena, tok->input + start, len);
            token->value_len = len;
        }
        return token;
    }

    /* 6. At-keyword (@xxx) */
    if (c == '@') {
        tok->pos++;
        if (tok->pos < tok->input_len && is_name_start(tok->input[tok->pos])) {
            size_t start = tok->pos;
            consume_name(tok);
            size_t len = tok->pos - start;
            silk_css_token_t *token = create_token(tok->arena, CSS_TOK_AT_KEYWORD);
            if (token) {
                token->value = arena_strdup(tok->arena, tok->input + start, len);
                token->value_len = len;
            }
            return token;
        }
        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_DELIM);
        if (token) token->delim = '@';
        return token;
    }

    /* 7. Number, percentage, dimension */
    if (starts_number(tok)) {
        size_t num_start = tok->pos;
        double num = consume_number(tok);

        /* Check for % */
        if (peek(tok) == '%') {
            tok->pos++;
            silk_css_token_t *token = create_token(tok->arena, CSS_TOK_PERCENTAGE);
            if (token) {
                token->numeric_value = num;
                size_t len = tok->pos - num_start;
                token->value = arena_strdup(tok->arena, tok->input + num_start, len);
                token->value_len = len;
            }
            return token;
        }

        /* Check for dimension unit (ident after number) */
        if (is_name_start(peek(tok))) {
            size_t unit_start = tok->pos;
            consume_name(tok);
            size_t unit_len = tok->pos - unit_start;

            silk_css_token_t *token = create_token(tok->arena, CSS_TOK_DIMENSION);
            if (token) {
                token->numeric_value = num;
                token->unit = arena_strdup(tok->arena, tok->input + unit_start, unit_len);
                token->unit_len = unit_len;
                size_t total_len = tok->pos - num_start;
                token->value = arena_strdup(tok->arena, tok->input + num_start, total_len);
                token->value_len = total_len;
            }
            return token;
        }

        /* Plain number */
        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_NUMBER);
        if (token) {
            token->numeric_value = num;
            size_t len = tok->pos - num_start;
            token->value = arena_strdup(tok->arena, tok->input + num_start, len);
            token->value_len = len;
        }
        return token;
    }

    /* 8. Ident or function token */
    if (is_name_start(c)) {
        size_t start = tok->pos;
        consume_name(tok);
        size_t len = tok->pos - start;

        /* Check if followed by '(' -> function token */
        if (peek(tok) == '(') {
            tok->pos++; /* consume '(' */
            silk_css_token_t *token = create_token(tok->arena, CSS_TOK_FUNCTION);
            if (token) {
                token->value = arena_strdup(tok->arena, tok->input + start, len);
                token->value_len = len;
            }
            return token;
        }

        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_IDENT);
        if (token) {
            token->value = arena_strdup(tok->arena, tok->input + start, len);
            token->value_len = len;
        }
        return token;
    }

    /* 9. Dot followed by digit -> number */
    if (c == '.' && tok->pos + 1 < tok->input_len &&
        isdigit((unsigned char)tok->input[tok->pos + 1])) {
        size_t num_start = tok->pos;
        double num = consume_number(tok);
        silk_css_token_t *token = create_token(tok->arena, CSS_TOK_NUMBER);
        if (token) {
            token->numeric_value = num;
            size_t len = tok->pos - num_start;
            token->value = arena_strdup(tok->arena, tok->input + num_start, len);
            token->value_len = len;
        }
        return token;
    }

    /* 10. Any other character -> DELIM */
    tok->pos++;
    silk_css_token_t *token = create_token(tok->arena, CSS_TOK_DELIM);
    if (token) token->delim = c;
    return token;
}

silk_css_token_t *silk_css_tokenizer_peek(silk_css_tokenizer_t *tok) {
    size_t saved_pos = tok->pos;
    silk_css_token_t *token = silk_css_tokenizer_next_token(tok);
    tok->pos = saved_pos;
    return token;
}
