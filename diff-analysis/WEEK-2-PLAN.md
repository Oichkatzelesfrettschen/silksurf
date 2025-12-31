# Week 2 Plan - Cleanroom Lexer Implementation
**Date**: 2025-12-30 to 2026-01-05
**Goal**: Zero-copy lexer with >50K LOC/s throughput and zero allocations
**Deliverable**: Production-ready lexer passing all Test262 lexer tests

---

## EXECUTIVE SUMMARY

**What We're Building**: A zero-copy JavaScript lexer that is:
- **FASTER** than Boa's lexer (target: >50K LOC/s vs Boa's unknown)
- **ZERO allocations** during tokenization (arena-based)
- **ZERO memory leaks** (automatic arena cleanup)
- **100% Test262 lexer compliance** (all edge cases handled)

**Why This Week Matters**:
- Lexer is the **foundation** - every other component depends on it
- Zero-copy design **eliminates** 4% CPU overhead from malloc/free (identified in Phase 0)
- Arena allocation **prevents** 8.5% memory leak rate (identified in Phase 0 fuzzing)

**Success Criteria**:
1. ✅ Lexer tokenizes jquery-3.7.1.js (10K LOC) with <100 allocations
2. ✅ Throughput >50K LOC/s (measured with criterion)
3. ✅ Zero memory leaks (validated with heaptrack)
4. ✅ Passes all Test262 lexer tests (whitespace, comments, literals, etc.)

---

## DAY 1: Arena Allocator Research & Design (2025-12-30)

**Goal**: Select arena allocator and design lifetime strategy

### Morning (4 hours): Arena Allocator Research

**Task 1.1**: Research bumpalo crate (1 hour)
```bash
cd /home/eirikr/Github/silksurf/
mkdir -p arena-proto
cd arena-proto
cargo new arena-benchmark --bin
cd arena-benchmark
cargo add bumpalo
```

**What to evaluate**:
- API ergonomics (how easy to use)
- Performance (bump allocation speed)
- Lifetime management (how it integrates with Rust borrow checker)
- Reset mechanism (can we reuse arenas?)

**Example usage**:
```rust
use bumpalo::Bump;

fn test_bumpalo() {
    let arena = Bump::new();

    // Allocate string in arena
    let s: &str = arena.alloc_str("hello");

    // Allocate struct in arena
    let node: &Node = arena.alloc(Node { value: 42 });

    // Arena freed when dropped (automatic cleanup)
}
```

**Task 1.2**: Research typed-arena crate (1 hour)
```bash
cargo add typed-arena
```

**What to evaluate**:
- Type safety (typed-arena enforces single type per arena)
- Performance vs bumpalo
- API simplicity

**Example usage**:
```rust
use typed_arena::Arena;

fn test_typed_arena() {
    let arena: Arena<Token> = Arena::new();

    // Allocate token in arena
    let token: &Token = arena.alloc(Token { kind: TokenKind::Number });

    // Type-safe: Can only allocate Token in this arena
}
```

**Task 1.3**: Prototype simple arena-allocated AST (1 hour)
```rust
// arena-proto/src/main.rs

use bumpalo::Bump;

#[derive(Debug)]
enum Expr<'arena> {
    Number(i32),
    Add(&'arena Expr<'arena>, &'arena Expr<'arena>),
}

impl<'arena> Expr<'arena> {
    fn eval(&self) -> i32 {
        match self {
            Expr::Number(n) => *n,
            Expr::Add(left, right) => left.eval() + right.eval(),
        }
    }
}

fn main() {
    let arena = Bump::new();

    // Build AST: 1 + (2 + 3)
    let two = arena.alloc(Expr::Number(2));
    let three = arena.alloc(Expr::Number(3));
    let add_23 = arena.alloc(Expr::Add(two, three));
    let one = arena.alloc(Expr::Number(1));
    let expr = arena.alloc(Expr::Add(one, add_23));

    println!("Result: {}", expr.eval());  // 6

    // Arena automatically freed when dropped - zero manual deallocation!
}
```

**Task 1.4**: Benchmark arena vs Box allocation (1 hour)
```rust
// Benchmark: Allocate 1M AST nodes

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use bumpalo::Bump;

fn bench_arena(c: &mut Criterion) {
    c.bench_function("arena 1M nodes", |b| {
        b.iter(|| {
            let arena = Bump::new();
            for i in 0..1_000_000 {
                let node = arena.alloc(Expr::Number(i));
                black_box(node);
            }
            // Arena dropped here - single deallocation for 1M nodes!
        });
    });
}

fn bench_box(c: &mut Criterion) {
    c.bench_function("box 1M nodes", |b| {
        b.iter(|| {
            for i in 0..1_000_000 {
                let node = Box::new(Expr::Number(i));
                black_box(node);
                // Box dropped here - 1M individual deallocations!
            }
        });
    });
}

criterion_group!(benches, bench_arena, bench_box);
criterion_main!(benches);
```

**Expected Results**:
- Arena: ~10ms for 1M allocations (single bulk allocation)
- Box: ~100ms for 1M allocations (1M individual malloc/free calls)
- **Arena is 10x faster**

### Afternoon (4 hours): Lifetime Strategy Design

**Task 1.5**: Design arena lifetime strategy (2 hours)

**Critical Decision**: When to allocate arenas and when to drop them?

**Strategy 1 - Compilation Arena** (RECOMMENDED):
```rust
struct CompilationUnit<'src> {
    source: &'src str,
    arena: Bump,  // Lives for entire compilation
}

impl<'src> CompilationUnit<'src> {
    fn new(source: &'src str) -> Self {
        Self { source, arena: Bump::new() }
    }

    fn lex(&self) -> Vec<Token<'src>> {
        // Tokens reference source (zero-copy)
        // No arena allocations during lexing!
        Lexer::new(self.source).collect()
    }

    fn parse(&self) -> &Ast<'src> {
        // AST nodes allocated in arena
        // Lifetime tied to CompilationUnit
        let tokens = self.lex();
        Parser::new(&self.arena, tokens).parse()
    }
}

// Arena dropped when CompilationUnit dropped
// All AST nodes freed in single operation!
```

**Strategy 2 - Per-Function Arena** (for runtime):
```rust
struct FunctionActivation<'arena> {
    arena: &'arena Bump,
    locals: HashMap<&'arena str, Value>,
}

// When function returns, arena can be reset
// Temporaries freed in bulk
```

**Task 1.6**: Document arena design decisions (2 hours)

Create `arena-design.md`:
```markdown
# Arena Allocation Strategy

## Decision: Use bumpalo

**Rationale**:
- Performance: 10x faster than Box for 1M allocations
- Flexibility: Supports mixed types in single arena
- Reset: Can reset and reuse arenas
- API: Simple and ergonomic

## Lifetime Strategy

### Compilation Phase
- **One arena per compilation unit** (lexer + parser + compiler)
- Arena lives until bytecode emitted
- Zero manual deallocation - arena handles cleanup

### Runtime Phase (Future)
- **Young-gen arena** for temporaries (reset after GC)
- **Old-gen tracing GC** for long-lived objects
- Hybrid approach minimizes GC overhead

## Benefits vs Boa

### Boa Issues (from Phase 0):
- 8.5% allocation leak rate
- 4% CPU in malloc/free
- Manual string lifecycle (skip_interning bugs)

### Arena Solution:
- 0% leak rate (automatic cleanup)
- 0% malloc overhead (bump allocation)
- Automatic string lifecycle (scoped to arena)
```

---

## DAY 2: Lexer Architecture Design (2025-12-31)

**Goal**: Design zero-copy token representation

### Morning (4 hours): Core Type Design

**Task 2.1**: Design Token<'src> struct (1 hour)
```rust
// silksurf-js/crates/lexer/src/token.rs

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Token<'src> {
    pub kind: TokenKind,
    pub lexeme: &'src str,  // Zero-copy reference into source
    pub span: Span,
}

impl<'src> Token<'src> {
    pub fn new(kind: TokenKind, lexeme: &'src str, span: Span) -> Self {
        Self { kind, lexeme, span }
    }

    // Utility methods
    pub fn is_keyword(&self) -> bool {
        matches!(self.kind, TokenKind::If | TokenKind::Else | /* ... */)
    }

    pub fn as_number(&self) -> Option<f64> {
        if self.kind == TokenKind::Number {
            self.lexeme.parse().ok()
        } else {
            None
        }
    }
}
```

**Task 2.2**: Design TokenKind enum (2 hours)
```rust
// Complete enumeration of all JavaScript token types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Literals
    Number,        // 123, 123.456, 0x1A, 0o755, 0b1010, 123n
    String,        // "...", '...', `...`
    TemplateHead,  // `...${
    TemplateMiddle,// }...${
    TemplateTail,  // }...`
    RegExp,        // /pattern/flags

    // Identifiers & Keywords
    Identifier,
    If, Else, While, For, Function, Return, Let, Const, Var,
    Class, Extends, Static, Async, Await, Yield, Import, Export,
    New, This, Super, Typeof, Void, Delete, In, Of, Instanceof,
    Try, Catch, Finally, Throw, Break, Continue, Switch, Case,
    Default, With, Debugger,

    // Literals (special)
    True, False, Null, Undefined,

    // Punctuators
    LParen, RParen,        // ( )
    LBrace, RBrace,        // { }
    LBracket, RBracket,    // [ ]
    Dot, Comma, Semicolon, // . , ;
    Colon, Question,       // : ?
    Arrow,                 // =>
    Ellipsis,              // ...

    // Operators
    Plus, Minus, Star, Slash, Percent,           // + - * / %
    StarStar,                                     // **
    Eq, EqEq, EqEqEq,                            // = == ===
    NotEq, NotEqEq,                              // != !==
    Lt, LtEq, Gt, GtEq,                          // < <= > >=
    LtLt, GtGt, GtGtGt,                          // << >> >>>
    Amp, AmpAmp, Pipe, PipePipe,                 // & && | ||
    Caret, Tilde, Not,                           // ^ ~ !
    PlusEq, MinusEq, StarEq, SlashEq, PercentEq, // += -= *= /= %=
    StarStarEq, LtLtEq, GtGtEq, GtGtGtEq,       // **= <<= >>= >>>=
    AmpEq, PipeEq, CaretEq,                      // &= |= ^=
    AmpAmpEq, PipePipeEq,                        // &&= ||=
    QuestionQuestion, QuestionQuestionEq,         // ?? ??=
    OptionalChaining,                             // ?.
    PlusPlus, MinusMinus,                         // ++ --

    // Special
    Eof,
}

impl TokenKind {
    pub fn from_keyword(s: &str) -> Option<Self> {
        Some(match s {
            "if" => Self::If,
            "else" => Self::Else,
            "while" => Self::While,
            // ... all 60+ keywords
            _ => return None,
        })
    }
}
```

**Task 2.3**: Design Span struct (1 hour)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,  // Byte offset in source
    pub end: usize,
    pub line: u32,     // Line number (1-based)
    pub column: u32,   // Column number (1-based)
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, column: u32) -> Self {
        Self { start, end, line, column }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    // For error messages
    pub fn format(&self) -> String {
        format!("{}:{}", self.line, self.column)
    }
}
```

### Afternoon (4 hours): Lexer State Machine Design

**Task 2.4**: Design Lexer<'src> struct (2 hours)
```rust
pub struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
    line: u32,
    column: u32,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    // Core methods
    fn current(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.current()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn skip_while(&mut self, pred: impl Fn(u8) -> bool) {
        while let Some(ch) = self.current() {
            if !pred(ch) { break; }
            self.advance();
        }
    }

    fn slice(&self, start: usize, end: usize) -> &'src str {
        &self.source[start..end]
    }
}

impl<'src> Iterator for Lexer<'src> {
    type Item = Token<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}
```

**Task 2.5**: Design error recovery strategy (1 hour)
```rust
// Strategy: On error, skip to next safe point and continue

pub enum LexError {
    UnterminatedString { span: Span },
    InvalidNumber { span: Span },
    UnexpectedChar { ch: char, span: Span },
}

impl<'src> Lexer<'src> {
    fn error(&mut self, kind: LexError) -> Token<'src> {
        eprintln!("Lexer error: {:?}", kind);

        // Skip to next whitespace or punctuator
        self.skip_while(|ch| !ch.is_ascii_whitespace() && !is_punctuator(ch));

        // Return error token (parser will handle)
        Token::new(TokenKind::Error, "", Span::default())
    }
}
```

**Task 2.6**: Create lexer-design.md specification (1 hour)

Document complete lexer design with examples for every token type.

---

## DAY 3-5: Lexer Implementation (Core Features)

*[Continuing with remaining days...]*

**Total Week 2 Tasks**: 150+ atomic tasks
**Estimated Completion**: Day 7 (2026-01-05)
**Success Metric**: Lexer passes all Test262 lexer tests

---

## WEEK 2 DELIVERABLES

1. **Arena allocator decision** (bumpalo vs typed-arena)
2. **Arena benchmark results** (10x faster than Box)
3. **Lexer architecture document** (complete design)
4. **Production lexer implementation** (all token types)
5. **Test suite** (100+ unit tests)
6. **Benchmarks** (>50K LOC/s throughput)
7. **Validation** (zero leaks, zero allocations)

---

**Next**: Week 3 - RegExp implementation (166 Test262 tests)
