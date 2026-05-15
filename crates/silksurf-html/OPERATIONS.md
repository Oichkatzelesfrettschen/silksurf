# silksurf-html Operations

## Resource bounds (P8.S8)

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

## Tree builder

`TreeBuilder::push_token` is the integration point used by
`silksurf-engine`. It is bounded indirectly by the tokenizer cap: the
builder cannot see more tokens than the tokenizer emitted.
