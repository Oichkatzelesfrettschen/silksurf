# silksurf-css Operations

## Resource bounds (P8.S8)

| Constant         | Default  | Enforcement site                                    | Failure mode                       |
|------------------|----------|------------------------------------------------------|------------------------------------|
| `MAX_CSS_RULES`  | `50_000` | `parse_stylesheet` (free fn, post-parse check)       | Returns `CssError` (becomes `SilkError::Css`) |

The cap is checked at the top-level `parse_stylesheet` entry point
(and transitively at `parse_stylesheet_bytes`). The inner
`CssParser::parse_stylesheet` method does not currently return
`Result`, so the check happens after parsing rather than during
accumulation. A future API window will push the check into the parser
loop so adversarial inputs abort earlier without allocating the full
rule `Vec`.

## Existing `MAX_CSS_BYTES`

`parse_stylesheet_with_interner` truncates inputs larger than
`128 KiB` at a safe rule boundary before parsing. This is an
independent cap layered on top of `MAX_CSS_RULES`; the truncation
predates the rule-count cap and exists to bound the tokenizer cost on
very large stylesheets (ChatGPT serves ~1.4 MiB of CSS).

## Diagnostics

`CssError::offset` is byte-relative to the input passed to
`parse_stylesheet`. For the `MAX_CSS_RULES` failure the offset is
reported as `0` because the parser has already consumed the entire
input by that point.
