/*
 * SilkSurf HTML5 Tokenizer Implementation
 *
 * Self-contained HTML5 tokenization engine following WHATWG HTML spec:
 * https://html.spec.whatwg.org/multipage/parsing.html#tokenization
 *
 * Copyright (c) 2025 SilkSurf Project
 * SPDX-License-Identifier: MIT
 */

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <ctype.h>
#include "silksurf/html_tokenizer.h"
#include "silksurf/allocator.h"

/* ============================================================================
 * Constants
 * ============================================================================ */

#define REPLACEMENT_CHARACTER 0xFFFD
#define EOF_CODE_POINT 0xFFFFFFFF

/* Initial capacity for temporary buffer */
#define INITIAL_TEMP_BUFFER_CAPACITY 256

/* ============================================================================
 * Debug Functions
 * ============================================================================ */

silk_html_tokenizer_state_t silk_html_tokenizer_state_from_name(const char *name) {
    if (strcmp(name, "Data state") == 0) return HTML_TOK_DATA;
    if (strcmp(name, "RCDATA state") == 0) return HTML_TOK_RCDATA;
    if (strcmp(name, "RAWTEXT state") == 0) return HTML_TOK_RAWTEXT;
    if (strcmp(name, "Script data state") == 0) return HTML_TOK_SCRIPT_DATA;
    if (strcmp(name, "PLAINTEXT state") == 0) return HTML_TOK_PLAINTEXT;
    /* Add more as needed by tests */
    return HTML_TOK_DATA;
}

const char *silk_html_tokenizer_state_name(silk_html_tokenizer_state_t state) {
    switch (state) {
        case HTML_TOK_DATA: return "Data";
        case HTML_TOK_RCDATA: return "RCDATA";
        case HTML_TOK_RAWTEXT: return "RAWTEXT";
        case HTML_TOK_SCRIPT_DATA: return "ScriptData";
        case HTML_TOK_PLAINTEXT: return "PLAINTEXT";
        case HTML_TOK_TAG_OPEN: return "TagOpen";
        case HTML_TOK_END_TAG_OPEN: return "EndTagOpen";
        case HTML_TOK_TAG_NAME: return "TagName";
        case HTML_TOK_RCDATA_LESS_THAN_SIGN: return "RCDATALessThanSign";
        case HTML_TOK_RCDATA_END_TAG_OPEN: return "RCDATAEndTagOpen";
        case HTML_TOK_RCDATA_END_TAG_NAME: return "RCDATAEndTagName";
        case HTML_TOK_RAWTEXT_LESS_THAN_SIGN: return "RAWTEXTLessThanSign";
        case HTML_TOK_RAWTEXT_END_TAG_OPEN: return "RAWTEXTEndTagOpen";
        case HTML_TOK_RAWTEXT_END_TAG_NAME: return "RAWTEXTEndTagName";
        case HTML_TOK_SCRIPT_DATA_LESS_THAN_SIGN: return "ScriptDataLessThanSign";
        case HTML_TOK_SCRIPT_DATA_END_TAG_OPEN: return "ScriptDataEndTagOpen";
        case HTML_TOK_SCRIPT_DATA_END_TAG_NAME: return "ScriptDataEndTagName";
        case HTML_TOK_SCRIPT_DATA_ESCAPE_START: return "ScriptDataEscapeStart";
        case HTML_TOK_SCRIPT_DATA_ESCAPE_START_DASH: return "ScriptDataEscapeStartDash";
        case HTML_TOK_SCRIPT_DATA_ESCAPED: return "ScriptDataEscaped";
        case HTML_TOK_SCRIPT_DATA_ESCAPED_DASH: return "ScriptDataEscapedDash";
        case HTML_TOK_SCRIPT_DATA_ESCAPED_DASH_DASH: return "ScriptDataEscapedDashDash";
        case HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN: return "ScriptDataEscapedLessThanSign";
        case HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_OPEN: return "ScriptDataEscapedEndTagOpen";
        case HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_NAME: return "ScriptDataEscapedEndTagName";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_START: return "ScriptDataDoubleEscapeStart";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED: return "ScriptDataDoubleEscaped";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH: return "ScriptDataDoubleEscapedDash";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH: return "ScriptDataDoubleEscapedDashDash";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN: return "ScriptDataDoubleEscapedLessThanSign";
        case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_END: return "ScriptDataDoubleEscapeEnd";
        case HTML_TOK_BEFORE_ATTRIBUTE_NAME: return "BeforeAttributeName";
        case HTML_TOK_ATTRIBUTE_NAME: return "AttributeName";
        case HTML_TOK_AFTER_ATTRIBUTE_NAME: return "AfterAttributeName";
        case HTML_TOK_BEFORE_ATTRIBUTE_VALUE: return "BeforeAttributeValue";
        case HTML_TOK_ATTRIBUTE_VALUE_DOUBLE_QUOTED: return "AttributeValueDoubleQuoted";
        case HTML_TOK_ATTRIBUTE_VALUE_SINGLE_QUOTED: return "AttributeValueSingleQuoted";
        case HTML_TOK_ATTRIBUTE_VALUE_UNQUOTED: return "AttributeValueUnquoted";
        case HTML_TOK_AFTER_ATTRIBUTE_VALUE_QUOTED: return "AfterAttributeValueQuoted";
        case HTML_TOK_SELF_CLOSING_START_TAG: return "SelfClosingStartTag";
        case HTML_TOK_BOGUS_COMMENT: return "BogusComment";
        case HTML_TOK_MARKUP_DECLARATION_OPEN: return "MarkupDeclarationOpen";
        case HTML_TOK_COMMENT_START: return "CommentStart";
        case HTML_TOK_COMMENT_START_DASH: return "CommentStartDash";
        case HTML_TOK_COMMENT: return "Comment";
        case HTML_TOK_COMMENT_LESS_THAN_SIGN: return "CommentLessThanSign";
        case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG: return "CommentLessThanSignBang";
        case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH: return "CommentLessThanSignBangDash";
        case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH: return "CommentLessThanSignBangDashDash";
        case HTML_TOK_COMMENT_END_DASH: return "CommentEndDash";
        case HTML_TOK_COMMENT_END: return "CommentEnd";
        case HTML_TOK_COMMENT_END_BANG: return "CommentEndBang";
        case HTML_TOK_DOCTYPE: return "DOCTYPE";
        case HTML_TOK_BEFORE_DOCTYPE_NAME: return "BeforeDOCTYPEName";
        case HTML_TOK_DOCTYPE_NAME: return "DOCTYPEName";
        case HTML_TOK_AFTER_DOCTYPE_NAME: return "AfterDOCTYPEName";
        case HTML_TOK_AFTER_DOCTYPE_PUBLIC_KEYWORD: return "AfterDOCTYPEPublicKeyword";
        case HTML_TOK_BEFORE_DOCTYPE_PUBLIC_IDENTIFIER: return "BeforeDOCTYPEPublicIdentifier";
        case HTML_TOK_DOCTYPE_PUBLIC_IDENTIFIER_DOUBLE_QUOTED: return "DOCTYPEPublicIdentifierDoubleQuoted";
        case HTML_TOK_DOCTYPE_PUBLIC_IDENTIFIER_SINGLE_QUOTED: return "DOCTYPEPublicIdentifierSingleQuoted";
        case HTML_TOK_AFTER_DOCTYPE_PUBLIC_IDENTIFIER: return "AfterDOCTYPEPublicIdentifier";
        case HTML_TOK_BETWEEN_DOCTYPE_PUBLIC_AND_SYSTEM_IDENTIFIERS: return "BetweenDOCTYPEPublicAndSystemIdentifiers";
        case HTML_TOK_AFTER_DOCTYPE_SYSTEM_KEYWORD: return "AfterDOCTYPESystemKeyword";
        case HTML_TOK_BEFORE_DOCTYPE_SYSTEM_IDENTIFIER: return "BeforeDOCTYPESystemIdentifier";
        case HTML_TOK_DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED: return "DOCTYPESystemIdentifierDoubleQuoted";
        case HTML_TOK_DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED: return "DOCTYPESystemIdentifierSingleQuoted";
        case HTML_TOK_AFTER_DOCTYPE_SYSTEM_IDENTIFIER: return "AfterDOCTYPESystemIdentifier";
        case HTML_TOK_BOGUS_DOCTYPE: return "BogusDOCTYPE";
        case HTML_TOK_CDATA_SECTION: return "CDATASection";
        case HTML_TOK_CDATA_SECTION_BRACKET: return "CDATASectionBracket";
        case HTML_TOK_CDATA_SECTION_END: return "CDATASectionEnd";
        case HTML_TOK_CHARACTER_REFERENCE: return "CharacterReference";
        case HTML_TOK_NAMED_CHARACTER_REFERENCE: return "NamedCharacterReference";
        case HTML_TOK_AMBIGUOUS_AMPERSAND: return "AmbiguousAmpersand";
        case HTML_TOK_NUMERIC_CHARACTER_REFERENCE: return "NumericCharacterReference";
        case HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE_START: return "HexadecimalCharacterReferenceStart";
        case HTML_TOK_DECIMAL_CHARACTER_REFERENCE_START: return "DecimalCharacterReferenceStart";
        case HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE: return "HexadecimalCharacterReference";
        case HTML_TOK_DECIMAL_CHARACTER_REFERENCE: return "DecimalCharacterReference";
        case HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END: return "NumericCharacterReferenceEnd";
        default: return "Unknown";
    }
}

const char *silk_html_token_type_name(silk_html_token_type_t type) {
    switch (type) {
        case HTML_TOKEN_DOCTYPE: return "DOCTYPE";
        case HTML_TOKEN_START_TAG: return "StartTag";
        case HTML_TOKEN_END_TAG: return "EndTag";
        case HTML_TOKEN_COMMENT: return "Comment";
        case HTML_TOKEN_CHARACTER: return "Character";
        case HTML_TOKEN_EOF: return "EOF";
        default: return "Unknown";
    }
}

/* ============================================================================
 * UTF-8 Input Stream Implementation
 * ============================================================================ */

/**
 * Decode one UTF-8 code point from buffer
 *
 * Returns number of bytes consumed (1-4), or 0 on error.
 * Sets *code_point to decoded value, or REPLACEMENT_CHARACTER on error.
 */
static int utf8_decode(const uint8_t *bytes, size_t len, uint32_t *code_point) {
    if (len == 0) {
        *code_point = EOF_CODE_POINT;
        return 0;
    }

    uint8_t first = bytes[0];

    /* ASCII (0x00-0x7F) */
    if (first < 0x80) {
        *code_point = first;
        return 1;
    }

    /* 2-byte sequence (0xC0-0xDF) */
    if ((first & 0xE0) == 0xC0) {
        if (len < 2 || (bytes[1] & 0xC0) != 0x80) {
            *code_point = REPLACEMENT_CHARACTER;
            return 1;  /* Skip invalid byte */
        }
        *code_point = ((first & 0x1F) << 6) | (bytes[1] & 0x3F);
        return 2;
    }

    /* 3-byte sequence (0xE0-0xEF) */
    if ((first & 0xF0) == 0xE0) {
        if (len < 3 || (bytes[1] & 0xC0) != 0x80 || (bytes[2] & 0xC0) != 0x80) {
            *code_point = REPLACEMENT_CHARACTER;
            return 1;
        }
        *code_point = ((first & 0x0F) << 12) |
                      ((bytes[1] & 0x3F) << 6) |
                      (bytes[2] & 0x3F);
        return 3;
    }

    /* 4-byte sequence (0xF0-0xF7) */
    if ((first & 0xF8) == 0xF0) {
        if (len < 4 ||
            (bytes[1] & 0xC0) != 0x80 ||
            (bytes[2] & 0xC0) != 0x80 ||
            (bytes[3] & 0xC0) != 0x80) {
            *code_point = REPLACEMENT_CHARACTER;
            return 1;
        }
        *code_point = ((first & 0x07) << 18) |
                      ((bytes[1] & 0x3F) << 12) |
                      ((bytes[2] & 0x3F) << 6) |
                      (bytes[3] & 0x3F);
        return 4;
    }

    /* Invalid UTF-8 */
    *code_point = REPLACEMENT_CHARACTER;
    return 1;
}

silk_html_input_stream_t *silk_html_input_stream_create(
    silk_arena_t *arena,
    const char *input,
    size_t input_len)
{
    if (!arena || !input) {
        return NULL;
    }

    silk_html_input_stream_t *stream =
        silk_arena_alloc(arena, sizeof(silk_html_input_stream_t));
    if (!stream) {
        return NULL;
    }

    stream->input = input;
    stream->input_len = input_len;
    stream->pos = 0;
    stream->line = 1;
    stream->column = 1;
    stream->lookahead_count = 0;
    stream->lookahead_pos = 0;
    stream->at_eof = (input_len == 0);

    return stream;
}

uint32_t silk_html_input_stream_next(silk_html_input_stream_t *stream) {
    if (!stream || stream->at_eof) {
        return EOF_CODE_POINT;
    }

    /* Use lookahead if available and valid */
    if (stream->lookahead_count > 0 && stream->lookahead_pos == stream->pos) {
        uint32_t cp = stream->lookahead[0].cp;
        int len = stream->lookahead[0].len;

        /* Advance position */
        stream->pos += len;
        stream->lookahead_pos = stream->pos;

        /* Shift lookahead buffer */
        for (int i = 1; i < stream->lookahead_count; i++) {
            stream->lookahead[i - 1] = stream->lookahead[i];
        }
        stream->lookahead_count--;

        /* Update line/column */
        if (cp == '\n') {
            stream->line++;
            stream->column = 1;
        } else {
            stream->column++;
        }

        /* Check for EOF */
        if (stream->pos >= stream->input_len) {
            stream->at_eof = true;
        }

        return cp;
    }

    /* Invalidate lookahead since we're about to change position */
    stream->lookahead_count = 0;

    /* Decode next UTF-8 code point */
    const uint8_t *bytes = (const uint8_t *)stream->input + stream->pos;
    size_t remaining = stream->input_len - stream->pos;

    uint32_t cp;
    int consumed = utf8_decode(bytes, remaining, &cp);

    if (consumed == 0 || cp == EOF_CODE_POINT) {
        stream->at_eof = true;
        return EOF_CODE_POINT;
    }

    stream->pos += consumed;

    /* Update line/column tracking */
    if (cp == '\n') {
        stream->line++;
        stream->column = 1;
    } else if (cp == '\r') {
        /* HTML5 preprocessor: CR and CRLF are normalized to LF */
        /* Check if next char is LF */
        if (stream->pos < stream->input_len &&
            stream->input[stream->pos] == '\n') {
            /* CRLF -> skip the LF, return LF */
            stream->pos++;
        }
        /* CR -> return LF */
        cp = '\n';
        stream->line++;
        stream->column = 1;
    } else {
        stream->column++;
    }

    /* Check for EOF */
    if (stream->pos >= stream->input_len) {
        stream->at_eof = true;
    }

    return cp;
}

uint32_t silk_html_input_stream_peek(silk_html_input_stream_t *stream, int offset) {
    if (!stream || offset < 0) {
        return EOF_CODE_POINT;
    }

    /* If lookahead is valid and we have this offset, return it */
    if (stream->lookahead_pos == stream->pos && offset < stream->lookahead_count) {
        return stream->lookahead[offset].cp;
    }

    /* Lookahead is invalid or insufficient - rebuild from current position */
    if (stream->lookahead_pos != stream->pos) {
        stream->lookahead_count = 0;
        stream->lookahead_pos = stream->pos;
    }

    /* Start temp position from current lookahead end */
    size_t temp_pos = stream->pos;
    for (int i = 0; i < stream->lookahead_count; i++) {
        temp_pos += stream->lookahead[i].len;
    }

    /* Fill lookahead buffer up to the requested offset */
    while (stream->lookahead_count <= offset) {
        if (temp_pos >= stream->input_len) {
            return EOF_CODE_POINT;
        }

        const uint8_t *bytes = (const uint8_t *)stream->input + temp_pos;
        size_t remaining = stream->input_len - temp_pos;

        uint32_t cp;
        int consumed = utf8_decode(bytes, remaining, &cp);

        if (consumed == 0 || cp == EOF_CODE_POINT) {
            return EOF_CODE_POINT;
        }

        int total_consumed = consumed;
        temp_pos += consumed;

        /* Handle CR/CRLF normalization */
        if (cp == '\r') {
            if (temp_pos < stream->input_len &&
                stream->input[temp_pos] == '\n') {
                temp_pos++;
                total_consumed++;
            }
            cp = '\n';
        }

        /* Add to lookahead */
        if (stream->lookahead_count < 16) {
            stream->lookahead[stream->lookahead_count].cp = cp;
            stream->lookahead[stream->lookahead_count].len = total_consumed;
            stream->lookahead_count++;
        } else {
            return EOF_CODE_POINT;
        }
    }

    return stream->lookahead[offset].cp;
}

bool silk_html_input_stream_is_eof(silk_html_input_stream_t *stream) {
    if (!stream) {
        return true;
    }
    return stream->at_eof && stream->lookahead_count == 0;
}

void silk_html_input_stream_get_position(
    silk_html_input_stream_t *stream,
    size_t *line,
    size_t *column)
{
    if (!stream) {
        if (line) *line = 0;
        if (column) *column = 0;
        return;
    }

    if (line) *line = stream->line;
    if (column) *column = stream->column;
}

/* ============================================================================
 * Character Reference Decoding (Stubs for now)
 * ============================================================================ */

int silk_html_decode_named_char_ref(
    const char *name,
    size_t name_len,
    bool has_semicolon,
    uint32_t *code_points,
    size_t *consumed)
{
    /* Common entities for MVP - Longest match first! */
    struct { const char *name; uint32_t cp; } common_entities[] = {
        { "notin", 0x2209 },
        { "apos", '\'' },
        { "nbsp", 0xA0 },
        { "quot", '"' },
        { "amp", '&' },
        { "not", 0xAC },
        { "lt", '<' },
        { "gt", '>' },
    };

    if (!name || !code_points || !consumed) {
        return 0;
    }

    for (size_t i = 0; i < sizeof(common_entities) / sizeof(common_entities[0]); i++) {
        size_t len = strlen(common_entities[i].name);
        if (name_len >= len && strncmp(name, common_entities[i].name, len) == 0) {
            /* Special case: notin requires a semicolon */
            if (strcmp(common_entities[i].name, "notin") == 0 && !has_semicolon) {
                continue; /* Skip and try shorter match (like 'not') */
            }
            
            code_points[0] = common_entities[i].cp;
            *consumed = len;
            return 1;
        }
    }

    return 0;  /* Not found */
}

uint32_t silk_html_decode_numeric_char_ref(
    const char *ref_str,
    size_t ref_len,
    bool is_hex)
{
    if (!ref_str || ref_len == 0) {
        return REPLACEMENT_CHARACTER;
    }

    uint32_t value = 0;

    for (size_t i = 0; i < ref_len; i++) {
        char c = ref_str[i];
        int digit;

        if (is_hex) {
            if (c >= '0' && c <= '9') {
                digit = c - '0';
            } else if (c >= 'A' && c <= 'F') {
                digit = c - 'A' + 10;
            } else if (c >= 'a' && c <= 'f') {
                digit = c - 'a' + 10;
            } else {
                return REPLACEMENT_CHARACTER;
            }
            value = value * 16 + digit;
        } else {
            if (c >= '0' && c <= '9') {
                digit = c - '0';
            } else {
                return REPLACEMENT_CHARACTER;
            }
            value = value * 10 + digit;
        }

        /* Check for overflow */
        if (value > 0x10FFFF) {
            return REPLACEMENT_CHARACTER;
        }
    }

    /* HTML5 invalid code point checks */
    if (value == 0 ||
        (value >= 0xD800 && value <= 0xDFFF) ||  /* Surrogates */
        value > 0x10FFFF) {
        return REPLACEMENT_CHARACTER;
    }

    return value;
}

/* ============================================================================
 * Tokenizer Implementation (Skeleton)
 * ============================================================================ */

silk_html_tokenizer_t *silk_html_tokenizer_create(
    silk_arena_t *arena,
    const char *input,
    size_t input_len)
{
    if (!arena || !input) {
        return NULL;
    }

    silk_html_tokenizer_t *tokenizer =
        silk_arena_alloc(arena, sizeof(silk_html_tokenizer_t));
    if (!tokenizer) {
        return NULL;
    }

    memset(tokenizer, 0, sizeof(silk_html_tokenizer_t));
    tokenizer->arena = arena;
    tokenizer->stream = silk_html_input_stream_create(arena, input, input_len);
    if (!tokenizer->stream) {
        return NULL;
    }

    tokenizer->state = HTML_TOK_DATA;
    tokenizer->return_state = HTML_TOK_DATA;
    
    /* Allocate buffers from arena */
    tokenizer->temp_buffer_capacity = INITIAL_TEMP_BUFFER_CAPACITY;
    tokenizer->temp_buffer = silk_arena_alloc(arena, tokenizer->temp_buffer_capacity);
    
    tokenizer->char_buffer_capacity = INITIAL_TEMP_BUFFER_CAPACITY;
    tokenizer->char_buffer = silk_arena_alloc(arena, tokenizer->char_buffer_capacity);

    return tokenizer;
}

void silk_html_tokenizer_destroy(silk_html_tokenizer_t *tokenizer) {
    /* Since we use an arena, we don't need to free individual members.
       The arena itself will be destroyed by the caller. */
    (void)tokenizer;
}

void silk_html_tokenizer_set_state(
    silk_html_tokenizer_t *tokenizer,
    silk_html_tokenizer_state_t state)
{
    if (tokenizer) {
        tokenizer->state = state;
    }
}

void silk_html_tokenizer_set_error_callback(
    silk_html_tokenizer_t *tokenizer,
    void (*callback)(void *context, const char *message, size_t line, size_t column),
    void *context)
{
    if (tokenizer) {
        tokenizer->error_callback = callback;
        tokenizer->error_context = context;
    }
}

/**
 * Emit parse error (if callback is set)
 */
static void emit_error(
    silk_html_tokenizer_t *tokenizer,
    const char *message)
{
    if (tokenizer && tokenizer->error_callback) {
        size_t line, column;
        silk_html_input_stream_get_position(tokenizer->stream, &line, &column);
        tokenizer->error_callback(tokenizer->error_context, message, line, column);
    }
}

/* ============================================================================
 * Token Creation Helpers
 * ============================================================================ */

static silk_html_token_t *create_token(silk_html_tokenizer_t *tok, silk_html_token_type_t type) {
    silk_html_token_t *token = silk_arena_alloc(tok->arena, sizeof(silk_html_token_t));
    if (!token) return NULL;

    memset(token, 0, sizeof(silk_html_token_t));
    token->type = type;
    silk_html_input_stream_get_position(tok->stream, &token->start_line, &token->start_column);
    return token;
}

static void emit_character(silk_html_tokenizer_t *tok, uint32_t cp) {
    /* Ensure capacity */
    if (tok->char_buffer_size + 5 >= tok->char_buffer_capacity) {
        size_t new_cap = tok->char_buffer_capacity * 2;
        char *new_buf = silk_arena_alloc(tok->arena, new_cap);
        if (new_buf) {
            memcpy(new_buf, tok->char_buffer, tok->char_buffer_size);
            tok->char_buffer = new_buf;
            tok->char_buffer_capacity = new_cap;
        }
    }

    /* Encode UTF-8 */
    if (cp < 0x80) {
        tok->char_buffer[tok->char_buffer_size++] = (char)cp;
    } else if (cp < 0x800) {
        tok->char_buffer[tok->char_buffer_size++] = (char)(0xC0 | (cp >> 6));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        tok->char_buffer[tok->char_buffer_size++] = (char)(0xE0 | (cp >> 12));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    } else {
        tok->char_buffer[tok->char_buffer_size++] = (char)(0xF0 | (cp >> 18));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | ((cp >> 12) & 0x3F));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        tok->char_buffer[tok->char_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    }
    tok->char_buffer[tok->char_buffer_size] = '\0';
}

static silk_html_token_t *flush_char_buffer(silk_html_tokenizer_t *tok) {
    if (tok->char_buffer_size == 0) return NULL;

    silk_html_token_t *token = create_token(tok, HTML_TOKEN_CHARACTER);
    if (token) {
        token->character_data = silk_arena_alloc(tok->arena, tok->char_buffer_size + 1);
        if (token->character_data) {
            memcpy(token->character_data, tok->char_buffer, tok->char_buffer_size);
            token->character_data[tok->char_buffer_size] = '\0';
            token->character_len = tok->char_buffer_size;
        }
    }
    tok->char_buffer_size = 0;
    return token;
}

static void emit_token(silk_html_tokenizer_t *tok, silk_html_token_t *token) {
    if (!token) return;

    if (token->type == HTML_TOKEN_START_TAG) {
        /* Track last start tag name for appropriate end tag checks */
        if (token->tag_name) {
            tok->last_start_tag_name = silk_arena_alloc(tok->arena, strlen(token->tag_name) + 1);
            if (tok->last_start_tag_name) strcpy(tok->last_start_tag_name, token->tag_name);
        }
    }

    if (tok->char_buffer_size > 0) {
        /* If we have characters, we MUST emit them first.
           Save the new token in pending_token. */
        tok->pending_token = token;
        tok->emitted_token = flush_char_buffer(tok);
    } else {
        tok->emitted_token = token;
    }
}

static void emit_eof(silk_html_tokenizer_t *tok) {
    emit_token(tok, create_token(tok, HTML_TOKEN_EOF));
}

/* ============================================================================
 * String Building Helpers
 * ============================================================================ */

/**
 * Ensure temp_buffer has capacity for at least one more character
 */
static bool ensure_temp_buffer_capacity(silk_html_tokenizer_t *tok) {
    if (tok->temp_buffer_size + 1 >= tok->temp_buffer_capacity) {
        size_t new_capacity = tok->temp_buffer_capacity == 0
            ? INITIAL_TEMP_BUFFER_CAPACITY
            : tok->temp_buffer_capacity * 2;

        char *new_buffer = silk_arena_alloc(tok->arena, new_capacity);
        if (!new_buffer) return false;

        if (tok->temp_buffer && tok->temp_buffer_size > 0) {
            memcpy(new_buffer, tok->temp_buffer, tok->temp_buffer_size);
        }

        tok->temp_buffer = new_buffer;
        tok->temp_buffer_capacity = new_capacity;
    }
    return true;
}

/**
 * Append a character to temp_buffer
 */
static bool append_to_temp_buffer(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (!ensure_temp_buffer_capacity(tok)) return false;

    /* For now, only handle ASCII (U+0000-U+007F) */
    /* TODO: Full UTF-8 encoding for non-ASCII characters */
    if (cp < 0x80) {
        tok->temp_buffer[tok->temp_buffer_size++] = (char)cp;
    } else if (cp < 0x800) {
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0xC0 | (cp >> 6));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0xE0 | (cp >> 12));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    } else {
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0xF0 | (cp >> 18));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | ((cp >> 12) & 0x3F));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        tok->temp_buffer[tok->temp_buffer_size++] = (char)(0x80 | (cp & 0x3F));
    }

    /* Always keep a null terminator just in case, but don't count it in size */
    if (tok->temp_buffer_size < tok->temp_buffer_capacity) {
        tok->temp_buffer[tok->temp_buffer_size] = '\0';
    }

    return true;
}

/**
 * Clear temp_buffer
 */
static void clear_temp_buffer(silk_html_tokenizer_t *tok) {
    tok->temp_buffer_size = 0;
}

/**
 * Convert temp_buffer contents to lowercase
 */
static void temp_buffer_to_lowercase(silk_html_tokenizer_t *tok) {
    for (size_t i = 0; i < tok->temp_buffer_size; i++) {
        char c = tok->temp_buffer[i];
        if (c >= 'A' && c <= 'Z') {
            tok->temp_buffer[i] = c + ('a' - 'A');
        }
    }
}

/**
 * Copy temp_buffer to a new string
 */
static char *copy_temp_buffer(silk_html_tokenizer_t *tok) {
    char *str = silk_arena_alloc(tok->arena, tok->temp_buffer_size + 1);
    if (!str) return NULL;

    if (tok->temp_buffer_size > 0) {
        memcpy(str, tok->temp_buffer, tok->temp_buffer_size);
    }
    str[tok->temp_buffer_size] = '\0';

    return str;
}

/**
 * Start a new attribute on current token
 */
static silk_html_attribute_t *start_new_attribute(silk_html_tokenizer_t *tok) {
    /* Allocate/expand attributes array */
    silk_html_attribute_t *attrs = silk_arena_alloc(
        tok->arena,
        sizeof(silk_html_attribute_t) * (tok->active_token->attribute_count + 1)
    );
    if (!attrs) return NULL;

    /* Copy existing attributes */
    if (tok->active_token->attributes && tok->active_token->attribute_count > 0) {
        memcpy(attrs, tok->active_token->attributes,
               sizeof(silk_html_attribute_t) * tok->active_token->attribute_count);
    }

    /* Initialize new attribute */
    silk_html_attribute_t *new_attr = &attrs[tok->active_token->attribute_count];
    memset(new_attr, 0, sizeof(silk_html_attribute_t));

    tok->active_token->attributes = attrs;
    tok->active_token->attribute_count++;

    return new_attr;
}

/* ============================================================================
 * State Machine Implementation
 * ============================================================================ */

/**
 * Data state (baseline state)
 *
 * This is the default state for parsing HTML content.
 * Most text content is emitted as character tokens from this state.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#data-state
 */
static void process_data_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '&':
            /* Character reference state */
            tok->return_state = HTML_TOK_DATA;
            tok->state = HTML_TOK_CHARACTER_REFERENCE;
            break;

        case '<':
            /* Tag open state */
            tok->state = HTML_TOK_TAG_OPEN;
            break;

        case 0:
            /* NULL character - parse error */
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, cp);
            break;

        case EOF_CODE_POINT:
            /* End of file */
            emit_eof(tok);
            break;

        default:
            /* Emit the character */
            emit_character(tok, cp);
            break;
    }
}

/**
 * Tag open state
 *
 * Entered when '<' is encountered in Data state.
 * Determines if this is a start tag, end tag, comment, or something else.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#tag-open-state
 */
static void process_tag_open_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '!':
            /* Markup declaration open state */
            tok->state = HTML_TOK_MARKUP_DECLARATION_OPEN;
            break;

        case '/':
            /* End tag open state */
            tok->state = HTML_TOK_END_TAG_OPEN;
            break;

        case '?':
            /* Parse error: unexpected-question-mark-instead-of-tag-name */
            emit_error(tok, "unexpected-question-mark-instead-of-tag-name");
            /* Create a comment token and treat as bogus comment */
            tok->active_token = create_token(tok, HTML_TOKEN_COMMENT);
            tok->state = HTML_TOK_BOGUS_COMMENT;
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-before-tag-name */
            emit_error(tok, "eof-before-tag-name");
            emit_character(tok, '<');
            emit_eof(tok);
            break;

        default:
            if (silk_html_is_alpha(cp)) {
                /* Start tag */
                tok->active_token = create_token(tok, HTML_TOKEN_START_TAG);
                clear_temp_buffer(tok);
                tok->state = HTML_TOK_TAG_NAME;
                /* Reconsume in tag name state */
                tok->reconsume = true;
            } else {
                /* Parse error: invalid-first-character-of-tag-name */
                emit_error(tok, "invalid-first-character-of-tag-name");
                emit_character(tok, '<');
                tok->state = HTML_TOK_DATA;
                /* Reconsume in data state */
                tok->reconsume = true;
            }
            break;
    }
}

/**
 * End tag open state
 *
 * Entered when '</' is encountered. Determines if this is a valid end tag.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#end-tag-open-state
 */
static void process_end_tag_open_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_alpha(cp)) {
        /* Start of end tag name */
        tok->active_token = create_token(tok, HTML_TOKEN_END_TAG);
        clear_temp_buffer(tok);
        tok->state = HTML_TOK_TAG_NAME;
        /* Reconsume in tag name state */
        tok->reconsume = true;
    } else if (cp == '>') {
        /* Parse error: missing-end-tag-name */
        emit_error(tok, "missing-end-tag-name");
        tok->state = HTML_TOK_DATA;
    } else if (cp == EOF_CODE_POINT) {
        /* Parse error: eof-before-tag-name */
        emit_error(tok, "eof-before-tag-name");
        emit_character(tok, '<');
        emit_character(tok, '/');
        tok->state = HTML_TOK_DATA;
        tok->reconsume = true;
    } else {
        /* Parse error: invalid-first-character-of-tag-name */
        emit_error(tok, "invalid-first-character-of-tag-name");
        tok->active_token = create_token(tok, HTML_TOKEN_COMMENT);
        clear_temp_buffer(tok);
        tok->state = HTML_TOK_BOGUS_COMMENT;
        /* Reconsume in bogus comment state */
        tok->reconsume = true;
    }
}


static void emit_active_token(silk_html_tokenizer_t *tok) {
    if (!tok->active_token) return;
    
    /* Finalize name if needed? 
       Actually, most states already call copy_temp_buffer before calling this. */
    
    emit_token(tok, tok->active_token);
    tok->active_token = NULL;
}

/**
 * Tag name state
 *
 * Accumulates tag name characters until end of tag.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#tag-name-state
 */
static void process_tag_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Whitespace - switch to before attribute name */
            temp_buffer_to_lowercase(tok);
            tok->active_token->tag_name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            break;

        case '/':
            /* Self-closing start tag */
            temp_buffer_to_lowercase(tok);
            tok->active_token->tag_name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_SELF_CLOSING_START_TAG;
            break;

        case '>':
            /* End of tag - emit token */
            temp_buffer_to_lowercase(tok);
            tok->active_token->tag_name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;

        case 0:
            /* Parse error: unexpected-null-character */
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-in-tag */
            emit_error(tok, "eof-in-tag");
            temp_buffer_to_lowercase(tok);
            tok->active_token->tag_name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;

        default:
            /* Append to tag name */
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * Before attribute name state
 *
 * Looking for start of attribute name, or end of tag.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#before-attribute-name-state
 */
static void process_before_attribute_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Ignore whitespace */
            break;

        case '/':
        case '>':
        case EOF_CODE_POINT:
            /* End of attributes - reconsume in after attribute name state */
            tok->state = HTML_TOK_AFTER_ATTRIBUTE_NAME;
            tok->reconsume = true;
            break;

        case '=':
            /* Parse error: unexpected-equals-sign-before-attribute-name */
            emit_error(tok, "unexpected-equals-sign-before-attribute-name");
            /* Start new attribute with '=' as name */
            start_new_attribute(tok);
            clear_temp_buffer(tok);
            append_to_temp_buffer(tok, cp);
            tok->state = HTML_TOK_ATTRIBUTE_NAME;
            break;

        default:
            /* Start new attribute */
            start_new_attribute(tok);
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_ATTRIBUTE_NAME;
            /* Reconsume in attribute name state */
            tok->reconsume = true;
            break;
    }
}

/**
 * Attribute name state
 *
 * Accumulates attribute name characters.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#attribute-name-state
 */
static void process_attribute_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
        case '/':
        case '>':
        case EOF_CODE_POINT:
            /* End of attribute name */
            temp_buffer_to_lowercase(tok);
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->name = copy_temp_buffer(tok);
                attr->name_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_AFTER_ATTRIBUTE_NAME;
            /* Reconsume */
            tok->reconsume = true;
            break;

        case '=':
            /* Equals - switch to before attribute value */
            temp_buffer_to_lowercase(tok);
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->name = copy_temp_buffer(tok);
                attr->name_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_VALUE;
            break;

        case 0:
            /* Parse error: unexpected-null-character */
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        case '"':
        case '\'':
        case '<':
            /* Parse error: unexpected-character-in-attribute-name */
            emit_error(tok, "unexpected-character-in-attribute-name");
            /* Fall through to append anyway */
            __attribute__((fallthrough));

        default:
            /* Append to attribute name */
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * After attribute name state
 *
 * After attribute name, looking for '=' or next attribute.
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#after-attribute-name-state
 */
static void process_after_attribute_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Ignore whitespace */
            break;

        case '/':
            /* Self-closing flag */
            tok->state = HTML_TOK_SELF_CLOSING_START_TAG;
            break;

        case '=':
            /* Attribute value coming */
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_VALUE;
            break;

        case '>':
            /* End of tag */
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            /* Token will be emitted */
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-in-tag */
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            /* Start new attribute */
            start_new_attribute(tok);
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_ATTRIBUTE_NAME;
            /* Reconsume */
            tok->reconsume = true;
            break;
    }
}

/**
 * Before attribute value state
 *
 * Looking for start of attribute value (quoted or unquoted).
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#before-attribute-value-state
 */
static void process_before_attribute_value_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Ignore whitespace */
            break;

        case '"':
            /* Double-quoted value */
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_ATTRIBUTE_VALUE_DOUBLE_QUOTED;
            break;

        case '\'':
            /* Single-quoted value */
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_ATTRIBUTE_VALUE_SINGLE_QUOTED;
            break;

        case '>':
            /* Parse error: missing-attribute-value */
            emit_error(tok, "missing-attribute-value");
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;

        case EOF_CODE_POINT:
            /* Parse error: missing-attribute-value, eof-in-tag */
            /* Spec says switch to Data and reconsume EOF, which will just emit EOF */
            /* But we also need to emit the current tag token first? */
            /* Spec says "Emit the current tag token. Switch to the data state. Reconsume the EOF character." */
            emit_error(tok, "missing-attribute-value");
            /* Force emit current tag by going to DATA (loop will see active_token and emit it) */
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;

        default:
            /* Unquoted value */
            clear_temp_buffer(tok);
            append_to_temp_buffer(tok, cp);
            tok->state = HTML_TOK_ATTRIBUTE_VALUE_UNQUOTED;
            break;
    }
}

/**
 * Attribute value (double-quoted) state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(double-quoted)-state
 */
static void process_attribute_value_double_quoted_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '"':
            /* End of value */
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->value = copy_temp_buffer(tok);
                attr->value_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_AFTER_ATTRIBUTE_VALUE_QUOTED;
            break;

        case '&':
            /* Character reference */
            tok->return_state = HTML_TOK_ATTRIBUTE_VALUE_DOUBLE_QUOTED;
            tok->state = HTML_TOK_CHARACTER_REFERENCE;
            break;

        case 0:
            /* Parse error: unexpected-null-character */
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-in-tag */
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * Attribute value (single-quoted) state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(single-quoted)-state
 */
static void process_attribute_value_single_quoted_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\'':
            /* End of value */
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->value = copy_temp_buffer(tok);
                attr->value_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_AFTER_ATTRIBUTE_VALUE_QUOTED;
            break;

        case '&':
            /* Character reference */
            tok->return_state = HTML_TOK_ATTRIBUTE_VALUE_SINGLE_QUOTED;
            tok->state = HTML_TOK_CHARACTER_REFERENCE;
            break;

        case 0:
            /* Parse error: unexpected-null-character */
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-in-tag */
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * Attribute value (unquoted) state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(unquoted)-state
 */
static void process_attribute_value_unquoted_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* End of value */
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->value = copy_temp_buffer(tok);
                attr->value_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            break;

        case '&':
            /* Character reference */
            tok->return_state = HTML_TOK_ATTRIBUTE_VALUE_UNQUOTED;
            tok->state = HTML_TOK_CHARACTER_REFERENCE;
            break;

        case '>':
            /* End of tag */
            if (tok->active_token->attribute_count > 0) {
                silk_html_attribute_t *attr =
                    &tok->active_token->attributes[tok->active_token->attribute_count - 1];
                attr->value = copy_temp_buffer(tok);
                attr->value_len = tok->temp_buffer_size;
            }
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;

        case 0:
            /* Parse error: unexpected-null-character */
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        case '"':
        case '\'':
        case '<':
        case '=':
        case '`':
            /* Parse error: unexpected-character-in-unquoted-attribute-value */
            emit_error(tok, "unexpected-character-in-unquoted-attribute-value");
            append_to_temp_buffer(tok, cp);
            break;

        case EOF_CODE_POINT:
            /* Parse error: eof-in-tag */
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * After attribute value (quoted) state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#after-attribute-value-(quoted)-state
 */
static void process_after_attribute_value_quoted_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            break;

        case '/':
            tok->state = HTML_TOK_SELF_CLOSING_START_TAG;
            break;

        case '>':
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;

        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            emit_error(tok, "missing-whitespace-between-attributes");
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            tok->reconsume = true;
            break;
    }
}

/**
 * Self-closing start tag state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#self-closing-start-tag-state
 */
static void process_self_closing_start_tag_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
            tok->active_token->self_closing = true;
            tok->state = HTML_TOK_DATA;
            break;

        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-tag");
            emit_eof(tok);
            break;

        default:
            emit_error(tok, "unexpected-solidus-in-tag");
            tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            tok->reconsume = true;
            break;
    }
}

static void finalize_comment(silk_html_tokenizer_t *tok) {
    if (tok->active_token && tok->active_token->type == HTML_TOKEN_COMMENT) {
        tok->active_token->comment_data = copy_temp_buffer(tok);
        tok->active_token->comment_len = tok->temp_buffer_size;
        emit_token(tok, tok->active_token);
        tok->active_token = NULL;
    }
    tok->state = HTML_TOK_DATA;
}

/**
 * Bogus comment state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#bogus-comment-state
 */
static void process_bogus_comment_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
            finalize_comment(tok);
            break;

        case EOF_CODE_POINT:
            finalize_comment(tok);
            tok->reconsume = true;
            break;

        case 0:
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;

        default:
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * Markup declaration open state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#markup-declaration-open-state
 */
static void process_markup_declaration_open_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    /* Need to check for --, DOCTYPE, or [CDATA[ */
    /* We use peek for this */

    if (cp == '-' && silk_html_input_stream_peek(tok->stream, 0) == '-') {
        /* Consume the second '-' */
        silk_html_input_stream_next(tok->stream);
        tok->active_token = create_token(tok, HTML_TOKEN_COMMENT);
        clear_temp_buffer(tok);
        tok->state = HTML_TOK_COMMENT_START;
    } else {
        /* Check for DOCTYPE (case-insensitive) */
        bool is_doctype = true;
        const char *doctype_match = "DOCTYPE";
        /* cp is the first 'D' (maybe) */
        if (silk_html_to_lower(cp) == 'd') {
            for (int i = 0; i < 6; i++) {
                uint32_t peeked = silk_html_input_stream_peek(tok->stream, i);
                if (silk_html_to_lower(peeked) != silk_html_to_lower(doctype_match[i+1])) {
                    is_doctype = false;
                    break;
                }
            }
        } else {
            is_doctype = false;
        }

        if (is_doctype) {
            /* Consume remaining "OCTYPE" */
            for (int i = 0; i < 6; i++) silk_html_input_stream_next(tok->stream);
            tok->state = HTML_TOK_DOCTYPE;
        } else {
            /* Check for [CDATA[ (case-sensitive) */
            /* TODO: Only if in foreign content. For now, assume not. */
            
            /* Otherwise: bogus comment */
            emit_error(tok, "incorrectly-opened-comment");
            tok->active_token = create_token(tok, HTML_TOKEN_COMMENT);
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_BOGUS_COMMENT;
            tok->reconsume = true;
        }
    }
}

/**
 * Comment start state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-start-state
 */
static void process_comment_start_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            tok->state = HTML_TOK_COMMENT_START_DASH;
            break;
        case '>':
            emit_error(tok, "abrupt-closing-of-empty-comment");
            finalize_comment(tok);
            break;
        default:
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment start dash state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-start-dash-state
 */
static void process_comment_start_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            tok->state = HTML_TOK_COMMENT_END;
            break;
        case '>':
            emit_error(tok, "abrupt-closing-of-empty-comment");
            finalize_comment(tok);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-comment");
            finalize_comment(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, '-');
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-state
 */
static void process_comment_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '<':
            append_to_temp_buffer(tok, cp);
            tok->state = HTML_TOK_COMMENT_LESS_THAN_SIGN;
            break;
        case '-':
            tok->state = HTML_TOK_COMMENT_END_DASH;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-comment");
            finalize_comment(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, cp);
            break;
    }
}

/**
 * Comment end dash state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-end-dash-state
 */
static void process_comment_end_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            tok->state = HTML_TOK_COMMENT_END;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-comment");
            finalize_comment(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, '-');
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment end state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-end-state
 */
static void process_comment_end_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
            finalize_comment(tok);
            break;
        case '!':
            tok->state = HTML_TOK_COMMENT_END_BANG;
            break;
        case '-':
            append_to_temp_buffer(tok, '-');
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-comment");
            finalize_comment(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, '-');
            append_to_temp_buffer(tok, '-');
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment end bang state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#comment-end-bang-state
 */
static void process_comment_end_bang_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            append_to_temp_buffer(tok, '-');
            append_to_temp_buffer(tok, '-');
            append_to_temp_buffer(tok, '!');
            tok->state = HTML_TOK_COMMENT_END_DASH;
            break;
        case '>':
            emit_error(tok, "incorrectly-closed-comment");
            finalize_comment(tok);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-comment");
            finalize_comment(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, '-');
            append_to_temp_buffer(tok, '-');
            append_to_temp_buffer(tok, '!');
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment less-than sign state
 */
static void process_comment_less_than_sign_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '!':
            append_to_temp_buffer(tok, cp);
            tok->state = HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG;
            break;
        case '<':
            append_to_temp_buffer(tok, cp);
            break;
        default:
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment less-than sign bang state
 */
static void process_comment_less_than_sign_bang_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            tok->state = HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH;
            break;
        default:
            tok->state = HTML_TOK_COMMENT;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment less-than sign bang dash state
 */
static void process_comment_less_than_sign_bang_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            tok->state = HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH;
            break;
        default:
            tok->state = HTML_TOK_COMMENT_END_DASH;
            tok->reconsume = true;
            break;
    }
}

/**
 * Comment less-than sign bang dash dash state
 */
static void process_comment_less_than_sign_bang_dash_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
        case EOF_CODE_POINT:
            tok->state = HTML_TOK_COMMENT_END;
            tok->reconsume = true;
            break;
        default:
            emit_error(tok, "nested-comment");
            tok->state = HTML_TOK_COMMENT_END;
            tok->reconsume = true;
            break;
    }
}

static silk_html_doctype_data_t *create_doctype_data(silk_arena_t *arena) {
    silk_html_doctype_data_t *data = silk_arena_alloc(arena, sizeof(silk_html_doctype_data_t));
    if (data) {
        memset(data, 0, sizeof(silk_html_doctype_data_t));
        data->missing_name = true;
        data->missing_public_identifier = true;
        data->missing_system_identifier = true;
    }
    return data;
}

/**
 * DOCTYPE state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#doctype-state
 */
static void process_doctype_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            tok->state = HTML_TOK_BEFORE_DOCTYPE_NAME;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-doctype");
            tok->active_token = create_token(tok, HTML_TOKEN_DOCTYPE);
            if (tok->active_token) {
                tok->active_token->doctype_data = create_doctype_data(tok->arena);
                if (tok->active_token->doctype_data) {
                    tok->active_token->doctype_data->force_quirks = true;
                }
            }
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;
        default:
            emit_error(tok, "missing-whitespace-before-doctype-name");
            tok->state = HTML_TOK_BEFORE_DOCTYPE_NAME;
            tok->reconsume = true;
            break;
    }
}

/**
 * Before DOCTYPE name state
 *
 * Spec: https://html.spec.whatwg.org/multipage/parsing.html#before-doctype-name-state
 */
static void process_before_doctype_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Ignore whitespace */
            break;
        case '>':
            emit_error(tok, "missing-doctype-name");
            tok->active_token = create_token(tok, HTML_TOKEN_DOCTYPE);
            if (tok->active_token) {
                tok->active_token->doctype_data = create_doctype_data(tok->arena);
                if (tok->active_token->doctype_data) {
                    tok->active_token->doctype_data->force_quirks = true;
                }
            }
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            tok->active_token = create_token(tok, HTML_TOKEN_DOCTYPE);
            if (tok->active_token) {
                tok->active_token->doctype_data = create_doctype_data(tok->arena);
                if (tok->active_token->doctype_data) {
                    tok->active_token->doctype_data->name = silk_arena_alloc(tok->arena, 4);
                    if (tok->active_token->doctype_data->name) {
                        tok->active_token->doctype_data->name[0] = '?'; /* Simplified replacement */
                        tok->active_token->doctype_data->name[1] = '\0';
                    }
                    tok->active_token->doctype_data->missing_name = false;
                }
            }
            tok->state = HTML_TOK_DOCTYPE_NAME;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-doctype");
            tok->active_token = create_token(tok, HTML_TOKEN_DOCTYPE);
            if (tok->active_token) {
                tok->active_token->doctype_data = create_doctype_data(tok->arena);
                if (tok->active_token->doctype_data) {
                    tok->active_token->doctype_data->force_quirks = true;
                }
            }
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;
        default:
            tok->active_token = create_token(tok, HTML_TOKEN_DOCTYPE);
            if (tok->active_token) {
                tok->active_token->doctype_data = create_doctype_data(tok->arena);
                if (tok->active_token->doctype_data) {
                    clear_temp_buffer(tok);
                    append_to_temp_buffer(tok, silk_html_to_lower(cp));
                    tok->active_token->doctype_data->missing_name = false;
                }
            }
            tok->state = HTML_TOK_DOCTYPE_NAME;
            break;
    }
}

/**
 * DOCTYPE name state
 */
static void process_doctype_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            tok->active_token->doctype_data->name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_AFTER_DOCTYPE_NAME;
            break;
        case '>':
            tok->active_token->doctype_data->name = copy_temp_buffer(tok);
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            append_to_temp_buffer(tok, REPLACEMENT_CHARACTER);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-doctype");
            tok->active_token->doctype_data->name = copy_temp_buffer(tok);
            tok->active_token->doctype_data->force_quirks = true;
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;
        default:
            append_to_temp_buffer(tok, silk_html_to_lower(cp));
            break;
    }
}

/**
 * After DOCTYPE name state
 */
static void process_after_doctype_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            /* Ignore whitespace */
            break;
        case '>':
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-doctype");
            tok->active_token->doctype_data->force_quirks = true;
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;
        default:
            /* TODO: Handle PUBLIC/SYSTEM keywords */
            tok->active_token->doctype_data->force_quirks = true;
            tok->state = HTML_TOK_BOGUS_DOCTYPE;
            break;
    }
}

/**
 * Bogus DOCTYPE state
 */
static void process_bogus_doctype_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;
        case EOF_CODE_POINT:
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            tok->reconsume = true;
            break;
        default:
            /* Ignore */
            break;
    }
}

/**
 * Flush characters to the appropriate destination (either current attribute or as character tokens)
 */
static void flush_char_ref(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (tok->return_state == HTML_TOK_DATA || 
        tok->return_state == HTML_TOK_RCDATA || 
        tok->return_state == HTML_TOK_RAWTEXT || 
        tok->return_state == HTML_TOK_SCRIPT_DATA) 
    {
        emit_character(tok, cp);
    } else {
        append_to_temp_buffer(tok, cp);
    }
}

/**
 * Character reference state
 */
static void process_character_reference_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_whitespace(cp) || cp == '<' || cp == '&' || cp == EOF_CODE_POINT ||
        (tok->return_state != HTML_TOK_DATA && cp == '=')) 
    {
        /* Not a character reference */
        tok->state = tok->return_state;
        tok->reconsume = true;
        flush_char_ref(tok, '&');
    } else if (cp == '#') {
        tok->state = HTML_TOK_NUMERIC_CHARACTER_REFERENCE;
        /* Don't append '#' to temp buffer for numeric, we use char_ref_code */
        tok->char_ref_code = 0;
    } else {
        tok->state = HTML_TOK_NAMED_CHARACTER_REFERENCE;
        tok->reconsume = true;
        /* Keep '&' in temp buffer for named ref lookup if needed, 
           but spec says temp buffer is for the name itself. */
        clear_temp_buffer(tok);
    }
}

/**
 * Numeric character reference state
 */
static void process_numeric_character_reference_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == 'x' || cp == 'X') {
        tok->state = HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE_START;
    } else {
        tok->state = HTML_TOK_DECIMAL_CHARACTER_REFERENCE_START;
        tok->reconsume = true;
    }
}

/**
 * Hexadecimal character reference start state
 */
static void process_hexadecimal_character_reference_start_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_hex_digit(cp)) {
        tok->state = HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE;
        tok->reconsume = true;
    } else {
        emit_error(tok, "absence-of-digits-in-numeric-character-reference");
        tok->state = tok->return_state;
        tok->reconsume = true;
        flush_char_ref(tok, '&');
        flush_char_ref(tok, '#');
        flush_char_ref(tok, 'x');
    }
}

/**
 * Decimal character reference start state
 */
static void process_decimal_character_reference_start_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_digit(cp)) {
        tok->state = HTML_TOK_DECIMAL_CHARACTER_REFERENCE;
        tok->reconsume = true;
    } else {
        emit_error(tok, "absence-of-digits-in-numeric-character-reference");
        tok->state = tok->return_state;
        tok->reconsume = true;
        flush_char_ref(tok, '&');
        flush_char_ref(tok, '#');
    }
}

/**
 * Hexadecimal character reference state
 */
static void process_hexadecimal_character_reference_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_hex_digit(cp)) {
        uint32_t digit = 0;
        if (cp >= '0' && cp <= '9') digit = cp - '0';
        else if (cp >= 'A' && cp <= 'F') digit = cp - 'A' + 10;
        else if (cp >= 'a' && cp <= 'f') digit = cp - 'a' + 10;
        tok->char_ref_code = (tok->char_ref_code * 16) + digit;
    } else if (cp == ';') {
        tok->state = HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END;
    } else {
        emit_error(tok, "missing-semicolon-after-character-reference");
        tok->state = HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END;
        tok->reconsume = true;
    }
}

/**
 * Decimal character reference state
 */
static void process_decimal_character_reference_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_digit(cp)) {
        tok->char_ref_code = (tok->char_ref_code * 10) + (cp - '0');
    } else if (cp == ';') {
        tok->state = HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END;
    } else {
        emit_error(tok, "missing-semicolon-after-character-reference");
        tok->state = HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END;
        tok->reconsume = true;
    }
}

/**
 * Numeric character reference end state
 */
static void process_numeric_character_reference_end_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    (void)cp;
    uint32_t decoded = tok->char_ref_code;
    
    /* HTML5 Character Reference overrides */
    if (decoded == 0x00) decoded = REPLACEMENT_CHARACTER;
    else if (decoded > 0x10FFFF) decoded = REPLACEMENT_CHARACTER;
    else if (decoded >= 0xD800 && decoded <= 0xDFFF) decoded = REPLACEMENT_CHARACTER;
    /* TODO: Implement full 12.2.6.1 Character reference overrides table (0x80-0x9F) */
    
    flush_char_ref(tok, decoded);
    tok->char_ref_code = 0;
    tok->state = tok->return_state;
    tok->reconsume = true;
}

/**
 * Named character reference state
 */
static void process_named_character_reference_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    /* HTML5 Named Character Reference state is complex.
       We accumulate characters that COULD be part of a name. */
    
    if (silk_html_is_alpha(cp) || silk_html_is_digit(cp)) {
        append_to_temp_buffer(tok, cp);
    } else {
        /* Check if we have a match */
        uint32_t decoded[2];
        size_t consumed = 0;
        bool has_semicolon = (cp == ';');
        int count = silk_html_decode_named_char_ref(tok->temp_buffer, tok->temp_buffer_size, has_semicolon, decoded, &consumed);
        
        if (count > 0) {
            /* We have a match! */
            /* In attribute, if not followed by ;, and next char is =, not a match */
            bool is_match = true;
            if (tok->return_state != HTML_TOK_DATA && cp != ';') {
                if (cp == '=' || silk_html_is_alpha(cp) || silk_html_is_digit(cp)) {
                    is_match = false;
                }
            }
            
            if (is_match) {
                for (int i = 0; i < (int)count; i++) flush_char_ref(tok, decoded[i]);
                
                /* Flush any remaining characters in temp_buffer that weren't part of the name */
                for (size_t i = consumed; i < tok->temp_buffer_size; i++) {
                    flush_char_ref(tok, (uint8_t)tok->temp_buffer[i]);
                }

                if (cp == ';') {
                    tok->state = tok->return_state;
                } else {
                    tok->state = tok->return_state;
                    tok->reconsume = true;
                }
            } else {
                /* Not a match due to attribute rules */
                flush_char_ref(tok, '&');
                for (size_t i = 0; i < tok->temp_buffer_size; i++) flush_char_ref(tok, (uint8_t)tok->temp_buffer[i]);
                tok->state = tok->return_state;
                tok->reconsume = true;
            }
        } else {
            /* No match at all */
            flush_char_ref(tok, '&');
            for (size_t i = 0; i < tok->temp_buffer_size; i++) flush_char_ref(tok, (uint8_t)tok->temp_buffer[i]);
            tok->state = tok->return_state;
            tok->reconsume = true;
        }
    }
}


/**
 * Script data state
 */
static void process_script_data_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '<':
            tok->state = HTML_TOK_SCRIPT_DATA_LESS_THAN_SIGN;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            break;
        case EOF_CODE_POINT:
            emit_eof(tok);
            break;
        default:
            emit_character(tok, cp);
            break;
    }
}

/**
 * Script data less-than sign state
 */
static void process_script_data_less_than_sign_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '/':
            clear_temp_buffer(tok);
            tok->state = HTML_TOK_SCRIPT_DATA_END_TAG_OPEN;
            break;
        case '!':
            emit_character(tok, '<');
            emit_character(tok, '!');
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPE_START;
            break;
        default:
            emit_character(tok, '<');
            tok->state = HTML_TOK_SCRIPT_DATA;
            tok->reconsume = true;
            break;
    }
}

/**
 * Script data end tag open state
 */
static void process_script_data_end_tag_open_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_alpha(cp)) {
        tok->active_token = create_token(tok, HTML_TOKEN_END_TAG);
        tok->state = HTML_TOK_SCRIPT_DATA_END_TAG_NAME;
        tok->reconsume = true;
    } else {
        emit_character(tok, '<');
        emit_character(tok, '/');
        tok->state = HTML_TOK_SCRIPT_DATA;
        tok->reconsume = true;
    }
}

static bool is_appropriate_end_tag(silk_html_tokenizer_t *tok) {
    if (!tok->last_start_tag_name || !tok->active_token || !tok->active_token->tag_name) return false;
    return strcmp(tok->last_start_tag_name, tok->active_token->tag_name) == 0;
}

/**
 * Script data end tag name state
 */
static void process_script_data_end_tag_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            if (is_appropriate_end_tag(tok)) tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            else goto script_not_appropriate;
            break;
        case '/':
            if (is_appropriate_end_tag(tok)) tok->state = HTML_TOK_SELF_CLOSING_START_TAG;
            else goto script_not_appropriate;
            break;
        case '>':
            if (is_appropriate_end_tag(tok)) {
                tok->active_token->tag_name = copy_temp_buffer(tok);
                tok->state = HTML_TOK_DATA;
                emit_active_token(tok);
            } else goto script_not_appropriate;
            break;
        default:
            if (silk_html_is_alpha(cp)) {
                append_to_temp_buffer(tok, silk_html_to_lower(cp));
            } else {
            script_not_appropriate:
                emit_character(tok, '<');
                emit_character(tok, '/');
                for (size_t i = 0; i < tok->temp_buffer_size; i++) emit_character(tok, (uint8_t)tok->temp_buffer[i]);
                tok->state = HTML_TOK_SCRIPT_DATA;
                tok->reconsume = true;
            }
            break;
    }
}

/**
 * Script data escape start state
 */
static void process_script_data_escape_start_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == '-') {
        emit_character(tok, '-');
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPE_START_DASH;
    } else {
        tok->state = HTML_TOK_SCRIPT_DATA;
        tok->reconsume = true;
    }
}

/**
 * Script data escape start dash state
 */
static void process_script_data_escape_start_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == '-') {
        emit_character(tok, '-');
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_DASH_DASH;
    } else {
        tok->state = HTML_TOK_SCRIPT_DATA;
        tok->reconsume = true;
    }
}

/**
 * Script data escaped state
 */
static void process_script_data_escaped_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_DASH;
            break;
        case '<':
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            break;
    }
}

/**
 * Script data escaped dash state
 */
static void process_script_data_escaped_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_DASH_DASH;
            break;
        case '<':
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            break;
    }
}

/**
 * Script data escaped dash dash state
 */
static void process_script_data_escaped_dash_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            break;
        case '<':
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN;
            break;
        case '>':
            emit_character(tok, '>');
            tok->state = HTML_TOK_SCRIPT_DATA;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            break;
    }
}

/**
 * Script data escaped less-than sign state
 */
static void process_script_data_escaped_less_than_sign_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == '/') {
        clear_temp_buffer(tok);
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_OPEN;
    } else if (silk_html_is_alpha(cp)) {
        clear_temp_buffer(tok);
        emit_character(tok, '<');
        tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_START;
        tok->reconsume = true;
    } else {
        emit_character(tok, '<');
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
        tok->reconsume = true;
    }
}

/**
 * Script data escaped end tag open state
 */
static void process_script_data_escaped_end_tag_open_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (silk_html_is_alpha(cp)) {
        tok->active_token = create_token(tok, HTML_TOKEN_END_TAG);
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_NAME;
        tok->reconsume = true;
    } else {
        emit_character(tok, '<');
        emit_character(tok, '/');
        tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
        tok->reconsume = true;
    }
}

/**
 * Script data escaped end tag name state
 */
static void process_script_data_escaped_end_tag_name_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
            if (is_appropriate_end_tag(tok)) tok->state = HTML_TOK_BEFORE_ATTRIBUTE_NAME;
            else goto escaped_not_appropriate;
            break;
        case '/':
            if (is_appropriate_end_tag(tok)) tok->state = HTML_TOK_SELF_CLOSING_START_TAG;
            else goto escaped_not_appropriate;
            break;
        case '>':
            if (is_appropriate_end_tag(tok)) {
                tok->active_token->tag_name = copy_temp_buffer(tok);
                tok->state = HTML_TOK_DATA;
                emit_active_token(tok);
            } else goto escaped_not_appropriate;
            break;
        default:
            if (silk_html_is_alpha(cp)) {
                append_to_temp_buffer(tok, silk_html_to_lower(cp));
            } else {
            escaped_not_appropriate:
                emit_character(tok, '<');
                emit_character(tok, '/');
                for (size_t i = 0; i < tok->temp_buffer_size; i++) emit_character(tok, (uint8_t)tok->temp_buffer[i]);
                tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
                tok->reconsume = true;
            }
            break;
    }
}

/**
 * Script data double escape start state
 */
static void process_script_data_double_escape_start_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
        case '/':
        case '>':
            emit_character(tok, cp);
            if (strcmp(tok->temp_buffer, "script") == 0) tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            else tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            break;
        default:
            if (silk_html_is_alpha(cp)) {
                append_to_temp_buffer(tok, silk_html_to_lower(cp));
                emit_character(tok, cp);
            } else {
                tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
                tok->reconsume = true;
            }
            break;
    }
}

/**
 * Script data double escaped state
 */
static void process_script_data_double_escaped_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH;
            break;
        case '<':
            emit_character(tok, '<');
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            break;
    }
}

/**
 * Script data double escaped dash state
 */
static void process_script_data_double_escaped_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH;
            break;
        case '<':
            emit_character(tok, '<');
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            break;
    }
}

/**
 * Script data double escaped dash dash state
 */
static void process_script_data_double_escaped_dash_dash_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '-':
            emit_character(tok, '-');
            break;
        case '<':
            emit_character(tok, '<');
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN;
            break;
        case '>':
            emit_character(tok, '>');
            tok->state = HTML_TOK_SCRIPT_DATA;
            break;
        case 0:
            emit_error(tok, "unexpected-null-character");
            emit_character(tok, REPLACEMENT_CHARACTER);
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            break;
        case EOF_CODE_POINT:
            emit_error(tok, "eof-in-script-html-comment-like-text");
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            break;
    }
}

/**
 * Script data double escaped less-than sign state
 */
static void process_script_data_double_escaped_less_than_sign_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == '/') {
        emit_character(tok, '/');
        clear_temp_buffer(tok);
        tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_END;
    } else {
        tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
        tok->reconsume = true;
    }
}

/**
 * Script data double escape end state
 */
static void process_script_data_double_escape_end_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '\t':
        case '\n':
        case '\f':
        case ' ':
        case '/':
        case '>':
            emit_character(tok, cp);
            if (strcmp(tok->temp_buffer, "script") == 0) tok->state = HTML_TOK_SCRIPT_DATA_ESCAPED;
            else tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
            break;
        default:
            if (silk_html_is_alpha(cp)) {
                append_to_temp_buffer(tok, silk_html_to_lower(cp));
                emit_character(tok, cp);
            } else {
                tok->state = HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED;
                tok->reconsume = true;
            }
            break;
    }
}

/**
 * CDATA section state
 */
static void process_cdata_section_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case ']':
            tok->state = HTML_TOK_CDATA_SECTION_BRACKET;
            break;
        case EOF_CODE_POINT:
            tok->state = HTML_TOK_DATA;
            tok->reconsume = true;
            break;
        default:
            emit_character(tok, cp);
            break;
    }
}

/**
 * CDATA section bracket state
 */
static void process_cdata_section_bracket_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    if (cp == ']') {
        tok->state = HTML_TOK_CDATA_SECTION_END;
    } else {
        emit_character(tok, ']');
        tok->state = HTML_TOK_CDATA_SECTION;
        tok->reconsume = true;
    }
}

/**
 * CDATA section end state
 */
static void process_cdata_section_end_state(silk_html_tokenizer_t *tok, uint32_t cp) {
    switch (cp) {
        case '>':
            tok->state = HTML_TOK_DATA;
            emit_active_token(tok);
            break;
        case ']':
            emit_character(tok, ']');
            break;
        default:
            emit_character(tok, ']');
            emit_character(tok, ']');
            tok->state = HTML_TOK_CDATA_SECTION;
            tok->reconsume = true;
            break;
    }
}

/**
 * Main tokenizer loop
 *
 * Runs the state machine until a token is emitted.
 */
silk_html_token_t *silk_html_tokenizer_next_token(silk_html_tokenizer_t *tokenizer) {
    if (!tokenizer) {
        return NULL;
    }

    /* Return pending token if we have one */
    if (tokenizer->pending_token) {
        silk_html_token_t *token = tokenizer->pending_token;
        tokenizer->pending_token = NULL;
        return token;
    }

    /* Clear emitted token */
    tokenizer->emitted_token = NULL;

    /* Run state machine until we emit a token */
    int iterations = 0;
    while (!tokenizer->emitted_token) {
        if (++iterations > 10000) {
            fprintf(stderr, "ERROR: Infinite loop detected in tokenizer (state=%d)\n", tokenizer->state);
            return NULL;
        }
        uint32_t cp;

        /* Check if we should reconsume the current character */
        if (tokenizer->reconsume) {
            cp = tokenizer->current_char;
            tokenizer->reconsume = false;
        } else {
            /* Get next code point */
            cp = silk_html_input_stream_next(tokenizer->stream);
            tokenizer->current_char = cp;
        }

        /* Check for EOF in DATA state (if no active token) */
        if (cp == EOF_CODE_POINT && tokenizer->state == HTML_TOK_DATA && !tokenizer->active_token) {
            if (tokenizer->char_buffer_size > 0) {
                tokenizer->emitted_token = flush_char_buffer(tokenizer);
            } else {
                emit_eof(tokenizer);
            }
            break;
        }

        /* Dispatch to state handler */
        switch (tokenizer->state) {
            case HTML_TOK_DATA:
                process_data_state(tokenizer, cp);
                break;

            case HTML_TOK_TAG_OPEN:
                process_tag_open_state(tokenizer, cp);
                break;

            case HTML_TOK_END_TAG_OPEN:
                process_end_tag_open_state(tokenizer, cp);
                break;

            case HTML_TOK_TAG_NAME:
                process_tag_name_state(tokenizer, cp);
                break;

            case HTML_TOK_BEFORE_ATTRIBUTE_NAME:
                process_before_attribute_name_state(tokenizer, cp);
                break;

            case HTML_TOK_ATTRIBUTE_NAME:
                process_attribute_name_state(tokenizer, cp);
                break;

            case HTML_TOK_AFTER_ATTRIBUTE_NAME:
                process_after_attribute_name_state(tokenizer, cp);
                break;

            case HTML_TOK_BEFORE_ATTRIBUTE_VALUE:
                process_before_attribute_value_state(tokenizer, cp);
                break;

            case HTML_TOK_ATTRIBUTE_VALUE_DOUBLE_QUOTED:
                process_attribute_value_double_quoted_state(tokenizer, cp);
                break;

            case HTML_TOK_ATTRIBUTE_VALUE_SINGLE_QUOTED:
                process_attribute_value_single_quoted_state(tokenizer, cp);
                break;

            case HTML_TOK_ATTRIBUTE_VALUE_UNQUOTED:
                process_attribute_value_unquoted_state(tokenizer, cp);
                break;

            case HTML_TOK_AFTER_ATTRIBUTE_VALUE_QUOTED:
                process_after_attribute_value_quoted_state(tokenizer, cp);
                break;

            case HTML_TOK_SELF_CLOSING_START_TAG:
                process_self_closing_start_tag_state(tokenizer, cp);
                break;

            case HTML_TOK_MARKUP_DECLARATION_OPEN:
                process_markup_declaration_open_state(tokenizer, cp);
                break;

            case HTML_TOK_BOGUS_COMMENT:
                process_bogus_comment_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_START:
                process_comment_start_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_START_DASH:
                process_comment_start_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT:
                process_comment_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_LESS_THAN_SIGN:
                process_comment_less_than_sign_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG:
                process_comment_less_than_sign_bang_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH:
                process_comment_less_than_sign_bang_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH:
                process_comment_less_than_sign_bang_dash_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_END_DASH:
                process_comment_end_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_END:
                process_comment_end_state(tokenizer, cp);
                break;

            case HTML_TOK_COMMENT_END_BANG:
                process_comment_end_bang_state(tokenizer, cp);
                break;

            case HTML_TOK_DOCTYPE:
                process_doctype_state(tokenizer, cp);
                break;

            case HTML_TOK_BEFORE_DOCTYPE_NAME:
                process_before_doctype_name_state(tokenizer, cp);
                break;

            case HTML_TOK_DOCTYPE_NAME:
                process_doctype_name_state(tokenizer, cp);
                break;

            case HTML_TOK_AFTER_DOCTYPE_NAME:
                process_after_doctype_name_state(tokenizer, cp);
                break;

            case HTML_TOK_BOGUS_DOCTYPE:
                process_bogus_doctype_state(tokenizer, cp);
                break;

            case HTML_TOK_CHARACTER_REFERENCE:
                process_character_reference_state(tokenizer, cp);
                break;

            case HTML_TOK_NAMED_CHARACTER_REFERENCE:
                process_named_character_reference_state(tokenizer, cp);
                break;

            case HTML_TOK_NUMERIC_CHARACTER_REFERENCE:
                process_numeric_character_reference_state(tokenizer, cp);
                break;

            case HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE_START:
                process_hexadecimal_character_reference_start_state(tokenizer, cp);
                break;

            case HTML_TOK_DECIMAL_CHARACTER_REFERENCE_START:
                process_decimal_character_reference_start_state(tokenizer, cp);
                break;

            case HTML_TOK_HEXADECIMAL_CHARACTER_REFERENCE:
                process_hexadecimal_character_reference_state(tokenizer, cp);
                break;

            case HTML_TOK_DECIMAL_CHARACTER_REFERENCE:
                process_decimal_character_reference_state(tokenizer, cp);
                break;

            case HTML_TOK_NUMERIC_CHARACTER_REFERENCE_END:
                process_numeric_character_reference_end_state(tokenizer, cp);
                break;

            case HTML_TOK_AMBIGUOUS_AMPERSAND:
                /* TODO: Handle ambiguous ampersand */
                tokenizer->state = tokenizer->return_state;
                break;

            case HTML_TOK_SCRIPT_DATA:
                process_script_data_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_LESS_THAN_SIGN:
                process_script_data_less_than_sign_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_END_TAG_OPEN:
                process_script_data_end_tag_open_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_END_TAG_NAME:
                process_script_data_end_tag_name_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPE_START:
                process_script_data_escape_start_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPE_START_DASH:
                process_script_data_escape_start_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED:
                process_script_data_escaped_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED_DASH:
                process_script_data_escaped_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED_DASH_DASH:
                process_script_data_escaped_dash_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN:
                process_script_data_escaped_less_than_sign_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_OPEN:
                process_script_data_escaped_end_tag_open_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_ESCAPED_END_TAG_NAME:
                process_script_data_escaped_end_tag_name_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_START:
                process_script_data_double_escape_start_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED:
                process_script_data_double_escaped_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH:
                process_script_data_double_escaped_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH:
                process_script_data_double_escaped_dash_dash_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN:
                process_script_data_double_escaped_less_than_sign_state(tokenizer, cp);
                break;

            case HTML_TOK_SCRIPT_DATA_DOUBLE_ESCAPE_END:
                process_script_data_double_escape_end_state(tokenizer, cp);
                break;

            case HTML_TOK_CDATA_SECTION:
                process_cdata_section_state(tokenizer, cp);
                break;

            case HTML_TOK_CDATA_SECTION_BRACKET:
                process_cdata_section_bracket_state(tokenizer, cp);
                break;

            case HTML_TOK_CDATA_SECTION_END:
                process_cdata_section_end_state(tokenizer, cp);
                break;

            /* TODO: Implement remaining states */

            default:
                /* Unimplemented state - error */
                emit_error(tokenizer, "unimplemented-state");
                tokenizer->state = HTML_TOK_DATA;
                break;
        }
    }

    /* Set end position */
    if (tokenizer->emitted_token) {
        silk_html_input_stream_get_position(
            tokenizer->stream,
            &tokenizer->emitted_token->end_line,
            &tokenizer->emitted_token->end_column
        );
    }

    return tokenizer->emitted_token;
}
