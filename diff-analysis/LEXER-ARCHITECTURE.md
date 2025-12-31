# SilkSurf JS Lexer Architecture
**Version**: 1.0
**Date**: 2025-12-30
**Status**: Design Specification (Pre-Implementation)
**Target**: Zero-copy, zero-allocation tokenization with >50K LOC/s throughput

---

## EXECUTIVE SUMMARY

**What**: A zero-copy JavaScript lexer that tokenizes source code without allocating strings.

**Why**: Eliminate Boa's 4% CPU overhead in malloc/free (identified in Phase 0 profiling).

**How**: String slices (`&'src str`) instead of owned `String` - all tokens reference original source.

**Performance Targets**:
- **Throughput**: >50,000 lines/second (benchmark: jquery-3.7.1.js, 10,276 LOC)
- **Allocations**: <100 total for 10K LOC file (arena for metadata only)
- **Memory**: <1 MB peak for 10K LOC file
- **Latency**: <200ms for 10K LOC file (cold start)

**Test262 Compliance**: 100% of lexer tests (whitespace, comments, literals, keywords, operators)

---

## 1. TOKEN REPRESENTATION

### 1.1 Token<'src> Struct

**Design**:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Token<'src> {
    pub kind: TokenKind,
    pub lexeme: &'src str,  // Zero-copy reference into source
    pub span: Span,
}
```

**Lifetime `'src`**:
- Tied to source string lifetime
- Tokens valid as long as source exists
- No allocations - pure references

**Size**: 32 bytes on 64-bit (TokenKind=4, lexeme=16, Span=12)

**Alignment**: 8-byte aligned for cache efficiency

**Example Usage**:
```rust
let source = "let x = 42;";
let lexer = Lexer::new(source);

for token in lexer {
    match token.kind {
        TokenKind::Let => println!("Keyword 'let' at {}", token.span.format()),
        TokenKind::Identifier => println!("Identifier '{}' at {}", token.lexeme, token.span.format()),
        TokenKind::Number => {
            let value: f64 = token.lexeme.parse().unwrap();
            println!("Number {} at {}", value, token.span.format());
        },
        _ => {}
    }
}
```

**Zero-Copy Guarantee**:
```rust
// GOOD: Zero-copy reference
let token = Token {
    kind: TokenKind::Identifier,
    lexeme: &source[0..5],  // Reference to "let x"
    span: Span::new(0, 5, 1, 1),
};

// BAD: Allocation (DO NOT DO THIS)
let token = Token {
    kind: TokenKind::Identifier,
    lexeme: source[0..5].to_string(),  // ❌ Allocates!
    span: Span::new(0, 5, 1, 1),
};
```

---

### 1.2 TokenKind Enum

**Complete Enumeration** (80+ variants):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]  // Optimize for size (1 byte)
pub enum TokenKind {
    // === LITERALS ===

    // Numbers
    Number,          // 123, 123.456, 1.23e10, Infinity, NaN
    BigInt,          // 123n, 0xFFn, 0o777n, 0b1010n

    // Strings
    String,          // "...", '...'
    TemplateHead,    // `...${
    TemplateMiddle,  // }...${
    TemplateTail,    // }...`
    TemplateNoSub,   // `...` (no substitutions)

    // RegExp
    RegExp,          // /pattern/flags

    // Literals (values)
    True,
    False,
    Null,
    Undefined,  // Note: 'undefined' is actually an identifier in spec, but commonly treated as literal

    // === IDENTIFIERS & KEYWORDS ===

    Identifier,  // any valid identifier not matching keywords

    // Keywords (strict mode + sloppy mode)
    Await, Break, Case, Catch, Class, Const, Continue,
    Debugger, Default, Delete, Do, Else, Enum, Export,
    Extends, Finally, For, Function, If, Import, In,
    Instanceof, Let, New, Return, Static, Super, Switch,
    This, Throw, Try, Typeof, Var, Void, While, With, Yield,

    // Contextual keywords (not reserved, but act as keywords in certain contexts)
    Async,      // async function
    Of,         // for-of loop
    From,       // import x from 'y'
    As,         // import { x as y }
    Get,        // get prop() {}
    Set,        // set prop(v) {}
    Target,     // new.target
    Meta,       // import.meta

    // Future reserved words
    Implements, Interface, Package, Private, Protected, Public,

    // === PUNCTUATORS ===

    // Grouping
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]

    // Separators
    Dot,        // .
    Comma,      // ,
    Semicolon,  // ;
    Colon,      // :

    // Operators (arithmetic)
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    StarStar,   // **

    // Operators (increment/decrement)
    PlusPlus,   // ++
    MinusMinus, // --

    // Operators (bitwise)
    Amp,        // &
    Pipe,       // |
    Caret,      // ^
    Tilde,      // ~
    LtLt,       // <<
    GtGt,       // >>
    GtGtGt,     // >>>

    // Operators (logical)
    AmpAmp,     // &&
    PipePipe,   // ||
    Not,        // !

    // Operators (comparison)
    Eq,         // =
    EqEq,       // ==
    EqEqEq,     // ===
    NotEq,      // !=
    NotEqEq,    // !==
    Lt,         // <
    LtEq,       // <=
    Gt,         // >
    GtEq,       // >=

    // Operators (assignment)
    PlusEq,     // +=
    MinusEq,    // -=
    StarEq,     // *=
    SlashEq,    // /=
    PercentEq,  // %=
    StarStarEq, // **=
    LtLtEq,     // <<=
    GtGtEq,     // >>=
    GtGtGtEq,   // >>>=
    AmpEq,      // &=
    PipeEq,     // |=
    CaretEq,    // ^=
    AmpAmpEq,   // &&=
    PipePipeEq, // ||=
    QuestionQuestionEq, // ??=

    // Operators (other)
    Question,   // ?
    QuestionDot,// ?.  (optional chaining)
    QuestionQuestion, // ?? (nullish coalescing)
    Arrow,      // =>
    Ellipsis,   // ...

    // === SPECIAL ===

    Eof,        // End of file
    Error,      // Lexer error (for error recovery)
}
```

**Size Optimization**: `#[repr(u8)]` ensures TokenKind is 1 byte (not 4-8 bytes).

**Keyword Lookup Table**:
```rust
use once_cell::sync::Lazy;
use std::collections::HashMap;

static KEYWORDS: Lazy<HashMap<&'static str, TokenKind>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("await", TokenKind::Await);
    m.insert("break", TokenKind::Break);
    m.insert("case", TokenKind::Case);
    m.insert("catch", TokenKind::Catch);
    m.insert("class", TokenKind::Class);
    m.insert("const", TokenKind::Const);
    m.insert("continue", TokenKind::Continue);
    m.insert("debugger", TokenKind::Debugger);
    m.insert("default", TokenKind::Default);
    m.insert("delete", TokenKind::Delete);
    m.insert("do", TokenKind::Do);
    m.insert("else", TokenKind::Else);
    m.insert("enum", TokenKind::Enum);
    m.insert("export", TokenKind::Export);
    m.insert("extends", TokenKind::Extends);
    m.insert("finally", TokenKind::Finally);
    m.insert("for", TokenKind::For);
    m.insert("function", TokenKind::Function);
    m.insert("if", TokenKind::If);
    m.insert("import", TokenKind::Import);
    m.insert("in", TokenKind::In);
    m.insert("instanceof", TokenKind::Instanceof);
    m.insert("let", TokenKind::Let);
    m.insert("new", TokenKind::New);
    m.insert("return", TokenKind::Return);
    m.insert("static", TokenKind::Static);
    m.insert("super", TokenKind::Super);
    m.insert("switch", TokenKind::Switch);
    m.insert("this", TokenKind::This);
    m.insert("throw", TokenKind::Throw);
    m.insert("try", TokenKind::Try);
    m.insert("typeof", TokenKind::Typeof);
    m.insert("var", TokenKind::Var);
    m.insert("void", TokenKind::Void);
    m.insert("while", TokenKind::While);
    m.insert("with", TokenKind::With);
    m.insert("yield", TokenKind::Yield);
    m.insert("async", TokenKind::Async);
    m.insert("of", TokenKind::Of);
    m.insert("null", TokenKind::Null);
    m.insert("true", TokenKind::True);
    m.insert("false", TokenKind::False);
    // ... (60+ keywords total)
    m
});

impl TokenKind {
    pub fn from_keyword(s: &str) -> Option<Self> {
        KEYWORDS.get(s).copied()
    }

    pub fn is_keyword(&self) -> bool {
        matches!(self,
            TokenKind::Await | TokenKind::Break | /* ... all keywords ... */
        )
    }
}
```

---

### 1.3 Span Struct

**Design**:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,   // Byte offset in source (0-based)
    pub end: u32,     // Byte offset in source (exclusive)
    pub line: u32,    // Line number (1-based)
    pub column: u32,  // Column number (1-based, UTF-16 code units for web compat)
}
```

**Size**: 16 bytes (4 u32 fields)

**Why u32 not usize?**:
- u32 max = 4GB source files (sufficient)
- Saves 8 bytes per Span on 64-bit (usize=8, u32=4)
- 10,000 tokens × 8 bytes = 80KB saved

**Column Counting Strategy**:
- **UTF-16 code units** (NOT Unicode codepoints, NOT bytes)
- **Rationale**: JavaScript specifies String.length in UTF-16 code units
- **Example**: '𝌆' = 2 columns (surrogate pair), 'é' = 1 column

**Implementation**:
```rust
impl Span {
    pub fn new(start: u32, end: u32, line: u32, column: u32) -> Self {
        debug_assert!(start <= end, "Invalid span: start > end");
        Self { start, end, line, column }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn format(&self) -> String {
        format!("{}:{}", self.line, self.column)
    }

    pub fn format_range(&self) -> String {
        format!("{}:{}-{}:{}", self.line, self.column, self.line, self.column + self.len())
    }

    // Merge two spans (for multi-token constructs)
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line,  // Use first span's line
            column: self.column,
        }
    }
}
```

**Error Message Example**:
```
Error: Unterminated string literal
  --> source.js:10:15
   |
10 |   let name = "John
   |              ^^^^^
```

---

## 2. LEXER STATE MACHINE

### 2.1 Lexer<'src> Struct

**Design**:
```rust
pub struct Lexer<'src> {
    source: &'src str,    // Original source (UTF-8)
    bytes: &'src [u8],    // Byte view (for fast indexing)
    pos: usize,           // Current byte position
    line: u32,            // Current line (1-based)
    column: u32,          // Current column (1-based, UTF-16 units)
    had_line_terminator: bool,  // For automatic semicolon insertion
    template_depth: u32,  // Nested template literal depth
}
```

**Invariants**:
- `pos` always on UTF-8 character boundary
- `pos <= source.len()`
- `line >= 1`, `column >= 1`
- `bytes.len() == source.len()` (same data, different view)

**Why bytes AND source?**:
- `bytes`: Fast u8 access for ASCII (most code)
- `source`: UTF-8 validation when slicing (ensure no split codepoints)

---

### 2.2 Core Methods

**Peek/Advance**:
```rust
impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
            had_line_terminator: false,
            template_depth: 0,
        }
    }

    // Get current byte without advancing
    fn current(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    // Get next byte without advancing
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    // Get byte at offset without advancing
    fn peek_n(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    // Advance by one byte, update line/column
    fn advance(&mut self) -> Option<u8> {
        let ch = self.current()?;
        self.pos += 1;

        // Update line/column
        if ch == b'\n' {
            self.line += 1;
            self.column = 1;
            self.had_line_terminator = true;
        } else {
            // UTF-16 column counting (approximate for ASCII)
            self.column += 1;
        }

        Some(ch)
    }

    // Skip while predicate true
    fn skip_while(&mut self, pred: impl Fn(u8) -> bool) {
        while let Some(ch) = self.current() {
            if !pred(ch) { break; }
            self.advance();
        }
    }

    // Get source slice (SAFE: ensures UTF-8 boundaries)
    fn slice(&self, start: usize, end: usize) -> &'src str {
        &self.source[start..end]
    }
}
```

**UTF-16 Column Tracking** (Detailed):
```rust
// Accurate UTF-16 column for non-ASCII
fn advance_unicode(&mut self) -> Option<char> {
    let ch = self.source[self.pos..].chars().next()?;
    let len = ch.len_utf8();
    self.pos += len;

    // UTF-16 code units
    let utf16_len = ch.len_utf16();
    if ch == '\n' {
        self.line += 1;
        self.column = 1;
        self.had_line_terminator = true;
    } else {
        self.column += utf16_len as u32;
    }

    Some(ch)
}
```

---

### 2.3 Tokenization State Machine

**Main Loop**:
```rust
impl<'src> Lexer<'src> {
    fn next_token(&mut self) -> Option<Token<'src>> {
        // Skip whitespace & comments
        self.skip_trivia();

        let start_pos = self.pos;
        let start_line = self.line;
        let start_column = self.column;

        let ch = self.current()?;

        let kind = match ch {
            // Whitespace (already skipped, but check)
            b' ' | b'\t' | b'\n' | b'\r' => unreachable!("whitespace should be skipped"),

            // Identifiers & keywords
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' => self.lex_identifier_or_keyword(),

            // Numbers
            b'0'..=b'9' => self.lex_number(),

            // Strings
            b'"' | b'\'' => self.lex_string(ch),

            // Template literals
            b'`' => self.lex_template_start(),

            // RegExp or division
            b'/' => self.lex_slash(),

            // Operators & punctuators
            b'(' => { self.advance(); TokenKind::LParen },
            b')' => { self.advance(); TokenKind::RParen },
            b'{' => { self.advance(); TokenKind::LBrace },
            b'}' => {
                if self.template_depth > 0 {
                    self.lex_template_middle_or_tail()
                } else {
                    self.advance();
                    TokenKind::RBrace
                }
            },
            b'[' => { self.advance(); TokenKind::LBracket },
            b']' => { self.advance(); TokenKind::RBracket },
            b'.' => self.lex_dot(),
            b',' => { self.advance(); TokenKind::Comma },
            b';' => { self.advance(); TokenKind::Semicolon },
            b':' => { self.advance(); TokenKind::Colon },
            b'?' => self.lex_question(),
            b'+' => self.lex_plus(),
            b'-' => self.lex_minus(),
            b'*' => self.lex_star(),
            b'%' => self.lex_percent(),
            b'&' => self.lex_amp(),
            b'|' => self.lex_pipe(),
            b'^' => self.lex_caret(),
            b'~' => { self.advance(); TokenKind::Tilde },
            b'!' => self.lex_not(),
            b'=' => self.lex_eq(),
            b'<' => self.lex_lt(),
            b'>' => self.lex_gt(),

            // Unexpected character
            _ => {
                self.advance();
                TokenKind::Error
            }
        };

        let end_pos = self.pos;
        let lexeme = self.slice(start_pos, end_pos);
        let span = Span::new(
            start_pos as u32,
            end_pos as u32,
            start_line,
            start_column
        );

        Some(Token { kind, lexeme, span })
    }
}
```

---

### 2.4 Whitespace & Comment Handling

**Trivia Skipping**:
```rust
impl<'src> Lexer<'src> {
    fn skip_trivia(&mut self) {
        loop {
            match self.current() {
                Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') => {
                    self.advance();
                }
                Some(b'/') if self.peek() == Some(b'/') => {
                    self.skip_line_comment();
                }
                Some(b'/') if self.peek() == Some(b'*') => {
                    self.skip_block_comment();
                }
                Some(b'<') if self.is_html_comment_start() => {
                    self.skip_html_comment();
                }
                _ => break,
            }
        }
    }

    fn skip_line_comment(&mut self) {
        // Skip //
        self.advance();
        self.advance();

        // Skip until newline or EOF
        while let Some(ch) = self.current() {
            if ch == b'\n' { break; }
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) {
        // Skip /*
        self.advance();
        self.advance();

        // Skip until */ or EOF
        loop {
            match self.current() {
                None => break,  // Unterminated comment (error, but we're lenient)
                Some(b'*') if self.peek() == Some(b'/') => {
                    self.advance();  // *
                    self.advance();  // /
                    break;
                }
                Some(_) => { self.advance(); }
            }
        }
    }

    // annexB: HTML comments <!-- ... -->
    fn is_html_comment_start(&self) -> bool {
        self.current() == Some(b'<') &&
        self.peek() == Some(b'!') &&
        self.peek_n(2) == Some(b'-') &&
        self.peek_n(3) == Some(b'-')
    }

    fn skip_html_comment(&mut self) {
        // Skip <!--
        for _ in 0..4 { self.advance(); }

        // Skip until newline (HTML comments are line-based in JS)
        while let Some(ch) = self.current() {
            if ch == b'\n' { break; }
            self.advance();
        }
    }
}
```

---

## 3. NUMBER PARSING

**All Number Formats**:
```rust
impl<'src> Lexer<'src> {
    fn lex_number(&mut self) -> TokenKind {
        let start = self.pos;

        // Check for base prefix
        if self.current() == Some(b'0') {
            match self.peek() {
                Some(b'x') | Some(b'X') => return self.lex_hex_number(),
                Some(b'o') | Some(b'O') => return self.lex_octal_number(),
                Some(b'b') | Some(b'B') => return self.lex_binary_number(),
                _ => {}
            }
        }

        // Decimal number
        self.skip_while(|ch| ch.is_ascii_digit());

        // Decimal point
        if self.current() == Some(b'.') && self.peek().map_or(false, |ch| ch.is_ascii_digit()) {
            self.advance();  // .
            self.skip_while(|ch| ch.is_ascii_digit());
        }

        // Exponent
        if matches!(self.current(), Some(b'e') | Some(b'E')) {
            self.advance();
            if matches!(self.current(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            self.skip_while(|ch| ch.is_ascii_digit());
        }

        // BigInt suffix
        if self.current() == Some(b'n') {
            self.advance();
            return TokenKind::BigInt;
        }

        TokenKind::Number
    }

    fn lex_hex_number(&mut self) -> TokenKind {
        self.advance();  // 0
        self.advance();  // x
        self.skip_while(|ch| ch.is_ascii_hexdigit());
        if self.current() == Some(b'n') {
            self.advance();
            TokenKind::BigInt
        } else {
            TokenKind::Number
        }
    }

    fn lex_octal_number(&mut self) -> TokenKind {
        self.advance();  // 0
        self.advance();  // o
        self.skip_while(|ch| matches!(ch, b'0'..=b'7'));
        if self.current() == Some(b'n') {
            self.advance();
            TokenKind::BigInt
        } else {
            TokenKind::Number
        }
    }

    fn lex_binary_number(&mut self) -> TokenKind {
        self.advance();  // 0
        self.advance();  // b
        self.skip_while(|ch| matches!(ch, b'0' | b'1'));
        if self.current() == Some(b'n') {
            self.advance();
            TokenKind::BigInt
        } else {
            TokenKind::Number
        }
    }
}
```

---

## 4. STRING PARSING

**Escape Sequences**:
```rust
impl<'src> Lexer<'src> {
    fn lex_string(&mut self, quote: u8) -> TokenKind {
        self.advance();  // Opening quote

        loop {
            match self.current() {
                None => return TokenKind::Error,  // Unterminated
                Some(ch) if ch == quote => {
                    self.advance();  // Closing quote
                    return TokenKind::String;
                }
                Some(b'\\') => {
                    self.advance();  // \
                    self.lex_escape_sequence();
                }
                Some(b'\n') | Some(b'\r') => {
                    return TokenKind::Error;  // Unescaped line terminator
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    fn lex_escape_sequence(&mut self) {
        match self.current() {
            Some(b'n') | Some(b't') | Some(b'r') | Some(b'\\') |
            Some(b'\'') | Some(b'"') | Some(b'b') | Some(b'f') |
            Some(b'v') | Some(b'0') => {
                self.advance();  // Single-char escape
            }
            Some(b'x') => {
                self.advance();  // x
                for _ in 0..2 {  // 2 hex digits
                    if self.current().map_or(false, |ch| ch.is_ascii_hexdigit()) {
                        self.advance();
                    }
                }
            }
            Some(b'u') => {
                self.advance();  // u
                if self.current() == Some(b'{') {
                    // \u{...} (Unicode code point)
                    self.advance();  // {
                    while self.current() != Some(b'}') && self.current().is_some() {
                        self.advance();
                    }
                    if self.current() == Some(b'}') {
                        self.advance();  // }
                    }
                } else {
                    // \uXXXX (4 hex digits)
                    for _ in 0..4 {
                        if self.current().map_or(false, |ch| ch.is_ascii_hexdigit()) {
                            self.advance();
                        }
                    }
                }
            }
            Some(_) => {
                self.advance();  // Unknown escape (lenient)
            }
            None => {}
        }
    }
}
```

---

## 5. TEMPLATE LITERAL PARSING

**State Machine**:
```rust
impl<'src> Lexer<'src> {
    fn lex_template_start(&mut self) -> TokenKind {
        self.advance();  // `

        loop {
            match self.current() {
                None => return TokenKind::Error,  // Unterminated
                Some(b'`') => {
                    self.advance();  // `
                    return TokenKind::TemplateNoSub;  // No ${...}
                }
                Some(b'$') if self.peek() == Some(b'{') => {
                    self.advance();  // $
                    self.advance();  // {
                    self.template_depth += 1;
                    return TokenKind::TemplateHead;
                }
                Some(b'\\') => {
                    self.advance();  // \
                    self.lex_escape_sequence();
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    fn lex_template_middle_or_tail(&mut self) -> TokenKind {
        self.advance();  // }

        loop {
            match self.current() {
                None => return TokenKind::Error,  // Unterminated
                Some(b'`') => {
                    self.advance();  // `
                    self.template_depth -= 1;
                    return TokenKind::TemplateTail;
                }
                Some(b'$') if self.peek() == Some(b'{') => {
                    self.advance();  // $
                    self.advance();  // {
                    return TokenKind::TemplateMiddle;
                }
                Some(b'\\') => {
                    self.advance();  // \
                    self.lex_escape_sequence();
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }
}
```

---

## 6. REGEXP LITERAL DISAMBIGUATION

**Critical**: `/` can mean division OR RegExp start!

**Strategy**: Context-dependent lexing
```rust
impl<'src> Lexer<'src> {
    fn lex_slash(&mut self) -> TokenKind {
        self.advance();  // /

        // Context: After what tokens can RegExp appear?
        // After: =, (, [, {, ,, ;, :, !, &, |, ^, ?, +, -, *, /, %, ~, return, throw, etc.
        // NOT after: ), ], identifier, number, string, etc.

        // For now, simple heuristic (parser will disambiguate if needed)
        if self.can_start_regexp() {
            return self.lex_regexp();
        }

        // Division or division-assignment
        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::SlashEq
        } else {
            TokenKind::Slash
        }
    }

    fn can_start_regexp(&self) -> bool {
        // Heuristic: Check if last non-trivia token was one that allows RegExp
        // (This is imperfect - parser will need to handle ambiguity)
        // For MVP, we'll lex as division and let parser re-lex if needed
        false  // Conservative: Lex as division by default
    }

    fn lex_regexp(&mut self) -> TokenKind {
        // Inside RegExp body
        let mut in_class = false;  // [...] character class

        loop {
            match self.current() {
                None => return TokenKind::Error,  // Unterminated
                Some(b'\n') | Some(b'\r') => return TokenKind::Error,  // Line terminator
                Some(b'/') if !in_class => {
                    self.advance();  // /
                    // Lex flags (g, i, m, s, u, y, d, v)
                    while let Some(ch) = self.current() {
                        if ch.is_ascii_alphabetic() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    return TokenKind::RegExp;
                }
                Some(b'[') => {
                    in_class = true;
                    self.advance();
                }
                Some(b']') if in_class => {
                    in_class = false;
                    self.advance();
                }
                Some(b'\\') => {
                    self.advance();  // \
                    if self.current().is_some() {
                        self.advance();  // Escaped char
                    }
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }
}
```

**Note**: RegExp/division disambiguation is HARD. We'll initially lex conservatively and let the parser re-lex if needed (contextual re-lexing).

---

## 7. OPERATOR PARSING

**Multi-Character Operators**:
```rust
impl<'src> Lexer<'src> {
    fn lex_plus(&mut self) -> TokenKind {
        self.advance();  // +
        match self.current() {
            Some(b'+') => { self.advance(); TokenKind::PlusPlus }
            Some(b'=') => { self.advance(); TokenKind::PlusEq }
            _ => TokenKind::Plus
        }
    }

    fn lex_minus(&mut self) -> TokenKind {
        self.advance();  // -
        match self.current() {
            Some(b'-') => { self.advance(); TokenKind::MinusMinus }
            Some(b'=') => { self.advance(); TokenKind::MinusEq }
            _ => TokenKind::Minus
        }
    }

    fn lex_star(&mut self) -> TokenKind {
        self.advance();  // *
        match self.current() {
            Some(b'*') => {
                self.advance();  // **
                if self.current() == Some(b'=') {
                    self.advance();
                    TokenKind::StarStarEq
                } else {
                    TokenKind::StarStar
                }
            }
            Some(b'=') => { self.advance(); TokenKind::StarEq }
            _ => TokenKind::Star
        }
    }

    fn lex_eq(&mut self) -> TokenKind {
        self.advance();  // =
        match self.current() {
            Some(b'=') => {
                self.advance();  // ==
                if self.current() == Some(b'=') {
                    self.advance();  // ===
                    TokenKind::EqEqEq
                } else {
                    TokenKind::EqEq
                }
            }
            Some(b'>') => { self.advance(); TokenKind::Arrow }  // =>
            _ => TokenKind::Eq
        }
    }

    fn lex_lt(&mut self) -> TokenKind {
        self.advance();  // <
        match self.current() {
            Some(b'<') => {
                self.advance();  // <<
                if self.current() == Some(b'=') {
                    self.advance();
                    TokenKind::LtLtEq
                } else {
                    TokenKind::LtLt
                }
            }
            Some(b'=') => { self.advance(); TokenKind::LtEq }
            _ => TokenKind::Lt
        }
    }

    fn lex_gt(&mut self) -> TokenKind {
        self.advance();  // >
        match self.current() {
            Some(b'>') => {
                self.advance();  // >>
                match self.current() {
                    Some(b'>') => {
                        self.advance();  // >>>
                        if self.current() == Some(b'=') {
                            self.advance();
                            TokenKind::GtGtGtEq
                        } else {
                            TokenKind::GtGtGt
                        }
                    }
                    Some(b'=') => { self.advance(); TokenKind::GtGtEq }
                    _ => TokenKind::GtGt
                }
            }
            Some(b'=') => { self.advance(); TokenKind::GtEq }
            _ => TokenKind::Gt
        }
    }

    fn lex_question(&mut self) -> TokenKind {
        self.advance();  // ?
        match self.current() {
            Some(b'?') => {
                self.advance();  // ??
                if self.current() == Some(b'=') {
                    self.advance();
                    TokenKind::QuestionQuestionEq
                } else {
                    TokenKind::QuestionQuestion
                }
            }
            Some(b'.') => {
                // ?. (optional chaining)
                // BUT: Don't lex as ?. if followed by digit (?.0 is ?. then 0)
                if self.peek().map_or(false, |ch| ch.is_ascii_digit()) {
                    TokenKind::Question
                } else {
                    self.advance();
                    TokenKind::QuestionDot
                }
            }
            _ => TokenKind::Question
        }
    }

    fn lex_dot(&mut self) -> TokenKind {
        self.advance();  // .

        // Check for ...
        if self.current() == Some(b'.') && self.peek() == Some(b'.') {
            self.advance();  // .
            self.advance();  // .
            return TokenKind::Ellipsis;
        }

        // Check for numeric literal (.123)
        if self.current().map_or(false, |ch| ch.is_ascii_digit()) {
            self.pos -= 1;  // Rewind
            return self.lex_number();
        }

        TokenKind::Dot
    }
}
```

---

## 8. ERROR RECOVERY

**Strategy**: Continue lexing after errors (don't panic)

```rust
impl<'src> Lexer<'src> {
    fn error(&mut self, kind: LexErrorKind) -> Token<'src> {
        // Log error (in production, would collect in error list)
        eprintln!("Lexer error: {:?} at {}:{}", kind, self.line, self.column);

        // Skip to next safe point (whitespace or punctuator)
        while let Some(ch) = self.current() {
            if ch.is_ascii_whitespace() || is_punctuator(ch) {
                break;
            }
            self.advance();
        }

        // Return error token (parser will handle)
        Token {
            kind: TokenKind::Error,
            lexeme: "",
            span: Span::new(self.pos as u32, self.pos as u32, self.line, self.column),
        }
    }
}

#[derive(Debug)]
enum LexErrorKind {
    UnterminatedString,
    UnterminatedTemplate,
    UnterminatedRegExp,
    UnterminatedBlockComment,
    InvalidNumber,
    UnexpectedCharacter(char),
}
```

---

## 9. TEST262 COMPLIANCE STRATEGY

**Test Categories**:
1. **Whitespace**: space, tab, LF, CR, LS, PS, ZWNBSP
2. **Comments**: single-line, multi-line, HTML comments
3. **Identifiers**: ASCII, Unicode, keywords
4. **Numbers**: decimal, hex, octal, binary, BigInt, edge cases
5. **Strings**: single, double, escape sequences, Unicode
6. **Templates**: substitution, nesting, edge cases
7. **RegExp**: patterns, flags, edge cases
8. **Operators**: all operators, precedence
9. **Automatic Semicolon Insertion**: line terminators

**Test Integration**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whitespace() {
        let source = "  \t\n\r  ";
        let mut lexer = Lexer::new(source);
        assert_eq!(lexer.next(), Some(Token { kind: TokenKind::Eof, .. }));
    }

    #[test]
    fn test_numbers() {
        assert_lex("123", TokenKind::Number, "123");
        assert_lex("123.456", TokenKind::Number, "123.456");
        assert_lex("0x1A", TokenKind::Number, "0x1A");
        assert_lex("0o755", TokenKind::Number, "0o755");
        assert_lex("0b1010", TokenKind::Number, "0b1010");
        assert_lex("123n", TokenKind::BigInt, "123n");
        assert_lex("1.23e10", TokenKind::Number, "1.23e10");
    }

    fn assert_lex(source: &str, kind: TokenKind, lexeme: &str) {
        let mut lexer = Lexer::new(source);
        let token = lexer.next().unwrap();
        assert_eq!(token.kind, kind);
        assert_eq!(token.lexeme, lexeme);
    }
}
```

**Test262 Runner** (Week 2 Day 1):
```rust
// tests/test262_lexer.rs

use std::fs;
use std::path::Path;

#[test]
fn test262_lexer_suite() {
    let test_dir = Path::new("test262/test/language/lexical-grammar");
    let mut passed = 0;
    let mut failed = 0;

    for entry in fs::read_dir(test_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |ext| ext == "js") {
            let source = fs::read_to_string(&path).unwrap();

            match run_lexer_test(&source) {
                Ok(_) => passed += 1,
                Err(e) => {
                    failed += 1;
                    eprintln!("FAIL: {:?}: {}", path, e);
                }
            }
        }
    }

    println!("Test262 lexer: {} passed, {} failed", passed, failed);
    assert_eq!(failed, 0, "Some Test262 lexer tests failed");
}

fn run_lexer_test(source: &str) -> Result<(), String> {
    let mut lexer = Lexer::new(source);

    // Lex all tokens
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next() {
        if token.kind == TokenKind::Error {
            return Err("Lexer error".into());
        }
        tokens.push(token);
        if token.kind == TokenKind::Eof {
            break;
        }
    }

    Ok(())
}
```

---

## 10. PERFORMANCE TARGETS

**Benchmarks**:
```rust
// benches/lexer_bench.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use silksurf_js_lexer::Lexer;
use std::fs;

fn bench_lexer(c: &mut Criterion) {
    let jquery = fs::read_to_string("benches/jquery-3.7.1.js").unwrap();
    let lines = jquery.lines().count();

    let mut group = c.benchmark_group("lexer");
    group.throughput(Throughput::Elements(lines as u64));

    group.bench_function("jquery-3.7.1.js", |b| {
        b.iter(|| {
            let mut lexer = Lexer::new(black_box(&jquery));
            let mut count = 0;
            while lexer.next().is_some() {
                count += 1;
            }
            count
        });
    });

    group.finish();
}

criterion_group!(benches, bench_lexer);
criterion_main!(benches);
```

**Expected Results**:
- **jquery-3.7.1.js**: 10,276 LOC → <200ms (<0.02ms/line)
- **Throughput**: >50,000 LOC/s
- **Allocations**: <100 (measured with heaptrack)

---

## 11. DELIVERABLES

**Phase 2 Day 7** (End of Week):
1. ✅ Complete Lexer<'src> implementation
2. ✅ All 80+ TokenKind variants handled
3. ✅ Zero-copy tokenization (lexeme = &'src str)
4. ✅ Test262 lexer tests passing (100%)
5. ✅ Benchmarks: >50K LOC/s
6. ✅ Allocations: <100 for 10K LOC
7. ✅ Memory: <1 MB peak for 10K LOC

---

**Next**: PARSER-ARCHITECTURE.md (arena-allocated AST design)
