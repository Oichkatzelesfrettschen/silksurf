# Phase 4a: HTML5 Parser Implementation - Granular Specification

**Date:** 2025-12-30
**Status:** Implementation Phase
**Target:** Working HTML parser with DOM tree construction

---

## 1. Scope Breakdown

### 1.1 Core Tasks

**Task 1: DOM Node Model**
- Create opaque silk_dom_node_t type
- Implement node creation (element, text, comment, doctype)
- Implement node tree structure (parent, children, siblings)
- Reference counting for node lifecycle

**Task 2: Tree Handler Implementation**
- Implement all 16 hubbub_tree_handler callbacks
- Map callbacks to node creation/manipulation
- Handle node references with arena allocation

**Task 3: Parser Integration**
- Create hubbub_parser with tree handler
- Implement chunk-based parsing
- Handle parse errors gracefully

**Task 4: Document Model Update**
- Extend silk_document_t with DOM root
- Update layout to traverse DOM
- Connect parsing to document lifecycle

**Task 5: Testing**
- Create test HTML documents
- Verify DOM construction
- Test layout traversal

---

## 2. Implementation Order (Top-Down)

### Step 1: Create DOM Node Structure
**File:** `src/document/dom_node.c` + `include/silksurf/dom_node.h`

```
silk_dom_node_t {
  - type (ELEMENT, TEXT, COMMENT, DOCTYPE)
  - name/data (element tag, text content, etc)
  - attributes (for elements)
  - parent, first_child, last_child, next_sibling
  - layout index (reference to layout_nodes array)
  - reference count
}
```

### Step 2: Implement Tree Callbacks
**File:** `src/document/tree_builder.c`

```
16 callback handlers:
  - create_comment() → allocate comment node
  - create_element() → allocate element, store tag/namespace
  - create_text() → allocate text node, store content
  - append_child() → link parent-child
  - insert_before() → insert between siblings
  - etc.
```

### Step 3: Integrate with Parser
**File:** `src/document/document.c` (update)

```
silk_document_load_html() {
  1. Create tree handler with callbacks
  2. Create parser: hubbub_parser_create()
  3. Parse chunks: hubbub_parser_parse_chunk()
  4. Get root node from callbacks
  5. Store in doc->dom_root
}
```

### Step 4: Layout Traversal
**File:** `src/document/document.c` (update)

```
silk_document_layout() {
  1. Traverse DOM tree (depth-first)
  2. Allocate layout_nodes for each DOM node
  3. Compute positions/sizes
  4. Store computed layout in parallel array
}
```

---

## 3. Memory Model

### Arena-Based Allocation

```
DOM nodes allocated in arena:
  ├── Element nodes (~200 bytes each: tag, attrs, children ptrs)
  ├── Text nodes (~100 bytes each: content string)
  └── Attribute arrays (variable size)

Layout nodes allocated in arena:
  ├── Parallel to DOM (1:1 mapping)
  └── Contains computed box model + style data

Total: Arena constrains total document size
  - 64 MB arena = ~300k elements maximum
  - Typical documents: <10k elements = <5 MB
```

### Reference Counting Strategy

```
silk_dom_node_t {
  int ref_count;  // hubbub may hold references
}

Node lifecycle:
  1. Created by callback (ref_count = 1)
  2. Added to tree (ref_count++)
  3. Parser finishes (ref_count--)
  4. Destroyed when ref_count = 0
```

---

## 4. API Design

### dom_node.h (New)

```c
typedef struct silk_dom_node silk_dom_node_t;

typedef enum {
    SILK_NODE_ELEMENT,
    SILK_NODE_TEXT,
    SILK_NODE_COMMENT,
    SILK_NODE_DOCTYPE
} silk_node_type_t;

/* Creation */
silk_dom_node_t *silk_dom_node_create_element(const char *tag);
silk_dom_node_t *silk_dom_node_create_text(const char *content);

/* Tree operations */
void silk_dom_node_append_child(silk_dom_node_t *parent,
                                 silk_dom_node_t *child);
void silk_dom_node_remove_child(silk_dom_node_t *parent,
                                 silk_dom_node_t *child);

/* Tree traversal */
silk_dom_node_t *silk_dom_node_get_parent(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_first_child(silk_dom_node_t *node);
silk_dom_node_t *silk_dom_node_get_next_sibling(silk_dom_node_t *node);

/* Attributes */
const char *silk_dom_node_get_tag_name(silk_dom_node_t *node);
const char *silk_dom_node_get_text_content(silk_dom_node_t *node);
const char *silk_dom_node_get_attribute(silk_dom_node_t *node,
                                         const char *name);

/* Lifecycle */
void silk_dom_node_ref(silk_dom_node_t *node);
void silk_dom_node_unref(silk_dom_node_t *node);
```

### document.h (Updated)

```c
/* Core document operations - already defined */
int silk_document_load_html(silk_document_t *doc, const char *html,
                             size_t html_len);

/* DOM access */
silk_dom_node_t *silk_document_get_root_element(silk_document_t *doc);
```

---

## 5. Callback Implementation Strategy

### Simplest Approach: Direct Node Allocation

```c
/* Tree handler context - passed to all callbacks */
struct tree_context {
    silk_arena_t *arena;        /* For allocating nodes */
    silk_dom_node_t *root;      /* Root node */
    silk_dom_node_t *current;   /* Current open node */
};

/* Callback example: create_element */
static hubbub_error create_element(void *ctx, const char *name, ...,
                                    void **node) {
    tree_context_t *tree = (tree_context_t *)ctx;

    /* Allocate node in arena */
    *node = silk_dom_node_create_element(name);

    /* If we have a current parent, add as child */
    if (tree->current) {
        silk_dom_node_append_child(tree->current, *node);
    } else {
        tree->root = *node;  /* First element is root */
    }

    return HUBBUB_OK;
}

/* Callback: append_child */
static hubbub_error append_child(void *ctx, void *parent, void *child) {
    silk_dom_node_append_child((silk_dom_node_t *)parent,
                               (silk_dom_node_t *)child);
    return HUBBUB_OK;
}
```

---

## 6. Error Handling

### Parser Errors

```
hubbub_parser_parse_chunk() returns:
  HUBBUB_OK               → Continue parsing
  HUBBUB_NOMEM            → Arena full (fail parse)
  HUBBUB_INVALID          → Malformed HTML (warn, continue)
  HUBBUB_NEEDDATA         → Normal (more chunks coming)

Action: Check return code, log, continue gracefully
```

### Memory Errors

```
Arena allocation failure:
  - silk_dom_node_create_element returns NULL
  - Check in callback, return HUBBUB_NOMEM
  - Parser stops, document incomplete
  - Render what we have
```

---

## 7. Layout Integration

### Traversal Algorithm

```c
void layout_dom_tree(silk_document_t *doc, silk_dom_node_t *node,
                      int x, int y, int max_width) {
    if (!node)
        return;

    /* Allocate layout node for this DOM node */
    struct silk_layout *layout = &doc->layout_nodes[doc->layout_count++];
    node->layout_index = doc->layout_count - 1;

    /* Compute layout based on node type */
    switch (silk_dom_node_get_type(node)) {
    case SILK_NODE_ELEMENT: {
        const char *tag = silk_dom_node_get_tag_name(node);

        if (is_block(tag)) {
            layout->x = 0;
            layout->y = y;
            layout->width = max_width;

            int child_y = y;
            for (silk_dom_node_t *child = silk_dom_node_get_first_child(node);
                 child; child = silk_dom_node_get_next_sibling(child)) {
                child_y += layout_dom_tree(doc, child, 0, child_y, max_width);
            }
            layout->height = child_y - y;
        } else if (is_inline(tag)) {
            layout->x = x;
            layout->y = y;
            layout->width = 200;  /* Estimate */
            layout->height = 16;
        }
        break;
    }
    case SILK_NODE_TEXT: {
        const char *text = silk_dom_node_get_text_content(node);
        layout->width = strlen(text) * 8;  /* Monospace estimate */
        layout->height = 16;
        break;
    }
    default:
        layout->width = 0;
        layout->height = 0;
    }

    /* Recurse to children (if not done above) */
    /* ... */

    return layout->height;
}
```

---

## 8. Testing Plan

### Test 1: Simple Document

```html
<html>
  <body>
    <h1>Test</h1>
  </body>
</html>
```

**Expected:**
- Root: <html> element
- Child: <body> element
- Grandchild: <h1> element
- Text node: "Test"

### Test 2: Nested Elements

```html
<div>
  <p>Paragraph 1</p>
  <p>Paragraph 2</p>
</div>
```

**Expected:**
- <div> with 2 <p> children
- Each <p> with text node

### Test 3: Malformed HTML

```html
<p>Unclosed paragraph
<div>Div after paragraph</div>
```

**Expected:**
- Parser auto-closes <p>
- Creates proper DOM structure

---

## 9. Success Criteria

- [x] DOM node structure defined
- [ ] Tree handler callbacks implemented (16 functions)
- [ ] Parser integration complete
- [ ] DOM tree construction working
- [ ] Layout traversal functional
- [ ] All 3 test documents parse correctly
- [ ] No memory leaks (valgrind clean)
- [ ] Binary compiles without errors

---

## 10. Code Style Guidelines

**For elegance:**
- Short, focused functions (<50 LOC each)
- Clear naming: create_X, append_X, get_X
- Consistent error checking
- No magic numbers (use named constants)
- Document via comments where logic is non-obvious
- Use arena allocator exclusively (no malloc in callbacks)

**Example:**
```c
/* Good: Clear intent, focused */
static silk_dom_node_t *create_node(silk_arena_t *arena,
                                     silk_node_type_t type) {
    silk_dom_node_t *node = silk_arena_alloc(arena, sizeof(*node));
    if (node) {
        node->type = type;
        node->ref_count = 1;
    }
    return node;
}

/* Bad: Magic numbers, unclear */
static void *alloc_node(void *arena, int type) {
    void *n = arena_alloc(arena, 128);  // Magic 128!
    if (n) *(int*)n = type;
    return n;
}
```

---

## 11. Implementation Estimate

| Task | LOC | Files | Time |
|------|-----|-------|------|
| DOM node structure | 150 | 2 | 15 min |
| Tree builder callbacks | 300 | 1 | 30 min |
| Parser integration | 100 | 1 | 10 min |
| Layout traversal | 100 | 1 | 10 min |
| Testing | 150 | 1 | 15 min |
| **Total** | **800** | **6** | **80 min** |

---

## 12. Integration Points

### With Phase 3 Renderer

```
Document parsing → DOM tree
       ↓
Layout algorithm → layout_nodes array
       ↓
Render tree → silk_renderer_fill_rect() calls
       ↓
Damage tracking → Only redraw changed elements
       ↓
X11 presentation
```

### With Phase 2 Memory System

```
silk_arena_t (64 MB)
├── DOM nodes (variable size)
├── layout_nodes (fixed: 4KB per node)
└── Attributes (variable)
```

---

## Next: Begin Implementation

Ready to implement with full specification and elegant code structure.
