# silksurf-html

HTML tree construction over html5ever, plus an auxiliary tokenizer for tooling.

## Public API

  * `parse_html` -- parses a document into a `silksurf_dom::Dom` through
    html5ever's `TreeSink`. This is the production path; `silksurf-engine`
    imports it as `html5ever_parse`.
  * `parse_fragment_into` -- parses a fragment in a context element's
    insertion mode and splices the nodes under a live parent, matching
    innerHTML semantics. Scripts in the fragment stay inert.
  * `treesink::SilkDomBuilder` -- the `TreeSink` implementation. Its `finish`
    calls `Dom::materialize_resolve_table`, the initial materialization
    boundary `silksurf-dom` and `silksurf-css` both build against.
  * `Tokenizer` -- streaming tokenizer with `feed` / `finish`. It serves
    tooling rather than page loads: `wpt_runner` and the `silksurf-css`
    harness use it to lift `<style>` contents out of markup.
  * `Token` -- tokenizer output enum (StartTag, EndTag, Comment, Doctype,
    Character, Eof).
  * `TokenizeError` -- crate-local error; `From<TokenizeError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.

## Conventions

  * Errors have `state`, `offset`, and `message` so a caller can render
    "syntax error at byte 1234 in BeforeAttributeName" diagnostics.
  * The tokenizer accepts streaming bytes; call `feed(chunk)` then
    `finish()`. `finish()` flushes the buffered state.
  * Fuzzed via `fuzz/fuzz_targets/html_parse.rs`, which drives the production
    path, and `fuzz/fuzz_targets/html_tokenizer.rs`.

## Measured conformance

  * Tree construction over WPT `html/syntax/parsing/resources`: 1,440 / 1,726
    executed = 83.43%. Recorded gaps live in
    `tests/html5lib-tree-construction.expectations`; template content and
    processing instructions lead them, both because `silksurf_dom::NodeKind`
    carries no matching variant.
  * Tokenization over the html5lib tokenizer corpus: 3,019 / 6,640 executed =
    45.47%. The `State` enum carries 8 states against roughly 80 in the
    standard, and named character references stay unresolved. Recorded gaps
    live in `tests/html5lib-tokenizer.expectations`.

Both harnesses skip with a reason when their corpus is absent and fail when an
operator names a corpus path that does not resolve. Run
`scripts/fetch_html_css_test_corpora.sh` to populate the extras tree.

## See Also

  * `docs/conformance/SCORECARD.md` for the aggregated harness dashboard
  * `docs/development/RUNBOOK-BENCH.md` for fuzz invocation
  * `docs/design/THREAT-MODEL.md` Subsystem 3 for the parser
    DoS-bound posture
