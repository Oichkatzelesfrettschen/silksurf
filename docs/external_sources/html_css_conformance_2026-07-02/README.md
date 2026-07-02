# HTML and CSS Conformance Source Bundle

This bundle retains primary source material for HTML and CSS conformance work.

## Sources

| File | Source | Role |
| --- | --- | --- |
| `css21.pdf` | `https://www.w3.org/TR/CSS21/css2.pdf` | CSS 2.1 Recommendation reference for legacy layout, cascade, media, and visual formatting behavior. |
| `css22.pdf` | `https://www.w3.org/TR/CSS22/css2.pdf` | CSS 2.2 Working Draft reference for CSS2-family browser behavior. |
| `html40.pdf` | `https://www.w3.org/TR/1998/REC-html40-19980424/html40.pdf` | Historical HTML PDF reference for legacy element and document behavior. |
| `html-living-standard.html` | `https://html.spec.whatwg.org/multipage/` | Current HTML living standard snapshot. The source endpoint serves HTML, not PDF. |

`SHA256SUMS` records the retained bytes.

## Fetch Command

```sh
scripts/fetch_html_css_conformance_sources.sh
```

`SILKSURF_WGET_UA` overrides the default Mozilla user agent when a source
requires a different replay string.

## Notes

`https://www.w3.org/TR/css-syntax-3/css-syntax-3.pdf` and
`https://www.w3.org/TR/html52/html52.pdf` returned 404 during source
discovery. The bundle keeps the available primary PDFs and stores the current
HTML standard in the format the source publishes.
