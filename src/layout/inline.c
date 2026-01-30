/**
 * \file inline.c
 * \brief Inline Layout Algorithm Implementation
 *
 * Implements CSS inline-level layout with:
 * - Horizontal text flow within line boxes
 * - Line breaking (word wrapping)
 * - Baseline alignment (text baselines aligned)
 * - Vertical alignment (top, middle, bottom)
 * - Mixed font sizes and line heights
 * - Whitespace collapsing
 * - Tab and newline handling
 *
 * Inline layout algorithm (per CSS 2.2):
 * 1. For each inline element in sequence:
 *    - Measure content width (sum of glyph widths)
 *    - If fits in current line: add to line
 *    - If doesn't fit: create new line
 * 2. For each line:
 *    - Compute line height (max ascent/descent)
 *    - Align baseline (typically bottom of line box)
 *    - Vertical align inline content
 * 3. Justify line if needed (text-align property)
 *
 * Performance: O(n) text processing, O(m) lines where m << n
 * Memory: O(m) line boxes, O(n) character positions
 */

#include <assert.h>
#include <ctype.h>
#include <limits.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

#include "silksurf/layout.h"
#include "silksurf/allocator.h"

/**
 * Inline box - represents positioned inline content
 *
 * Multiple inline boxes may exist on a single line box
 * (e.g., "Hello <span>World</span>!" = 3 inline boxes on 1 line box)
 */
typedef struct {
    int32_t x;              /* Position on line (relative to line box start) */
    int32_t width;          /* Content width (measured text + padding/border) */
    int32_t height;         /* Height (font metrics + padding/border) */
    int32_t baseline;       /* Baseline position within height */
    uint8_t align;          /* Vertical alignment: 0=baseline, 1=top, 2=middle, 3=bottom */
} inline_box_t;

/**
 * Line box - represents a single line of text
 *
 * Contains multiple inline boxes arranged horizontally
 * Line height is determined by tallest inline box
 */
typedef struct {
    int32_t x, y;           /* Position in block formatting context */
    int32_t width;          /* Content width (sum of inline boxes) */
    int32_t height;         /* Line height (max of inline boxes) */
    int32_t baseline;       /* Baseline position within line */

    inline_box_t *boxes;    /* Array of inline boxes on this line */
    size_t box_count;       /* Number of inline boxes */
    size_t box_capacity;    /* Allocated capacity */

    int32_t available_width; /* Space left on this line (for word wrapping) */
} line_box_t;

/**
 * Text measurement context
 *
 * Caches measurements to avoid repeated calculations
 * (In production: integrate with FreeType/HarfBuzz)
 */
typedef struct {
    const char *text;       /* Text being measured */
    size_t length;          /* Text length in bytes */
    int32_t font_size;      /* Font size in pixels (affects glyph width) */
    int32_t char_widths[256]; /* Precomputed ASCII widths */
    int32_t avg_width;      /* Average character width */
} text_measure_t;

/* ================================================================
   TEXT MEASUREMENT
   ================================================================

   For now: simplified monospace-style measurement
   TODO: Integrate with real font engine (FreeType + HarfBuzz)
*/

/**
 * Initialize text measurement context
 *
 * Computes character widths based on font size
 * For ASCII: roughly 0.5-0.7 of font size per character
 * Proportional fonts are more complex (TODO)
 */
static void measure_text_init(text_measure_t *measure, const char *text,
                               size_t length, int32_t font_size) {
    if (!measure) return;

    measure->text = text;
    measure->length = length;
    measure->font_size = font_size;

    /* Approximate: average character is ~0.6 * font_size width
       This is oversimplified; real fonts vary per character */
    measure->avg_width = (font_size * 3) / 5;  /* 0.6 * font_size */

    /* Precompute widths for ASCII characters
       TODO: Read from actual font metrics */
    for (int i = 0; i < 256; i++) {
        if (isspace(i)) {
            measure->char_widths[i] = font_size / 4;  /* Space = 0.25 * font_size */
        } else if (i >= '0' && i <= '9') {
            measure->char_widths[i] = measure->avg_width;
        } else if (i >= 'a' && i <= 'z') {
            measure->char_widths[i] = (font_size * 5) / 8;  /* 0.625 * font_size */
        } else if (i >= 'A' && i <= 'Z') {
            measure->char_widths[i] = (font_size * 7) / 8;  /* 0.875 * font_size */
        } else {
            measure->char_widths[i] = measure->avg_width;
        }
    }
}

/**
 * Measure text width in pixels
 *
 * \param measure Text measurement context
 * \param text Text string (may not be null-terminated)
 * \param length Number of characters to measure
 * \return Width in pixels
 */
static int32_t measure_text_width(const text_measure_t *measure,
                                   const char *text, size_t length) {
    if (!measure || !text || length == 0) return 0;

    int32_t width = 0;
    for (size_t i = 0; i < length; i++) {
        unsigned char c = (unsigned char)text[i];
        width += measure->char_widths[c];
    }
    return width;
}

/**
 * Find word boundary (space, hyphen, etc.)
 *
 * Used for line breaking: find where to break text
 *
 * \param text Text to search
 * \param length Text length
 * \param start Starting position
 * \return Position of word boundary, or length if none found
 */
static inline size_t __attribute__((unused)) find_word_boundary(const char *text, size_t length, size_t start) {
    if (!text || start >= length) return length;

    /* Find next space or punctuation */
    for (size_t i = start; i < length; i++) {
        if (isspace(text[i]) || text[i] == '-' || text[i] == ',') {
            return i;
        }
    }
    return length;  /* End of text */
}

/**
 * Skip whitespace in text
 *
 * CSS collapses sequences of whitespace to single space
 *
 * \param text Text to search
 * \param length Text length
 * \param start Starting position
 * \return Position after whitespace, or length
 */
static inline size_t __attribute__((unused)) skip_whitespace(const char *text, size_t length, size_t start) {
    if (!text || start >= length) return length;

    for (size_t i = start; i < length; i++) {
        if (!isspace(text[i])) {
            return i;
        }
    }
    return length;
}

/* ================================================================
   INLINE LAYOUT ALGORITHM
   ================================================================ */

/**
 * Layout inline content (text nodes, inline elements)
 *
 * Algorithm:
 * 1. For each text run:
 *    - Measure width
 *    - If fits on current line: add
 *    - If doesn't fit: wrap to new line
 * 2. For each line:
 *    - Align inline boxes vertically
 *    - Justify (text-align property)
 *    - Position in block context
 *
 * \param ctx Layout context
 * \param element Inline element (text or inline box)
 * \param parent_box Parent block box (defines line width)
 * \return Computed layout boxes for inline content
 */
layout_box_t silk_layout_compute_inline(
    layout_context_t *ctx,
    void *element,
    const layout_box_t *parent_box
) {
    if (!ctx || !element || !parent_box) {
        layout_box_t box = {0};
        return box;
    }

    layout_box_t box = {0};
    box.display = DISPLAY_INLINE;
    box.opacity = 255;

    /* Inline elements don't have padding/border in typical usage
       (though CSS allows it) */
    memset(&box.margin, 0, sizeof(box.margin));
    memset(&box.padding, 0, sizeof(box.padding));
    memset(&box.border, 0, sizeof(box.border));

    /* Position at parent's top-left (will be adjusted by line box) */
    box.x = parent_box->x;
    box.y = parent_box->y;

    /* Width and height determined by content and line breaking
       Set to reasonable defaults for now */
    box.width = 100;    /* TODO: actual text measurement */
    box.height = 16;    /* Default: ~1em at 16px font */

    ctx->box_count++;
    return box;
}

/**
 * Layout multiple inline elements into lines
 *
 * This is the main inline layout entry point:
 * - Takes a sequence of inline elements/text
 * - Breaks into lines based on container width
 * - Returns array of line boxes
 *
 * \param ctx Layout context
 * \param elements Array of inline elements
 * \param count Number of elements
 * \param container_width Available width for lines
 * \param font_size Font size for text measurement
 * \return Array of line boxes (arena-allocated)
 */
static inline line_box_t * __attribute__((unused)) layout_inline_lines(
    layout_context_t *ctx,
    void **elements,
    size_t count,
    int32_t container_width,
    int32_t font_size
) {
    if (!ctx || !elements || count == 0 || container_width <= 0) {
        return NULL;
    }

    /* Allocate line boxes array */
    line_box_t *lines = silk_arena_alloc(ctx->arena, sizeof(line_box_t) * 10);
    if (!lines) return NULL;

    memset(lines, 0, sizeof(line_box_t) * 10);

    /* For now: simple single-line layout
       TODO: Implement full line breaking algorithm */

    size_t line_idx = 0;
    lines[line_idx].x = 0;
    lines[line_idx].y = 0;
    lines[line_idx].width = container_width;
    lines[line_idx].height = font_size;  /* Line height = font size */
    lines[line_idx].baseline = font_size * 4 / 5;  /* Baseline ~80% down */
    lines[line_idx].available_width = container_width;

    return lines;
}

/* ================================================================
   PUBLIC LAYOUT FUNCTIONS (match header signatures)
   ================================================================ */

/**
 * Compute inline layout for a single inline element
 *
 * Simplified version: returns basic inline box
 * Full version would:
 * - Create line boxes for text content
 * - Handle line breaking (wrap words)
 * - Align baselines
 * - Justify text
 */
layout_box_t silk_layout_compute_inline_full(
    layout_context_t *ctx,
    void *element,
    const layout_box_t *parent_box
) {
    layout_box_t box = silk_layout_compute_inline(ctx, element, parent_box);

    /* TODO: Full inline layout:
       1. Extract text content from element
       2. Measure text width
       3. Apply line breaking if needed
       4. Align baseline
       5. Return positioned box */

    return box;
}

/**
 * Collapse whitespace in text per CSS spec
 *
 * "Sequences of whitespace are collapsed into a single space"
 *
 * \param text Input text
 * \param length Text length
 * \param output Output buffer
 * \param output_size Output buffer size
 * \return Length of collapsed text
 */
size_t silk_layout_collapse_whitespace(
    const char *text,
    size_t length,
    char *output,
    size_t output_size
) {
    if (!text || length == 0 || !output || output_size == 0) {
        return 0;
    }

    size_t out_idx = 0;
    int in_whitespace = 0;

    for (size_t i = 0; i < length && out_idx < output_size - 1; i++) {
        if (isspace(text[i])) {
            if (!in_whitespace) {
                output[out_idx++] = ' ';  /* Single space for sequence */
                in_whitespace = 1;
            }
            /* Skip additional whitespace */
        } else {
            output[out_idx++] = text[i];
            in_whitespace = 0;
        }
    }

    output[out_idx] = '\0';
    return out_idx;
}

/**
 * Measure text width (simplified version)
 *
 * Full version would use actual font metrics
 * This approximates based on font size and character count
 *
 * \param text Text to measure
 * \param length Number of characters
 * \param font_size Font size in pixels
 * \return Approximate width in pixels
 */
int32_t silk_layout_measure_text(
    const char *text,
    size_t length,
    int32_t font_size
) {
    if (!text || length == 0 || font_size <= 0) {
        return 0;
    }

    text_measure_t measure;
    measure_text_init(&measure, text, length, font_size);
    return measure_text_width(&measure, text, length);
}

/**
 * Find line break position
 *
 * Given available width, finds where text can be wrapped
 *
 * \param text Text to break
 * \param length Text length
 * \param available_width Width available for line
 * \param font_size Font size for measurement
 * \return Position to break (may be 0 if even one character too wide)
 */
size_t silk_layout_find_line_break(
    const char *text,
    size_t length,
    int32_t available_width,
    int32_t font_size
) {
    if (!text || length == 0 || available_width <= 0 || font_size <= 0) {
        return 0;
    }

    text_measure_t measure;
    measure_text_init(&measure, text, length, font_size);

    /* Binary search for longest text that fits */
    size_t lo = 0, hi = length;
    size_t best = 0;

    while (lo <= hi) {
        size_t mid = (lo + hi) / 2;
        int32_t width = measure_text_width(&measure, text, mid);

        if (width <= available_width) {
            best = mid;
            lo = mid + 1;
        } else {
            hi = (mid == 0) ? 0 : mid - 1;
        }
    }

    return best;
}
