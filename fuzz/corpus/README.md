# Fuzz Seed Corpus

Seed inputs for the libfuzzer-sys harnesses under `../fuzz_targets/`.
The runtime corpus (libfuzzer's working set, with crash inputs and
auto-discovered new coverage) lives elsewhere -- by default at
`../fuzz/corpus/<target>/` after `cargo fuzz run`. This directory holds
the **committed seed corpus** that the harness starts from.

## Targets

  * `html_tokenizer/` -- minimal HTML inputs covering tag forms, attribute
    forms, comments, doctypes, entities, and a few invalid-byte cases.
  * `html_tree_builder/` -- well-formed and edge-case documents that
    exercise insertion modes (table, void elements, foreign content,
    template).
  * `css_tokenizer/` -- bare rules, classes, ids, combinators, attribute
    selectors, pseudo-classes, comments, at-rules, hex colors, function
    calls, calc, escapes.
  * `css_parser/` -- compound selectors, combinator chains, :not, at-rules
    (media, supports, keyframes, font-face), custom properties,
    !important.
  * `js_runtime/` -- ES5 + ES6 syntax samples covering var/let/const,
    functions, arrays, objects, control flow, classes, template literals,
    arrow functions, generators.

## Running

```sh
FUZZ=1 scripts/local_gate.sh full          # 30s per target via local-gate
cargo +nightly fuzz run html_tokenizer     # iterate one target
cargo +nightly fuzz run html_tokenizer -- -max_total_time=300
                                            # 5 minutes per target
```

## Adding seeds

A seed is just a file containing the input bytes. Pick a name that
sorts naturally (`NN_short_description.ext`) and prefer many small
seeds (under 1 KB each) over a few large ones -- libfuzzer mutates
better with diverse short seeds.

When a previously-uncovered code path is found via fuzzing, copy the
discovered minimised input from `fuzz/corpus/<target>/` into here so
the next run starts already covering it.

## Why the corpus is small

The current seed counts (~17-20 per target) are the bootstrap baseline.
Expansion is queued in SNAZZY-WAFFLE roadmap P3.S1 -- harvesting from
html5lib-tests, web-platform-tests, and the test262 corpus once those
are vendored.
