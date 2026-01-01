# Fuzzing

## Setup
```
cargo install cargo-fuzz
```

## Targets
- `html_tokenizer`
- `html_tree_builder`
- `css_tokenizer`
- `css_parser`
- `js_runtime`

## Example
```
cd fuzz
cargo fuzz run html_tokenizer
```
