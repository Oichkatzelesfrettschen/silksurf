# SilkSurf Phase 4: Web Engine Integration - Design Document

**Date:** 2025-12-30
**Status:** Design Phase
**Next Phase:** Implementation

---

## 1. Overview

Phase 4 integrates a complete web engine into SilkSurf, enabling rendering of HTML/CSS/JavaScript content. The design reuses optimized NetSurf libraries (libhubbub, libcss, libdom) to minimize code size while ensuring compatibility with existing web standards.

**Goals:**
- Parse HTML5 into DOM tree (via libhubbub)
- Apply CSS stylesheets (via libcss)
- Represent document structure (via libdom)
- Execute JavaScript (via Duktape)
- Render styled DOM to screen (via Phase 3 renderer)

---

## 2. Component Integration Map

### 2.1 Data Flow Pipeline

```
HTML Input
    ↓
libhubbub HTML5 Parser
    ↓
DOM Tree (libdom)
    ↓
CSS Cascade (libcss)
    ↓
Computed Styles + Layout
    ↓
Render Tree
    ↓
Phase 3 Renderer (damage tracking, SIMD pixel ops)
    ↓
X11 Window Display
```

### 2.2 Library Selection & Rationale

| Library | Purpose | Why Chosen | Alternative | Trade-off |
|---------|---------|-----------|-------------|-----------|
| **libhubbub** | HTML5 Parser | NetSurf origin, <100 KB | libxml2 | No external deps |
| **libcss** | CSS Engine | NetSurf origin, compact | Blink/WebKit | Minimal code |
| **libdom** | DOM Tree | NetSurf origin, lean | JSDOM | C/low-memory |
| **Duktape** | JavaScript | Embeddable, small (~200 KB) | V8/SpiderMonkey | Less optimization |
| **libnsfb** | Framebuffer | NetSurf output layer | Direct X11 | Abstraction layer |

---

## 3. Architectural Components

### 3.1 Document Model

```c
struct silk_document {
    /* Parsed content */
    dom_document *dom_root;              /* libdom root node */
    css_stylesheet *stylesheets[16];    /* Applied stylesheets */

    /* Layout cache */
    struct {
        int x, y, width, height;        /* Computed box model */
        struct silk_style_data {
            uint32_t color;             /* Foreground color (ARGB32) */
            uint32_t background_color;  /* Background (ARGB32) */
            int font_size;              /* In pixels */
            int font_weight;            /* 100-900 */
        } style;
    } *layout_nodes;                    /* Parallel array to DOM */
    int layout_count;

    /* JavaScript context */
    duk_context *js_ctx;                /* Duktape interpreter */

    /* Rendering state */
    silk_renderer_t *renderer;
    int dirty;
};
```

### 3.2 Parsing Pipeline

**Step 1: HTML Parsing (libhubbub)**
```c
silk_document_t *silk_document_create(const char *html, size_t html_len) {
    silk_document_t *doc = malloc(sizeof(silk_document_t));

    /* Create HTML5 parser */
    hubbub_parser *parser = hubbub_parser_create(
        "UTF-8",                               /* Charset */
        HUBBUB_TREE_FRAGMENT,                 /* Parse mode */
        &callbacks,                           /* Event handlers */
        doc                                   /* Opaque context */
    );

    /* Parse HTML content */
    hubbub_parser_parse_chunk(parser, (uint8_t *)html, html_len);
    hubbub_parser_insert_element(parser, ...);  /* EOF handling */

    /* Extract DOM tree from parser internals */
    doc->dom_root = hubbub_parser_get_document(parser);
    hubbub_parser_destroy(parser);

    return doc;
}
```

**Callback Handlers (from libhubbub):**
```c
static void tree_insert_element(void *ctx, const dom_element *elem) {
    silk_document_t *doc = (silk_document_t *)ctx;

    /* Build parallel layout node for element */
    append_layout_node(doc, elem, &default_style);
}

static void tree_insert_text(void *ctx, const dom_text *text) {
    silk_document_t *doc = (silk_document_t *)ctx;

    /* Allocate text layout node */
    append_layout_node(doc, (dom_node *)text, &text_style);
}
```

---

### 3.3 CSS Cascade

**Step 2: Style Resolution (libcss)**

```c
void silk_document_apply_stylesheets(silk_document_t *doc) {
    /* Create CSS selector (matches elements against rules) */
    css_select_ctx *select_ctx = css_select_ctx_create();

    /* Add default stylesheet (HTML baseline) */
    css_stylesheet *html_ss = create_default_stylesheet();
    css_select_ctx_append_stylesheet(select_ctx,
        CSS_ORIGIN_UA,      /* User-agent stylesheet */
        NULL,               /* Media types (all) */
        html_ss
    );

    /* Traverse DOM and compute styles */
    traverse_dom_compute_styles(doc->dom_root, select_ctx, doc);
}

static void compute_node_style(silk_document_t *doc, dom_node *node,
                               css_select_ctx *ctx) {
    if (node->type != DOM_ELEMENT_NODE)
        return;

    dom_element *elem = (dom_element *)node;

    /* Query CSS for computed styles */
    css_computed_style *computed = css_computed_style_create(...);
    css_select_style(ctx, elem, NULL, NULL, NULL, computed);

    /* Map CSS computed values to layout */
    struct silk_style_data *style = &doc->layout_nodes[node->layout_idx].style;
    style->color = css_color_to_argb32(computed->color);
    style->background_color = css_color_to_argb32(computed->background_color);
    style->font_size = css_length_to_pixels(computed->font_size);

    /* Recurse to children */
    dom_node *child = node->first_child;
    while (child) {
        compute_node_style(doc, child, ctx);
        child = child->next_sibling;
    }
}
```

---

### 3.4 Layout Engine

**Step 3: Compute Layout (Simplistic Box Model)**

```c
void silk_document_layout(silk_document_t *doc, int width, int height) {
    /* Viewport dimensions */
    int viewport_width = width;
    int viewport_height = height;

    /* Traverse DOM and compute positions */
    layout_node(doc, doc->dom_root, 0, 0, viewport_width, 0);
}

static int layout_node(silk_document_t *doc, dom_node *node,
                       int x, int y, int max_width, int parent_y) {
    if (!node)
        return 0;

    struct silk_layout *layout = &doc->layout_nodes[node->layout_idx];

    switch (node->type) {
    case DOM_ELEMENT_NODE: {
        dom_element *elem = (dom_element *)node;
        const char *tag = dom_element_get_tag_name(elem);

        /* Block-level elements */
        if (is_block_element(tag)) {
            layout->x = 0;
            layout->y = parent_y;
            layout->width = max_width;
            layout->height = 32;  /* Default height (computed later) */

            int child_y = parent_y;
            dom_node *child = node->first_child;
            while (child) {
                child_y += layout_node(doc, child, 0, child_y, max_width, child_y);
                child = child->next_sibling;
            }

            layout->height = child_y - parent_y;
            return layout->height;
        }

        /* Inline elements */
        if (is_inline_element(tag)) {
            layout->x = x;
            layout->y = y;
            layout->width = 200;  /* Estimated; refined by text measurement */
            layout->height = 16;  /* Line height */
            return layout->height;
        }
        break;
    }

    case DOM_TEXT_NODE: {
        /* Text node - measure and position */
        dom_text *text = (dom_text *)node;
        const char *content = dom_text_get_data(text);

        /* Measure text width (simplified: assume monospace 8px per char) */
        int text_width = strlen(content) * 8;
        if (x + text_width > max_width) {
            /* Wrap to next line */
            layout->x = 0;
            layout->y = y + 16;
        } else {
            layout->x = x;
            layout->y = y;
        }
        layout->width = text_width;
        layout->height = 16;  /* Line height */
        return layout->height;
    }

    default:
        return 0;
    }

    return 0;
}
```

---

### 3.5 Render Tree Generation

**Step 4: Generate Render Instructions**

```c
void silk_document_render(silk_document_t *doc) {
    if (!doc || !doc->renderer)
        return;

    /* Begin frame with damage tracking */
    silk_renderer_begin_frame(doc->renderer);

    /* Clear background */
    silk_renderer_clear(doc->renderer, SILK_COLOR_WHITE);

    /* Traverse layout nodes and render */
    render_node(doc, doc->dom_root);

    /* End frame and present */
    silk_renderer_end_frame(doc->renderer);
    silk_renderer_present(doc->renderer);
}

static void render_node(silk_document_t *doc, dom_node *node) {
    if (!node)
        return;

    struct silk_layout *layout = &doc->layout_nodes[node->layout_idx];
    struct silk_style_data *style = &layout->style;

    /* Render background */
    if (style->background_color != SILK_COLOR_TRANSPARENT) {
        silk_renderer_fill_rect(doc->renderer,
            layout->x, layout->y, layout->width, layout->height,
            style->background_color);
    }

    /* Render text content */
    if (node->type == DOM_TEXT_NODE) {
        dom_text *text = (dom_text *)node;
        const char *content = dom_text_get_data(text);
        render_text(doc, layout->x, layout->y, content, style->color);
    }

    /* Render children */
    dom_node *child = node->first_child;
    while (child) {
        render_node(doc, child);
        child = child->next_sibling;
    }
}

static void render_text(silk_document_t *doc, int x, int y,
                        const char *text, silk_color_t color) {
    /* TODO: Font rasterization via FreeType or bitmap fonts
       For MVP: Render simple ASCII using 8x8 bitmap font */
    for (const char *p = text; *p; p++) {
        render_char(doc, x, y, *p, color);
        x += 8;  /* Character width */
    }
}
```

---

### 3.6 JavaScript Integration

**Step 5: Execute Scripts (Duktape)**

```c
void silk_document_execute_script(silk_document_t *doc, const char *script) {
    if (!doc->js_ctx)
        doc->js_ctx = duk_create_heap_default();

    duk_context *ctx = doc->js_ctx;

    /* Push document object as 'document' global */
    duk_push_object(ctx);
    duk_push_pointer(ctx, doc);
    duk_put_prop_string(ctx, -2, "_internal");

    /* Register API functions */
    register_dom_api(ctx, doc);
    register_event_api(ctx, doc);

    duk_put_global_string(ctx, "document");

    /* Execute script */
    duk_push_lstring(ctx, script, strlen(script));
    if (duk_peval(ctx) != 0) {
        fprintf(stderr, "Script error: %s\n", duk_safe_to_string(ctx, -1));
    }
    duk_pop(ctx);
}

static int dom_get_element_by_id(duk_context *ctx) {
    silk_document_t *doc = get_document_from_ctx(ctx);
    const char *id = duk_to_string(ctx, 0);

    dom_element *elem = find_element_by_id(doc->dom_root, id);
    if (!elem) {
        duk_push_null(ctx);
    } else {
        duk_push_pointer(ctx, elem);
    }
    return 1;
}
```

---

## 4. Memory Model

### 4.1 Arena Allocation Strategy

```
+------------------+
| Arena (64 MB)    |
+------------------+
|  DOM Tree        |  ~40% (for large documents)
|  Layout Nodes    |  ~15% (parallel to DOM)
|  CSS Cache       |  ~20% (computed styles)
|  String Pool     |  ~10% (element names, text)
|  JS Heap         |  ~15% (Duktape VM)
+------------------+
```

### 4.2 Memory Ownership

- **DOM nodes:** Allocated in arena, freed on document destroy
- **Layout nodes:** Parallel array, same arena
- **CSS computed styles:** Cached in layout nodes
- **JavaScript heap:** Managed by Duktape, destroyed with context
- **Pixmap cache:** Shared with renderer (Phase 3)

---

## 5. Integration with Phase 3 Renderer

### Rendering Flow

```c
int main() {
    /* Phase 3 setup */
    silk_renderer_t *renderer = silk_renderer_create(...);

    /* Phase 4 setup */
    silk_document_t *doc = silk_document_create(html, strlen(html));
    doc->renderer = renderer;

    /* Layout and render */
    silk_document_layout(doc, 1024, 768);
    silk_document_render(doc);  /* Uses renderer internally */

    /* Handle events */
    silk_event_t event;
    while (get_next_event(&event)) {
        silk_document_handle_event(doc, &event);

        /* Re-render if dirty */
        if (doc->dirty) {
            silk_document_layout(doc, 1024, 768);
            silk_document_render(doc);
            doc->dirty = 0;
        }
    }

    silk_document_destroy(doc);
    silk_renderer_destroy(renderer);
}
```

### Damage Tracking via Renderer

- Each `silk_renderer_fill_rect()` call tracks damage
- DOM mutations trigger layout recalculation
- Only changed regions redrawn (via Phase 3 damage tracking)
- Result: Sub-second re-render for DOM changes

---

## 6. Implementation Phases

### Phase 4a: DOM Parser (libhubbub)
1. Link libhubbub library
2. Implement HTML parsing pipeline
3. Build layout node parallel array
4. Basic block-level layout

**Est. Code:** 200 LOC | **Binary:** +80 KB

### Phase 4b: CSS Engine (libcss)
1. Link libcss library
2. Implement CSS cascade
3. Add default HTML stylesheet
4. Compute computed styles

**Est. Code:** 150 LOC | **Binary:** +120 KB

### Phase 4c: Layout Engine
1. Implement block/inline box model
2. Text measurement and wrapping
3. Positioned elements (basic)
4. Viewport-relative coordinates

**Est. Code:** 300 LOC | **Binary:** +15 KB

### Phase 4d: Text Rendering
1. Bitmap font rasterization
2. Text layout and wrapping
3. Color and background rendering
4. Simple text metrics

**Est. Code:** 150 LOC | **Binary:** +10 KB

### Phase 4e: JavaScript (Duktape)
1. Link Duktape library
2. Create JS context
3. Expose DOM API to scripts
4. Event handling integration

**Est. Code:** 250 LOC | **Binary:** +200 KB

---

## 7. API Design

### silk_document.h

```c
/* Creation/destruction */
silk_document_t *silk_document_create(const char *html, size_t len);
void silk_document_destroy(silk_document_t *doc);

/* Parsing and rendering */
int silk_document_parse(silk_document_t *doc);
void silk_document_layout(silk_document_t *doc, int width, int height);
void silk_document_render(silk_document_t *doc);

/* Content access */
const char *silk_document_get_title(silk_document_t *doc);
silk_element_t *silk_document_get_element_by_id(silk_document_t *doc,
                                                  const char *id);

/* Scripting */
void silk_document_execute_script(silk_document_t *doc,
                                   const char *script);

/* Events */
void silk_document_handle_event(silk_document_t *doc,
                                 const silk_event_t *event);
```

---

## 8. Prototype Example

### Rendering a Simple HTML Document

```html
<!DOCTYPE html>
<html>
<head>
  <title>SilkSurf Test</title>
  <style>
    body { background-color: white; color: black; }
    h1 { color: blue; font-size: 32px; }
    p { font-size: 14px; margin: 10px; }
  </style>
</head>
<body>
  <h1>Welcome to SilkSurf</h1>
  <p>This is a test page.</p>
  <script>
    console.log("JavaScript executed!");
    document.getElementById("test").style.color = "red";
  </script>
</body>
</html>
```

### Rendering in SilkSurf

```c
int main() {
    /* Read HTML from file */
    const char *html = "<html><body>Hello</body></html>";

    /* Create renderer */
    silk_window_mgr_t *win_mgr = silk_window_mgr_create(NULL);
    silk_app_window_t *window = silk_window_mgr_create_window(win_mgr,
        "SilkSurf", 1024, 768);
    silk_renderer_t *renderer = silk_renderer_create(win_mgr, window, 16*1024*1024);

    /* Create and render document */
    silk_document_t *doc = silk_document_create(html, strlen(html));
    silk_document_parse(doc);
    silk_document_layout(doc, 1024, 768);
    doc->renderer = renderer;
    silk_document_render(doc);

    /* Event loop */
    while (window_is_open(window)) {
        silk_event_t event;
        if (get_next_event(&event)) {
            silk_document_handle_event(doc, &event);
            if (doc->dirty) {
                silk_document_layout(doc, 1024, 768);
                silk_document_render(doc);
                doc->dirty = 0;
            }
        }
        usleep(16666);  /* 60 FPS */
    }

    silk_document_destroy(doc);
    silk_renderer_destroy(renderer);
}
```

---

## 9. Performance Targets

| Metric | Target | Method |
|--------|--------|--------|
| HTML parse (100 KB) | < 50 ms | Streaming parser |
| CSS cascade | < 20 ms | Selector index |
| Layout (1000 elements) | < 30 ms | Single pass |
| Render (1024x768) | < 16 ms | SIMD pixel ops |
| JavaScript (simple) | < 100 ms | Duktape VM |

---

## 10. Risk Analysis

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| libhubbub complexity | Medium | High | Use NetSurf examples, test suite |
| CSS selector performance | Medium | Medium | Pre-index selectors, memoize |
| Layout recalc overhead | High | Medium | Dirty rectangle tracking |
| JS performance | High | Low | Limit script execution time |
| Memory footprint | Medium | Medium | Arena allocator cap, pixmap cache |

---

## 11. Success Criteria

- [x] Phase 3 rendering pipeline complete
- [ ] HTML5 parsing functional
- [ ] CSS cascade applied
- [ ] Basic layout computed
- [ ] Text rendered on screen
- [ ] JavaScript execution possible
- [ ] Simple web pages render correctly
- [ ] Binary size < 500 KB (with all libraries)

---

## 12. Next Steps

1. **Investigate libhubbub/libcss/libdom** on target system
2. **Create test suite** for parser/CSS/layout
3. **Implement Phase 4a** (HTML parser)
4. **Incrementally integrate** CSS, layout, rendering
5. **Validate against test documents** (e.g., simple HTML files)

---

## References

- NetSurf Project: https://www.netsurf-browser.org/
- libhubbub: HTML5 parser in C
- libcss: CSS engine
- libdom: DOM implementation
- Duktape: Embeddable JavaScript
