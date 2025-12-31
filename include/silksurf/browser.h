#ifndef SILKSURF_BROWSER_H
#define SILKSURF_BROWSER_H

/* Core browser engine - from NeoSurf */

typedef struct silk_browser silk_browser_t;
typedef struct silk_url silk_url_t;
typedef struct silk_dom silk_dom_t;
typedef struct silk_style silk_style_t;

/* Browser creation */
silk_browser_t *silk_browser_create(void);
void silk_browser_destroy(silk_browser_t *b);

/* Navigation */
void silk_browser_navigate(silk_browser_t *b, const char *url);
void silk_browser_reload(silk_browser_t *b);
void silk_browser_back(silk_browser_t *b);
void silk_browser_forward(silk_browser_t *b);

/* DOM access */
silk_dom_t *silk_browser_get_dom(silk_browser_t *b);
int silk_dom_element_count(silk_dom_t *dom);
void silk_dom_render(silk_dom_t *dom);

/* Rendering */
void silk_browser_render(silk_browser_t *b);
void silk_browser_scroll(silk_browser_t *b, int dx, int dy);
void silk_browser_zoom(silk_browser_t *b, float factor);

#endif
