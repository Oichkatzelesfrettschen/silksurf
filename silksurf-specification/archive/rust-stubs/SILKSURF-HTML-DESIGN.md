# SilkSurf HTML Tokenizer Design (Rust)

## Goals
- Cleanroom HTML5 tokenizer with streaming input and deterministic output.
- Zero-copy where possible, with explicit ownership for emitted tokens.
- Error recovery per HTML5 rules; record recoverable errors for diagnostics.
- Stable public API that the DOM tree builder can consume.

## Non-Goals (Phase 1)
- Full HTML5 conformance in the first milestone.
- Script execution or DOM mutation during tokenization.

## Token Model
Tokens are emitted as a sequence:
- `Doctype { name, public_id, system_id, force_quirks }`
- `StartTag { name, attributes, self_closing }`
- `EndTag { name }`
- `Comment { data }`
- `Character { data }`
- `Eof`

Token fields are normalized to ASCII-lowercase tag names; attributes preserve
original value bytes and decoded Unicode scalars.

## State Machine Outline
Initial subset:
- Data
- TagOpen
- EndTagOpen
- TagName
- AttributeName
- AttributeValue (quoted/unquoted)
- SelfClosingStartTag

Unsupported states return structured errors and are tracked for backlog.

## API Surface
- `Tokenizer::new()` creates a fresh tokenizer.
- `Tokenizer::feed(&mut self, input: &str)` pushes bytes and returns tokens.
- `Tokenizer::finish(&mut self)` flushes remaining tokens and returns `Eof`.
- Errors are returned as `TokenizeError` with state + byte offset.

## Conformance Strategy
- Use html5lib test vectors for tokenizer compliance.
- Add SilkSurf-owned tests for error recovery and edge cases.

## Performance Targets
- Streaming throughput: >= 50 MB/s on reference hardware.
- Zero allocations for ASCII text-only inputs.
- Avoid regex and heap churn in hot paths.
