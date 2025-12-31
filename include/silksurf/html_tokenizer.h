/*
 * SilkSurf HTML5 Tokenizer
 *
 * Self-contained HTML5 tokenization engine following WHATWG HTML spec:
 * https://html.spec.whatwg.org/multipage/parsing.html#tokenization
 *
 * This is a cleanroom implementation - no external library dependencies.
 * All tokenizer states from the HTML5 specification are implemented.
 *
 * Copyright (c) 2025 SilkSurf Project
 * SPDX-License-Identifier: MIT
 */

#ifndef SILK_HTML_TOKENIZER_H
#define SILK_HTML_TOKENIZER_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include "silksurf/allocator.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * HTML5 Tokenizer States (70 states from WHATWG spec)
 * ============================================================================ */

typedef enum {
    /* Basic states */
    HTML_TOK_DATA,
    HTML_TOK_RCDATA,
    HTML_TOK_RAWTEXT,
    HTML_TOK_SCRIPT_DATA,
    HTML_TOK_PLAINTEXT,

    /* Tag states */
    HTML_TOK_TAG_OPEN,
    HTML_TOK_END_TAG_OPEN,
    HTML_TOK_TAG_NAME,

    /* RCDATA states */
    HTML_TOK_RCDATA_LESS_THAN_SIGN,
    HTML_TOK_RCDATA_END_TAG_OPEN,
    HTML_TOK_RCDATA_END_TAG_NAME,

    /* RAWTEXT states */
    HTML_TOK_RAWTEXT_LESS_THAN_SIGN,
    HTML_TOK_RAWTEXT_END_TAG_OPEN,
    HTML_TOK_RAWTEXT_END_TAG_NAME,

    /* Script data states */
    HTML_TOK_SCRIPT_DATA_LESS_THAN_SIGN,
    HTML_TOK_SCRIPT_DATA_END_TAG_OPEN,
    HTML_TOK_SCRIPT_DATA_END_TAG_NAME,
    HTML_TOK_SCRIPT_DATA_ESCAPE_START,
    HTML_TOK_SCRIPT_DATA_ESCAPE_START_DASH,
    HTML_TOK_SCRIPT_DATA_ESCAPED,
    HTML_TOK_SCRIPT_DATA_ESCAPED_DASH,
    HTML_TOK_SCRIPT_DATA_ESCAPED_DASH_DASH,
    HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN,
    HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_OPEN,
    HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_NAME,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_START,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN,
    HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_END,

    /* Attribute states */
    HTML_TOK_BEFORE_ATTRIBUTE_NAME,
    HTML_TOK_ATTRIBUTE_NAME,
    HTML_TOK_AFTER_ATTRIBUTE_NAME,
    HTML_TOK_BEFORE_ATTRIBUTE_VALUE,
    HTML_TOK_ATTRIBUTE_VALUE_DOUBLE_QUOTED,
    HTML_TOK_ATTRIBUTE_VALUE_SINGLE_QUOTED,
    HTML_TOK_ATTRIBUTE_VALUE_UNQUOTED,
    HTML_TOK_AFTER_ATTRIBUTE_VALUE_QUOTED,
    HTML_TOK_SELF_CLOSING_START_TAG,

    /* Comment states */
    HTML_TOK_BOGUS_COMMENT,
    HTML_TOK_MARKUP_DECLARATION_OPEN,
    HTML_TOK_COMMENT_START,
    HTML_TOK_COMMENT_START_DASH,
    HTML_TOK_COMMENT,
    HTML_TOK_COMMENT_LESS_THAN_SIGN,
    HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG,
    HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH,
    HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH,
    HTML_TOK_COMMENT_END_DASH,
    HTML_TOK_COMMENT_END,
    HTML_TOK_COMMENT_END_BANG,

    /* DOCTYPE states */
    HTML_TOK_DOCTYPE,
    HTML_TOK_BEFORE_DOCTYPE_NAME,
    HTML_TOK_DOCTYPE_NAME,
    HTML_TOK_AFTER_DOCTYPE_NAME,
    HTML_TOK_AFTER_DOCTYPE_PUBLIC_KEYWORD,
    HTML_TOK_BEFORE_DOCTYPE_PUBLIC_IDENTIFIER,
    HTML_TOK_DOCTYPE_PUBLIC_IDENTIFIER_DOUBLE_QUOTED,
    HTML_TOK_DOCTYPE_PUBLIC_IDENTIFIER_SINGLE_QUOTED,
    HTML_TOK_AFTER_DOCTYPE_PUBLIC_IDENTIFIER,
    HTML_TOK_BETWEEN_DOCTYPE_PUBLIC_AND_SYSTEM_IDENTIFIERS,
    HTML_TOK_AFTER_DOCTYPE_SYSTEM_KEYWORD,
    HTML_TOK_BEFORE_DOCTYPE_SYSTEM_IDENTIFIER,
    HTML_TOK_DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED,
    HTML_TOK_DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED,
    HTML_TOK_AFTER_DOCTYPE_SYSTEM_IDENTIFIER,
    HTML_TOK_BOGUS_DOCTYPE,

    /* CDATA states */
    HTML_TOK_CDATA_SECTION,
    HTML_TOK_CDATA_SECTION_BRACKET,
    HTML_TOK_CDATA_SECTION_END,

    /* Character reference states */
    HTML_TOK_CHARACTER_REFERENCE,
    HTML_TOK_NAMED_CHARACTER_REFERENCE,
    HTML_TOK_AMBIGUOUS_AMPERSAND,
    HTML_TOK_NUMERIC_CHARACTER_REFERENCE,
    HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE_START,
    HTML_TOK_DECIMAL_CHARACTER_REFERENCE_START,
    HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE,
    HTML_TOK_DECIMAL_CHARACTER_REFERENCE,
    HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END
} silk_html_tokenizer_state_t;

/* ============================================================================
 * Token Types (6 types from HTML5 spec)
 * ============================================================================ */

typedef enum {
    HTML_TOKEN_DOCTYPE,
    HTML_TOKEN_START_TAG,
    HTML_TOKEN_END_TAG,
    HTML_TOKEN_COMMENT,
    HTML_TOKEN_CHARACTER,
    HTML_TOKEN_EOF
} silk_html_token_type_t;

/* ============================================================================
 * Token Structures
 * ============================================================================ */

/**
 * HTML attribute
 *
 * Represents a single attribute on a start tag.
 * Arena-allocated - no manual free needed.
 */
typedef struct {
    char *name;         /* Attribute name (lowercased) */
    char *value;        /* Attribute value */
    size_t name_len;    /* Length of name */
    size_t value_len;   /* Length of value */
} silk_html_attribute_t;

/**
 * DOCTYPE token data
 *
 * HTML5 distinguishes between missing and empty strings:
 * - missing_name = true means no name was present
 * - name = "" with missing_name = false means name was empty string
 */
typedef struct {
    char *name;                     /* DOCTYPE name */
    char *public_identifier;        /* PUBLIC identifier */
    char *system_identifier;        /* SYSTEM identifier */
    bool missing_name;              /* True if name missing */
    bool missing_public_identifier; /* True if PUBLIC missing */
    bool missing_system_identifier; /* True if SYSTEM missing */
    bool force_quirks;              /* Quirks mode flag */
} silk_html_doctype_data_t;

/**
 * HTML5 token
 *
 * Unified token structure for all token types.
 * Uses tagged union for type-specific data.
 * Arena-allocated - cleaned up with arena.
 */
typedef struct {
    silk_html_token_type_t type;

    /* Tag tokens (START_TAG, END_TAG) */
    char *tag_name;                     /* Tag name (lowercased) */
    silk_html_attribute_t *attributes;  /* Array of attributes */
    int attribute_count;                /* Number of attributes */
    bool self_closing;                  /* Self-closing flag (/) */
    bool self_closing_acknowledged;     /* Parser acknowledged flag */

    /* Character token (CHARACTER) */
    char *character_data;               /* Character text (UTF-8) */
    size_t character_len;               /* Length of text */

    /* Comment token (COMMENT) */
    char *comment_data;                 /* Comment text */
    size_t comment_len;                 /* Comment length */

    /* DOCTYPE token (DOCTYPE) */
    silk_html_doctype_data_t *doctype_data;

    /* Position tracking for error reporting */
    size_t start_line;
    size_t start_column;
    size_t end_line;
    size_t end_column;
} silk_html_token_t;

/* ============================================================================
 * UTF-8 Input Stream
 * ============================================================================ */

/**
 * UTF-8 input stream with lookahead
 *
 * Handles UTF-8 decoding and provides character-by-character access
 * to the HTML input. Tracks line/column for error reporting.
 */
typedef struct {
    const char *input;       /* Input buffer (UTF-8) */
    size_t input_len;        /* Input length in bytes */
    size_t pos;              /* Current byte position */

    /* Position tracking */
    size_t line;             /* Current line (1-based) */
    size_t column;           /* Current column (1-based) */

    /* Lookahead buffer (for peeking ahead) */
    struct {
        uint32_t cp;
        int len;
    } lookahead[16];
    int lookahead_count;     /* Number of valid lookahead entries */
    size_t lookahead_pos;    /* Byte position where lookahead starts */

    /* EOF flag */
    bool at_eof;
} silk_html_input_stream_t;

/* ============================================================================
 * Tokenizer Context
 * ============================================================================ */

/**
 * HTML5 tokenizer context
 *
 * Main tokenizer state machine. Processes HTML input character-by-character
 * and emits tokens according to HTML5 specification.
 */
typedef struct {
    /* Arena for allocations */
    silk_arena_t *arena;

    /* Input stream */
    silk_html_input_stream_t *stream;

    /* Current state */
    silk_html_tokenizer_state_t state;
    silk_html_tokenizer_state_t return_state;  /* For nested states */

    /* Current token being constructed by the state machine */
    silk_html_token_t *active_token;

    /* Token ready to be returned by next_token */
    silk_html_token_t *emitted_token;

    /* Pending token to be emitted after characters */
    silk_html_token_t *pending_token;

    /* Temporary buffer for building strings */
    char *temp_buffer;
    size_t temp_buffer_size;
    size_t temp_buffer_capacity;

    /* Buffer for accumulating character tokens */
    char *char_buffer;
    size_t char_buffer_size;
    size_t char_buffer_capacity;

    /* Character reference decoding */
    uint32_t char_ref_code;

    /* Last emitted start tag name (for end tag matching) */
    char *last_start_tag_name;

    /* Reconsume support */
    uint32_t current_char;      /* Current character being processed */
    bool reconsume;              /* True if current_char should be reconsumed */

    /* Error callback */
    void (*error_callback)(void *context, const char *message, size_t line, size_t column);
    void *error_context;
} silk_html_tokenizer_t;

/* ============================================================================
 * Tokenizer API
 * ============================================================================ */

/**
 * Create a new HTML5 tokenizer
 *
 * @param arena Arena for allocations (tokenizer will allocate from this)
 * @param input UTF-8 encoded HTML input
 * @param input_len Length of input in bytes
 * @return New tokenizer, or NULL on error
 */
silk_html_tokenizer_t *silk_html_tokenizer_create(
    silk_arena_t *arena,
    const char *input,
    size_t input_len
);

/**
 * Destroy HTML5 tokenizer
 */
void silk_html_tokenizer_destroy(silk_html_tokenizer_t *tokenizer);

/**
 * Get next token from tokenizer
 *
 * Advances the tokenizer state machine and returns the next token.
 * Returns NULL when EOF is reached or on error.
 *
 * Token is arena-allocated and valid until arena is destroyed.
 *
 * @param tokenizer Tokenizer context
 * @return Next token, or NULL at EOF/error
 */
silk_html_token_t *silk_html_tokenizer_next_token(silk_html_tokenizer_t *tokenizer);

/**
 * Set tokenizer state
 *
 * Used by parser to switch tokenizer state when entering special contexts
 * (e.g., RCDATA for <title>, RAWTEXT for <style>).
 *
 * @param tokenizer Tokenizer context
 * @param state New state
 */
void silk_html_tokenizer_set_state(
    silk_html_tokenizer_t *tokenizer,
    silk_html_tokenizer_state_t state
);

/**
 * Set error callback
 *
 * Called when parse errors are encountered (for error reporting).
 *
 * @param tokenizer Tokenizer context
 * @param callback Error callback function
 * @param context User context passed to callback
 */
void silk_html_tokenizer_set_error_callback(
    silk_html_tokenizer_t *tokenizer,
    void (*callback)(void *context, const char *message, size_t line, size_t column),
    void *context
);

/* ============================================================================
 * UTF-8 Input Stream API
 * ============================================================================ */

/**
 * Create input stream from UTF-8 string
 *
 * @param arena Arena for allocations
 * @param input UTF-8 encoded input
 * @param input_len Length in bytes
 * @return New input stream, or NULL on error
 */
silk_html_input_stream_t *silk_html_input_stream_create(
    silk_arena_t *arena,
    const char *input,
    size_t input_len
);

/**
 * Get next code point from stream
 *
 * Decodes UTF-8 and returns next Unicode code point.
 * Returns 0xFFFFFFFF on EOF.
 *
 * @param stream Input stream
 * @return Next code point, or 0xFFFFFFFF at EOF
 */
uint32_t silk_html_input_stream_next(silk_html_input_stream_t *stream);

/**
 * Peek at next code point without consuming
 *
 * @param stream Input stream
 * @param offset Offset from current position (0 = next char, 1 = char after, etc.)
 * @return Code point at offset, or 0xFFFFFFFF if beyond EOF
 */
uint32_t silk_html_input_stream_peek(silk_html_input_stream_t *stream, int offset);

/**
 * Check if at EOF
 *
 * @param stream Input stream
 * @return True if at end of input
 */
bool silk_html_input_stream_is_eof(silk_html_input_stream_t *stream);

/**
 * Get current position (line, column)
 *
 * @param stream Input stream
 * @param line Output: current line (1-based)
 * @param column Output: current column (1-based)
 */
void silk_html_input_stream_get_position(
    silk_html_input_stream_t *stream,
    size_t *line,
    size_t *column
);

/* ============================================================================
 * Character Reference Decoding
 * ============================================================================ */

/**
 * Decode named character reference
 *
 * Looks up named entity (e.g., "&amp;", "&lt;", "&nbsp;") and returns
 * the corresponding Unicode code point(s).
 *
 * @param name Entity name (without & and ;)
 * @param name_len Length of name
 * @param code_points Output buffer for code points (max 2)
 * @param consumed Output: number of characters consumed from name
 * @return Number of code points decoded (1 or 2), or 0 if not found
 */
int silk_html_decode_named_char_ref(
    const char *name,
    size_t name_len,
    bool has_semicolon,
    uint32_t *code_points,
    size_t *consumed
);

/**
 * Decode numeric character reference
 *
 * Decodes &#1234; (decimal) or &#xABCD; (hexadecimal) references.
 *
 * @param ref_str Numeric string (after &#)
 * @param ref_len Length of string
 * @param is_hex True if hexadecimal, false if decimal
 * @return Decoded code point, or 0xFFFD (replacement char) on error
 */
uint32_t silk_html_decode_numeric_char_ref(
    const char *ref_str,
    size_t ref_len,
    bool is_hex
);

/* ============================================================================
 * Utility Functions
 * ============================================================================ */

/**
 * Check if code point is ASCII whitespace
 * HTML5 defines whitespace as: tab (0x09), LF (0x0A), FF (0x0C), CR (0x0D), space (0x20)
 */
static inline bool silk_html_is_whitespace(uint32_t cp) {
    return cp == 0x09 || cp == 0x0A || cp == 0x0C || cp == 0x0D || cp == 0x20;
}

/**
 * Check if code point is ASCII alpha (A-Z, a-z)
 */
static inline bool silk_html_is_alpha(uint32_t cp) {
    return (cp >= 'A' && cp <= 'Z') || (cp >= 'a' && cp <= 'z');
}

/**
 * Check if code point is ASCII digit (0-9)
 */
static inline bool silk_html_is_digit(uint32_t cp) {
    return cp >= '0' && cp <= '9';
}

/**
 * Check if code point is ASCII hex digit (0-9, A-F, a-f)
 */
static inline bool silk_html_is_hex_digit(uint32_t cp) {
    return silk_html_is_digit(cp) ||
           (cp >= 'A' && cp <= 'F') ||
           (cp >= 'a' && cp <= 'f');
}

/**
 * Check if code point is ASCII upper alpha (A-Z)
 */
static inline bool silk_html_is_upper_alpha(uint32_t cp) {
    return cp >= 'A' && cp <= 'Z';
}

/**
 * Convert ASCII upper to lower (A-Z -> a-z)
 */
static inline uint32_t silk_html_to_lower(uint32_t cp) {
    if (cp >= 'A' && cp <= 'Z')
        return cp + 0x20;
    return cp;
}

/**
 * Get state from name (for test harness)
 */
silk_html_tokenizer_state_t silk_html_tokenizer_state_from_name(const char *name);

/**
 * Get state name for debugging
 */
const char *silk_html_tokenizer_state_name(silk_html_tokenizer_state_t state);

/**
 * Get token type name for debugging
 */
const char *silk_html_token_type_name(silk_html_token_type_t type);

#ifdef __cplusplus
}
#endif

#endif /* SILK_HTML_TOKENIZER_H */
