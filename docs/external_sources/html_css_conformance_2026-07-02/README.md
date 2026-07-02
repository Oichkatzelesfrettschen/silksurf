# HTML and CSS Conformance Source Bundle

This bundle retains primary source material for HTML and CSS conformance work.

## Sources

| File | Source | Role |
| --- | --- | --- |
| `css21.pdf` | `https://www.w3.org/TR/CSS21/css2.pdf` | CSS 2.1 Recommendation reference for legacy layout, cascade, media, and visual formatting behavior. |
| `css22.pdf` | `https://www.w3.org/TR/CSS22/css2.pdf` | CSS 2.2 Working Draft reference for CSS2-family browser behavior. |
| `html40.pdf` | `https://www.w3.org/TR/1998/REC-html40-19980424/html40.pdf` | Historical HTML PDF reference for legacy element and document behavior. |
| `html-living-standard.html` | `https://html.spec.whatwg.org/multipage/` | Current HTML living standard snapshot. The source endpoint serves HTML, not PDF. |
| `css-syntax-3.html` | `https://www.w3.org/TR/css-syntax-3/` | CSS Syntax Level 3 snapshot for tokenizer and parser conformance. |
| `selectors-4.html` | `https://www.w3.org/TR/selectors-4/` | Selectors Level 4 snapshot for selector parsing and matching conformance. |
| `css-cascade-5.html` | `https://www.w3.org/TR/css-cascade-5/` | CSS Cascade Level 5 snapshot for origin, specificity, inheritance, and computed-value behavior. |
| `css-values-4.html` | `https://www.w3.org/TR/css-values-4/` | CSS Values and Units Level 4 snapshot for numeric, length, percentage, and function values. |
| `css-color-4.html` | `https://www.w3.org/TR/css-color-4/` | CSS Color Level 4 snapshot for color syntax and computed color behavior. |
| `css-backgrounds-3.html` | `https://www.w3.org/TR/css-backgrounds-3/` | CSS Backgrounds and Borders Level 3 snapshot for paint primitives. |
| `css-flexbox-1.html` | `https://www.w3.org/TR/css-flexbox-1/` | CSS Flexible Box Layout Level 1 snapshot for Taffy-backed layout conformance. |
| `css-display-3.html` | `https://www.w3.org/TR/css-display-3/` | CSS Display Level 3 snapshot for box-generation and hidden-subtree behavior. |
| `css-text-3.html` | `https://www.w3.org/TR/css-text-3/` | CSS Text Level 3 snapshot for whitespace, wrapping, and text layout behavior. |

`SHA256SUMS` records the retained bytes.

## Fetch Command

```sh
scripts/fetch_html_css_conformance_sources.sh
```

`SILKSURF_WGET_UA` overrides the default Mozilla user agent when a source
requires a different replay string.

## Notes

The W3C module PDF endpoints for CSS Syntax Level 3, Selectors Level 4, CSS
Cascade Level 5, CSS Values and Units Level 4, CSS Color Level 4, CSS
Backgrounds and Borders Level 3, CSS Flexible Box Layout Level 1, CSS Display
Level 3, CSS Text Level 3, and HTML 5.2 return 404 during source discovery.
The bundle keeps the available primary PDFs and stores current module
standards in the HTML format the source publishes.
