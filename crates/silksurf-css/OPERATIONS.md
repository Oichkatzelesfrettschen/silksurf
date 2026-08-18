# silksurf-css Operations

## Resource bounds

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
`4 MiB` at a safe rule boundary before parsing. This is an
independent cap layered on top of `MAX_CSS_RULES`; the truncation
predates the rule-count cap and exists to bound the tokenizer cost on
very large stylesheets.

The cap sits above the concatenated sheet a real page produces.
`silksurf-app` appends every external stylesheet onto one `css_text`
string before a single parse call, so the bound applies to the sum:
chatgpt.com alone contributes 99,982 bytes inline plus 79,481 bytes
external. A 128 KiB cap truncated that page mid-sheet.

## `MAX_AT_RULE_NESTING_DEPTH`

`parse_at_rule_block` recurses through `CssParser::parse_stylesheet`
for every at-rule that carries a rule list, so nesting depth is the
bound that matters; 32 levels exceeds any authored sheet and stops a
crafted payload from exhausting the stack. It replaces a 4,096-token
cap on nested block size, which silently emptied any large `@layer`
or `@media` block.

## Diagnostics

`CssError::offset` is byte-relative to the input passed to
`parse_stylesheet`. For the `MAX_CSS_RULES` failure the offset is
reported as `0` because the parser has already consumed the entire
input by that point.

## At-rule block grammar

`at_rule_block_holds_rules` selects the block parse from the at-rule
name, because the name fixes the grammar. `@media`, `@supports`,
`@layer`, `@container`, `@scope`, `@document`, `@keyframes`,
`@starting-style`, and `@font-feature-values` carry rule lists;
`@font-face`, `@page`, `@property`, `@counter-style`, `@viewport`,
`@font-palette-values`, and `@position-try` carry declarations. An
unrecognized at-rule is read from its block: a rule list opens a
nested block, a declaration list never does.

Scanning the block for an `Ident Colon` pair instead misreads any rule
list holding a pseudo-class selector at its head, so
`@layer theme{a:hover{...}}` parsed as declarations and discarded
every rule the layer carried.
