# Browser Analysis Tools Audit
Generated: 2025-12-30

## Already Installed Tools

###Code Metrics & Counting
- `tokei` 14.0.0 - Fast code counter with JSON/YAML/CBOR export
- `cloc` 2.06 - Count Lines of Code
- `sloccount` 2.26 - Physical source lines counter
- `scc` 3.6.0 - Sloc Cloc and Code (very fast, Go-based)

### Static Analysis (C/C++)
- `cppcheck` 2.19.0 - C/C++ static analyzer
- `sparse` 0.6.4 - Semantic parser for C
- `splint` 3.1.2 - Security vulnerabilities checker
- `include-what-you-use` 0.25 - #include analyzer

### Code Navigation & Indexing
- `cscope` 15.9-3.2 - Code browsing tool
- `ctags` (Universal Ctags 6.2.1) - Code tagging system

### AST & Parsers
- `tree-sitter` 0.25.10 - Incremental parsing library
- `tree-sitter-c` 0.24.1 - C grammar
- `tree-sitter-lua` 0.4.0 - Lua grammar
- `tree-sitter-markdown` 0.5.1 - Markdown grammar
- `tree-sitter-query` 0.7.0 - TS query grammar
- `tree-sitter-vim` 0.7.0 - Vimscript grammar
- `tree-sitter-vimdoc` 4.0.0 - Vim help file grammar

### Rust Tooling
- `rust-analyzer` 20251208 - Rust LSP server
- `rustup` 1.28.2 - Rust toolchain installer

### Graph & Diagram Generation
- `graphviz` 14.0.5 - Graph visualization
- `python-graphviz` 0.21 - Python interface
- `python-pygraphviz` 1.14 - Python interface
- `python-pydot` 3.0.4 - DOT language interface
- `mermaid-cli` 11.12.0 - Mermaid diagram generator
- `ditaa` 0.11.0 - ASCII art to bitmap converter
- `asciidoctor` 2.0.26 - AsciiDoc processor

### Diff Tools
- `difftastic` - Syntax-aware diff
- `diffoscope` - Deep file comparison
- `meld` - Graphical diff/merge
- `delta` - Git diff pager
- `diff-so-fancy` - Git diff enhancer

## Tools to Install

### Critical Additions
1. `doxygen` - C/C++ documentation generator
2. `plantuml` - UML diagram generator
3. `tree-sitter-cli` - Tree-sitter development tool
4. `tree-sitter-javascript` - JavaScript grammar
5. `tree-sitter-rust` - Rust grammar
6. `tree-sitter-python` - Python grammar
7. `tree-sitter-bash` - Bash grammar
8. `ccls` - C/C++/ObjC LSP server (enhanced navigation)
9. `cargo-flamegraph` - Flamegraph for Rust
10. `cargo-depgraph` - Dependency graph generator

### Advanced Tools (chaotic-aur/blackarch)
11. `sourcetrail` (chaotic-aur) - Interactive source explorer
12. `cflow` (blackarch) - C call graph generator

### Profiling & Performance
13. `inferno` - Rust FlameGraph port
14. `valgrind` (check if installed)
15. `perf` (Linux perf tools)

## Installation Strategy

### Phase 1: Core Analysis Tools
```bash
sudo pacman -S doxygen plantuml tree-sitter-cli tree-sitter-javascript tree-sitter-rust tree-sitter-python tree-sitter-bash ccls cargo-flamegraph cargo-depgraph
```

### Phase 2: Advanced Tools
```bash
sudo pacman -S sourcetrail  # from chaotic-aur
yay -S cflow  # from blackarch if enabled
```

### Phase 3: Profiling Tools
```bash
sudo pacman -S inferno valgrind linux-tools-meta  # or perf package
```

## Tool Capabilities Matrix

| Tool | C/C++ | Rust | JavaScript | Call Graphs | Metrics | Diagrams |
|------|-------|------|------------|-------------|---------|----------|
| cscope | ✓ | - | - | ✓ | - | - |
| ctags | ✓ | ✓ | ✓ | - | - | - |
| ccls | ✓ | - | - | ✓ | - | - |
| tree-sitter | ✓ | ✓ | ✓ | - | - | - |
| cppcheck | ✓ | - | - | - | - | - |
| rust-analyzer | - | ✓ | - | ✓ | - | - |
| doxygen | ✓ | - | - | - | - | ✓ |
| graphviz | ALL | ALL | ALL | ✓ | - | ✓ |
| mermaid | ALL | ALL | ALL | - | - | ✓ |
| tokei | ALL | ALL | ALL | - | ✓ | - |
| sourcetrail | ✓ | - | ✓ | ✓ | - | ✓ |
| difftastic | ✓ | ✓ | ✓ | - | - | - |

## Browser Codebase Compatibility

### NetSurf/NeoSurf (C)
- cscope, ctags, ccls, sparse, cppcheck, splint
- doxygen for docs
- cflow for call graphs
- graphviz for architecture diagrams

### Servo (Rust)
- rust-analyzer, rustup
- tokei, cargo-depgraph, cargo-flamegraph
- tree-sitter-rust
- rustdoc (built-in)

### Ladybird (C++)
- ccls, cscope, ctags
- cppcheck, include-what-you-use
- doxygen
- graphviz

### Text Browsers (C)
- Same as NetSurf
- Simpler, smaller codebases - easier analysis

### Mixed/Other
- Dillo (C++): C++ toolchain
- Sciter (C++): C++ toolchain
- Amaya (C): C toolchain
