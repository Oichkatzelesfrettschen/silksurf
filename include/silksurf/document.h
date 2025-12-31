#ifndef SILKSURF_DOCUMENT_H
#define SILKSURF_DOCUMENT_H

#include <stdint.h>
#include <stddef.h>
#include "silksurf/renderer.h"
#include "silksurf/events.h"

/* Web document representation - HTML/CSS/DOM/JavaScript */

typedef struct silk_document silk_document_t;
typedef struct silk_element silk_element_t;

/* Forward declarations */
struct silk_arena;

/* Document creation and lifecycle */
silk_document_t *silk_document_create(size_t arena_size);
void silk_document_destroy(silk_document_t *doc);

/* Internal accessor for CSS engine */
struct silk_arena *silk_document_get_arena(silk_document_t *doc);

/* Content loading */
int silk_document_load_html(silk_document_t *doc, const char *html,
                             size_t html_len);
int silk_document_load_html_file(silk_document_t *doc, const char *filename);

/* Layout and rendering */
int silk_document_layout(silk_document_t *doc, int width, int height);
void silk_document_render(silk_document_t *doc);
void silk_document_set_renderer(silk_document_t *doc, silk_renderer_t *renderer);

/* Document properties */
const char *silk_document_get_title(silk_document_t *doc);
const char *silk_document_get_content_type(silk_document_t *doc);

/* Element access */
void silk_document_set_root_element(silk_document_t *doc, struct silk_dom_node *root);
silk_element_t *silk_document_get_element_by_id(silk_document_t *doc,
                                                  const char *id);
silk_element_t *silk_document_get_root_element(silk_document_t *doc);

/* Event handling */
void silk_document_handle_event(silk_document_t *doc, silk_event_t *event);

/* Script execution */
int silk_document_execute_script(silk_document_t *doc, const char *script,
                                  size_t script_len);
int silk_document_execute_script_file(silk_document_t *doc,
                                       const char *filename);

/* State queries */
int silk_document_is_loaded(silk_document_t *doc);
int silk_document_is_rendering(silk_document_t *doc);
int silk_document_get_scroll_x(silk_document_t *doc);
int silk_document_get_scroll_y(silk_document_t *doc);
void silk_document_set_scroll(silk_document_t *doc, int x, int y);

/* Statistics */
int silk_document_element_count(silk_document_t *doc);
size_t silk_document_memory_used(silk_document_t *doc);

#endif
