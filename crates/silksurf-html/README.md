# silksurf-html

Cleanroom HTML5 tokenizer and tree builder.

## Public API

  * `Tokenizer` -- state-machine tokenizer with `feed` / `finish`
    streaming entry points. State enum captures every WHATWG state.
  * `Token` -- tokenizer output enum (StartTag, EndTag, Comment, Doctype,
    Character, Eof, ...).
  * `TokenizeError` -- crate-local error; `From<TokenizeError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.
  * `TreeBuilder` -- consumes `Token`s into a `silksurf_dom::Dom`.
    Insertion-mode state machine.
  * `TreeBuildError` -- crate-local error; `From<TreeBuildError> for
    silksurf_core::SilkError` at the bottom of `tree_builder.rs`.

## Conventions

  * Errors have `state`, `offset`, and `message` so caller can render
    "syntax error at byte 1234 in BeforeAttributeName" diagnostics.
  * The tokenizer accepts streaming bytes; call `feed(chunk)` then
    `finish()`. `finish()` flushes the buffered state.
  * Fuzzed via `fuzz/html_tokenizer` and `fuzz/html_tree_builder`.

## Status

Functional for the WHATWG happy path. Edge cases (foreign content
SVG/MathML, table-related insertion modes, template tag) need expansion;
tracked in the conformance work in roadmap P5.

## See Also

  * `docs/development/RUNBOOK-BENCH.md` for fuzz invocation
  * `docs/design/THREAT-MODEL.md` Subsystem 3 for the parser
    DoS-bound posture
