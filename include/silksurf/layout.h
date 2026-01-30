/**
 * \file layout.h
 * \brief SilkSurf Layout Engine - Box Model and Constraint Resolution
 *
 * Implements CSS box model layout algorithm with support for:
 * - Block-level layout (vertical stacking, margin collapse)
 * - Inline layout (horizontal text flow)
 * - Replaced elements (images, video)
 * - Min/max width constraints
 * - Auto height calculation
 *
 * Performance targets:
 * - Layout single page: <100ms
 * - Incremental reflow: <16ms (60 FPS)
 * - Memory: <1MB per 1000 elements
 */

#ifndef SILKSURF_LAYOUT_H
#define SILKSURF_LAYOUT_H

#include <stddef.h>
#include <stdint.h>
#include "allocator.h"

/**
 * Display type affecting layout algorithm
 */
typedef enum {
    DISPLAY_NONE = 0,        /* Not rendered */
    DISPLAY_BLOCK,           /* Block-level (vertical stacking) */
    DISPLAY_INLINE,          /* Inline (horizontal text flow) */
    DISPLAY_INLINE_BLOCK,    /* Inline-level block container */
    DISPLAY_TABLE,           /* Table layout */
    DISPLAY_FLEX,            /* Flexible box layout */
    DISPLAY_GRID,            /* Grid layout */
} display_type_t;

/**
 * Edge values: margin, padding, border
 *
 * Units: CSS pixels (1px = 1/96 inch on standard screens)
 * Range: -2^30 to 2^30 (covers arbitrary layouts)
 */
typedef struct {
    int32_t left;
    int32_t top;
    int32_t right;
    int32_t bottom;
} edges_t;

/**
 * Computed layout box for an element
 *
 * Box model composition (outside to inside):
 *   margin → border → padding → content
 *
 * Layout coordinates: relative to viewport
 * Width/height include content only, not padding/border
 */
typedef struct {
    /* Position and size (CSS pixels) */
    int32_t x;                   /* Left edge (includes margin) */
    int32_t y;                   /* Top edge (includes margin) */
    int32_t width;               /* Content width */
    int32_t height;              /* Content height */

    /* Box model layers (CSS pixels) */
    edges_t margin;              /* Outermost: transparent spacing */
    edges_t border;              /* Border thickness */
    edges_t padding;             /* Inner spacing */

    /* Visual properties */
    display_type_t display;
    uint8_t opacity;             /* 0-255 (0=transparent, 255=opaque) */

    /* Computed values for constraints */
    int32_t min_width;           /* Minimum content width */
    int32_t max_width;           /* Maximum content width */
    int32_t min_height;
    int32_t max_height;

    /* Flags */
    uint8_t is_replaced;         /* True for img, video, etc. */
    uint8_t is_floated;          /* True if float: left/right */
    uint8_t is_positioned;       /* True if position: absolute/fixed */

    /* For internal use during layout */
    int32_t baseline;            /* For inline alignment */
    uint8_t collapsible_top;     /* Top margin can collapse */
    uint8_t collapsible_bottom;  /* Bottom margin can collapse */
} layout_box_t;

/**
 * Layout context for computing a layout tree
 *
 * Arena-allocated for fast cleanup at end of layout phase.
 */
typedef struct silk_layout_context {
    void *root_node;                /* Opaque DOM node pointer */
    int32_t viewport_width;
    int32_t viewport_height;
    struct silk_arena *arena;       /* Layout boxes allocated here */

    /* Statistics */
    size_t box_count;               /* Number of boxes computed */
    size_t reflow_count;            /* Number of incremental reflows */
} layout_context_t;

/**
 * Create layout context for a document
 *
 * \param root_node Root DOM node (typically document element)
 * \param viewport_width Viewport width in pixels
 * \param viewport_height Viewport height in pixels
 * \param arena Arena allocator for layout boxes
 * \return Initialized layout context, or NULL on failure
 */
layout_context_t *silk_layout_context_create(
    void *root_node,
    int32_t viewport_width,
    int32_t viewport_height,
    struct silk_arena *arena
);

/**
 * Compute layout for entire document tree
 *
 * Performs full layout pass on DOM tree:
 * 1. Traverse tree in document order
 * 2. Compute box for each visible element
 * 3. Resolve constraints and collapse margins
 * 4. Apply positioning (absolute, fixed)
 *
 * \param ctx Layout context
 * \return true on success, false on error (OOM, invalid constraint)
 */
bool silk_layout_compute(layout_context_t *ctx);

/**
 * Compute block-level layout for an element
 *
 * Block-level elements:
 * - Stack vertically within parent
 * - Width defaults to parent width (minus margins)
 * - Height defaults to auto (sum of children + margins)
 * - Adjacent vertical margins collapse
 *
 * \param ctx Layout context
 * \param element DOM element node
 * \param parent_box Parent's computed layout box
 * \return Computed layout box for element
 */
layout_box_t silk_layout_compute_block(
    layout_context_t *ctx,
    void *element,
    const layout_box_t *parent_box
);

/**
 * Compute inline layout for an element
 *
 * Inline elements:
 * - Flow horizontally within line box
 * - Width determined by content
 * - Height determined by font metrics
 * - Do not have meaningful margins/padding
 *
 * \param ctx Layout context
 * \param element DOM element node
 * \param parent_box Parent's computed layout box
 * \return Computed layout box for element
 */
layout_box_t silk_layout_compute_inline(
    layout_context_t *ctx,
    void *element,
    const layout_box_t *parent_box
);

/**
 * Compute layout for replaced elements (img, video, canvas)
 *
 * Replaced elements:
 * - Have intrinsic width/height (image dimensions)
 * - Do not have children to layout
 * - Aspect ratio preserved if only one dimension specified
 *
 * \param ctx Layout context
 * \param element DOM element node
 * \return Computed layout box for element
 */
layout_box_t silk_layout_compute_replaced(
    layout_context_t *ctx,
    void *element
);

/**
 * Resolve CSS width value to computed width
 *
 * Handles: px, %, em, rem, auto, min/max constraints
 *
 * \param element DOM element
 * \param container_width Parent's content width
 * \return Computed width in CSS pixels, 0 if auto
 */
int32_t silk_layout_resolve_width(
    void *element,
    int32_t container_width
);

/**
 * Resolve CSS height value to computed height
 *
 * Handles: px, %, em, rem, auto
 *
 * \param element DOM element
 * \param container_height Parent's content height
 * \return Computed height in CSS pixels, 0 if auto
 */
int32_t silk_layout_resolve_height(
    void *element,
    int32_t container_height
);

/**
 * Resolve CSS margin value to computed margin
 *
 * Handles: px, %, em, rem, auto
 *
 * \param element DOM element
 * \param property_name CSS property ("margin-left", "margin-top", etc.)
 * \param container_width Parent's content width (for % resolution)
 * \return Computed margin in CSS pixels
 */
int32_t silk_layout_resolve_margin(
    void *element,
    const char *property_name,
    int32_t container_width
);

/**
 * Collapse adjacent vertical margins (margin collapse algorithm)
 *
 * Adjacent block-level margins combine using max() instead of sum:
 * - Child's top margin collapses with parent's top margin
 * - Block's bottom margin collapses with next block's top margin
 * - Negative margins are allowed (pull elements up)
 *
 * \param margin1 First margin (can be negative)
 * \param margin2 Second margin (can be negative)
 * \return Collapsed margin (max of the two)
 */
int32_t silk_layout_collapse_margins(int32_t margin1, int32_t margin2);

/**
 * Collapse whitespace in text per CSS spec
 *
 * Reduces sequences of whitespace (spaces, tabs, newlines) to single space.
 * Per CSS 2.2: "Sequences of whitespace are collapsed into a single space"
 *
 * Examples:
 * - "  hello   world  " → " hello world "
 * - "hello\n\tworld" → "hello world"
 *
 * \param text Input text (may have multiple spaces/tabs/newlines)
 * \param length Input text length
 * \param output Output buffer for collapsed text
 * \param output_size Output buffer capacity
 * \return Length of collapsed text (not including null terminator)
 */
size_t silk_layout_collapse_whitespace(
    const char *text,
    size_t length,
    char *output,
    size_t output_size
);

/**
 * Measure text width in pixels
 *
 * Approximates text width based on character count and font size.
 * Full implementation would use actual font metrics (FreeType + HarfBuzz).
 *
 * Current implementation: monospace approximation
 * - Average character width ≈ 0.6 * font_size
 * - Spaces ≈ 0.25 * font_size
 * - Uppercase letters ≈ 0.875 * font_size
 *
 * \param text Text to measure
 * \param length Number of characters
 * \param font_size Font size in pixels
 * \return Approximate width in pixels (may differ from actual with proportional fonts)
 */
int32_t silk_layout_measure_text(
    const char *text,
    size_t length,
    int32_t font_size
);

/**
 * Find line break position
 *
 * Given available width, finds where text can be wrapped to next line.
 * Uses binary search to find longest text that fits within width.
 * Useful for responsive layout and text wrapping.
 *
 * \param text Text to break
 * \param length Text length
 * \param available_width Width available for line (in pixels)
 * \param font_size Font size for text measurement
 * \return Position to break text (0 if even first character too wide)
 */
size_t silk_layout_find_line_break(
    const char *text,
    size_t length,
    int32_t available_width,
    int32_t font_size
);

/**
 * Apply min/max width constraints to computed width
 *
 * Clamps: computed_width = max(min_width, min(computed_width, max_width))
 *
 * \param computed_width Unconstrained width
 * \param min_width Minimum width (0 = no minimum)
 * \param max_width Maximum width (0 = no maximum)
 * \return Constrained width
 */
int32_t silk_layout_constrain_width(
    int32_t computed_width,
    int32_t min_width,
    int32_t max_width
);

/**
 * Calculate total box width including all layers
 *
 * total = margin_left + border_left + padding_left + content_width +
 *         padding_right + border_right + margin_right
 *
 * \param box Layout box
 * \return Total width including all layers, or -1 on overflow
 */
int32_t silk_layout_total_width(const layout_box_t *box);

/**
 * Calculate total box height including all layers
 *
 * total = margin_top + border_top + padding_top + content_height +
 *         padding_bottom + border_bottom + margin_bottom
 *
 * \param box Layout box
 * \return Total height including all layers, or -1 on overflow
 */
int32_t silk_layout_total_height(const layout_box_t *box);

#endif  /* SILKSURF_LAYOUT_H */
