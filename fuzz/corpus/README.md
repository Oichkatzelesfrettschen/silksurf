# Fuzz Seed Corpus

Seed inputs for the libfuzzer-sys harnesses under `../fuzz_targets/`.
The runtime corpus (libfuzzer's working set, with crash inputs and
auto-discovered new coverage) lives elsewhere -- by default at
`../fuzz/corpus/<target>/` after `cargo fuzz run`. This directory holds
the **committed seed corpus** that the harness starts from.

## Targets

  * `html_tokenizer/` -- minimal HTML inputs covering tag forms, attribute
    forms, comments, doctypes, entities, and a few invalid-byte cases.
  * `html_parse/` -- well-formed and edge-case documents that
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
    arrow functions, generators, plus 100 `test262_NNN.js` cases carried
    over from the retired AFL parser corpus. Those are procedurally
    generated test262 sources covering destructuring binding, async
    generators, class expressions, and default parameters -- syntax the
    hand-authored seeds do not reach.

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
better with diverse short seeds. `.gitignore` tracks exactly the
`NN_*.*`, `test262_NNN.*`, and `README.md` names, so a seed outside
those forms stays untracked alongside libfuzzer's runtime additions.

When a previously-uncovered code path is found via fuzzing, copy the
discovered minimised input from `fuzz/corpus/<target>/` into here so
the next run starts already covering it.

## Corpus size

The HTML and CSS targets carry roughly 17 to 20 hand-authored seeds each,
the bootstrap baseline. `js_runtime/` carries 120: the 20 hand-authored
plus 100 test262-derived cases.

Further expansion draws on the upstream corpora
`scripts/fetch_html_css_test_corpora.sh` clones into the untracked
`silksurf-extras/` tree. Those checkouts move to upstream HEAD on every
fetch, so a harvested seed is copied here rather than referenced there.
