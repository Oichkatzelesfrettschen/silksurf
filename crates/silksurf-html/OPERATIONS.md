# silksurf-html Operations

## Resource bounds

| Constant                  | Default     | Enforcement site                      | Failure mode                                     |
|---------------------------|-------------|----------------------------------------|--------------------------------------------------|
| `MAX_TOKENS_PER_FEED`     | `1_000_000` | `Tokenizer::feed` outer loop           | Returns `TokenizeError` (becomes `SilkError::HtmlTokenize`) |

Override by patching the constant at build time (no runtime knob today
-- the tokenizer state lives on the call stack, so a per-instance cap
would require a constructor argument; tracked for the next API window).

The cap counts tokens emitted per individual `feed()` call, not the
cumulative tokens for the document. A streaming consumer that calls
`feed()` repeatedly with smaller chunks is unaffected. The intent is
to bound a single batch's transient memory, not the lifetime of the
document tree.

## Tree construction

`parse_html` is the integration point used by `silksurf-engine`, which imports
it as `html5ever_parse`. Tree construction runs inside html5ever and reaches
the DOM through `treesink::SilkDomBuilder`, so the tokenizer cap above bounds
`Tokenizer` alone and places no bound on this path. html5ever's own input
handling is the bound that applies to a page load.
