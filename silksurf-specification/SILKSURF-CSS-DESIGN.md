# SilkSurf CSS Tokenizer Design (Rust)

## Goals
- Cleanroom CSS Syntax Level 3 tokenizer with streaming input.
- Emit a stable token stream for parser and cascade stages.
- Preserve raw text for values; avoid lossy normalization.
- Deterministic errors with byte offsets for diagnostics.

## Non-Goals (Phase 1)
- Full selector matching, cascade, and computed value resolution.
- Complete escape sequence handling and unicode ranges.

## Token Model (Initial Subset)
- `Ident(String)`
- `Hash(String)`
- `String(String)`
- `Number(String)`
- `Delim(char)`
- `Colon`, `Semicolon`, `Comma`
- `CurlyOpen`, `CurlyClose`
- `ParenOpen`, `ParenClose`
- `BracketOpen`, `BracketClose`
- `Whitespace`
- `Eof`

## API Surface
- `CssTokenizer::new()`
- `CssTokenizer::feed(&mut self, input: &str) -> Result<Vec<CssToken>, CssError>`
- `CssTokenizer::finish(&mut self) -> Result<Vec<CssToken>, CssError>`

## Conformance Strategy
- Use WPT CSS syntax tests as fixtures (cleanroom).
- Add SilkSurf-owned tests for token boundaries and comment handling.

## Performance Targets
- Streaming throughput: >= 40 MB/s on reference hardware.
- Minimal allocations for ASCII-only inputs.
- Zero regex usage in hot paths.
