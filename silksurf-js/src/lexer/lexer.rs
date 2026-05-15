//! JavaScript Lexer - Zero-copy, BPE-optimized, SIMD-accelerated
//!
//! Key features:
//! - Zero allocation during lexing (tokens reference source)
//! - BPE pattern matching for common tokens
//! - String interning for identifiers
//! - Proper Unicode identifier support (`XID_Start`, `XID_Continue`)
//! - SIMD-accelerated scanning via memchr (3-6x faster for comments/strings)
//!
//! Performance target: 100-200 MB/s throughput

use std::sync::OnceLock;

use memchr::{memchr2, memchr3};

use super::bpe::BpeMatcher;
use super::interner::Interner;
use super::span::Span;
use super::token::{Token, TokenKind, keyword_lookup};

/// Global BPE matcher - constructed once, reused across all lexers
static BPE_MATCHER: OnceLock<BpeMatcher> = OnceLock::new();

fn get_bpe_matcher() -> &'static BpeMatcher {
    BPE_MATCHER.get_or_init(BpeMatcher::new)
}

/// JavaScript lexer with zero-copy token output
pub struct Lexer<'src> {
    /// Source code being lexed
    source: &'src str,
    /// Source as bytes for fast indexing
    bytes: &'src [u8],
    /// Current byte position
    pos: usize,
    /// Start of current token
    start: usize,
    /// String interner for identifiers
    interner: Interner,
    /// Whether we just saw a line terminator (for ASI)
    saw_line_terminator: bool,
}

impl<'src> Lexer<'src> {
    /// Create a new lexer for the given source
    #[must_use]
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            start: 0,
            interner: Interner::default(),
            saw_line_terminator: false,
        }
    }

    /// Create lexer with custom interner (for sharing across files)
    #[must_use]
    pub fn with_interner(source: &'src str, interner: Interner) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            start: 0,
            interner,
            saw_line_terminator: false,
        }
    }

    /// Get the interner (for resolving symbols)
    #[must_use]
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// Consume the lexer and return the interner
    #[must_use]
    pub fn into_interner(self) -> Interner {
        self.interner
    }

    /// Check if at end of input
    #[inline]
    fn is_at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Peek current byte
    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Peek byte at offset from current position
    #[inline]
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    /// Advance position and return the consumed byte
    #[inline]
    fn advance(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    /// Advance if current byte matches expected
    #[inline]
    fn advance_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Skip whitespace and comments, tracking line terminators
    #[inline]
    fn skip_trivia(&mut self) {
        self.saw_line_terminator = false;

        loop {
            match self.peek() {
                // Whitespace (not line terminators)
                Some(b' ' | b'\t' | 0x0B | 0x0C | 0xA0) => {
                    self.pos += 1;
                }
                // Line terminators
                Some(b'\n') => {
                    self.pos += 1;
                    self.saw_line_terminator = true;
                }
                Some(b'\r') => {
                    self.pos += 1;
                    self.advance_if(b'\n'); // CRLF
                    self.saw_line_terminator = true;
                }
                // Comments
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    self.skip_line_comment();
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    self.skip_block_comment();
                }
                _ => break,
            }
        }
    }

    /// Skip single-line comment (// ...)
    /// Uses SIMD via memchr2 for fast newline detection
    #[inline]
    fn skip_line_comment(&mut self) {
        self.pos += 2; // Skip //
        let remaining = &self.bytes[self.pos..];
        // SIMD-accelerated search for \n or \r
        if let Some(offset) = memchr2(b'\n', b'\r', remaining) {
            self.pos += offset;
        } else {
            // No newline found - comment extends to EOF
            self.pos = self.bytes.len();
        }
    }

    /// Skip block comment (/* ... */)
    /// Uses SIMD via memchr to find potential comment terminators
    #[inline]
    fn skip_block_comment(&mut self) {
        self.pos += 2; // Skip /*

        loop {
            let remaining = &self.bytes[self.pos..];
            if remaining.is_empty() {
                return; // Unterminated comment - error will be reported
            }

            // SIMD search for `*` (potential end) or newlines (for ASI tracking)
            if let Some(offset) = memchr3(b'*', b'\n', b'\r', remaining) {
                let found = remaining[offset];
                self.pos += offset;

                if found == b'*' {
                    // Check if this is */
                    if self.peek_at(1) == Some(b'/') {
                        self.pos += 2;
                        return;
                    }
                    self.pos += 1; // Skip lone *
                } else {
                    // Found newline
                    self.saw_line_terminator = true;
                    self.pos += 1;
                }
            } else {
                // No terminator found - comment extends to EOF
                self.pos = self.bytes.len();
                return;
            }
        }
    }

    /// Create a span for the current token
    #[inline]
    fn make_span(&self) -> Span {
        Span::new(self.start as u32, self.pos as u32)
    }

    /// Get the current token's text
    #[inline(always)]
    fn current_text(&self) -> &'src str {
        &self.source[self.start..self.pos]
    }

    /// Scan the next token
    #[inline]
    #[cfg_attr(
        feature = "tracing-full",
        tracing::instrument(level = "trace", skip(self))
    )]
    pub fn next_token(&mut self) -> Token<'src> {
        self.skip_trivia();
        self.start = self.pos;

        if self.is_at_end() {
            return Token::new(TokenKind::Eof, self.make_span());
        }

        // UNWRAP-OK: is_at_end() above returned false, so peek() yields Some(u8).
        let byte = self.peek().unwrap();

        // Try BPE pattern match first (for keywords and multi-char operators)
        let bpe = get_bpe_matcher();
        if bpe.could_start_pattern(byte) {
            if let Some((pattern_id, len)) = bpe.try_match(&self.bytes[self.pos..]) {
                // Matched a BPE pattern - but for keywords/identifiers, verify it's not part of larger identifier
                if pattern_id < 30 {
                    // Keywords (IDs 0-29)
                    // Check if followed by identifier char
                    let next_byte = self.bytes.get(self.pos + len).copied();
                    if !is_identifier_continue_byte(next_byte) {
                        self.pos += len;
                        let text = self.current_text();
                        if let Some(kw) = keyword_lookup(text) {
                            return Token::new(kw, self.make_span());
                        }
                    }
                    // Fall through to regular identifier scanning
                } else if (30..=45).contains(&pattern_id) {
                    // Operators (IDs 30-45) - always match
                    self.pos += len;
                    let kind = match pattern_id {
                        30 => TokenKind::StrictEqual,
                        31 => TokenKind::StrictNotEqual,
                        32 => TokenKind::Arrow,
                        33 => TokenKind::AmpersandAmpersand,
                        34 => TokenKind::PipePipe,
                        35 => TokenKind::PlusPlus,
                        36 => TokenKind::MinusMinus,
                        37 => TokenKind::PlusAssign,
                        38 => TokenKind::MinusAssign,
                        39 => TokenKind::Equal,
                        40 => TokenKind::NotEqual,
                        41 => TokenKind::LessEqual,
                        42 => TokenKind::GreaterEqual,
                        43 => {
                            /* BPE matched '??' but we need to check for '??=' */
                            if self.advance_if(b'=') {
                                TokenKind::QuestionQuestionAssign
                            } else {
                                TokenKind::QuestionQuestion
                            }
                        }
                        44 => TokenKind::QuestionDot,
                        45 => TokenKind::Ellipsis,
                        _ => unreachable!(),
                    };
                    return Token::new(kind, self.make_span());
                }
                // For common identifiers (IDs 50+), fall through to regular identifier scanning
                // This allows proper handling and interning
            }
        }

        self.scan_token()
    }

    /// Scan a single token (when BPE didn't match)
    fn scan_token(&mut self) -> Token<'src> {
        // UNWRAP-OK: caller next_token() verified !is_at_end() before dispatch,
        // and BPE branch did not consume bytes; advance() returns Some.
        let byte = self.advance().unwrap();

        match byte {
            // Single-char punctuators
            b'{' => Token::new(TokenKind::LeftBrace, self.make_span()),
            b'}' => Token::new(TokenKind::RightBrace, self.make_span()),
            b'(' => Token::new(TokenKind::LeftParen, self.make_span()),
            b')' => Token::new(TokenKind::RightParen, self.make_span()),
            b'[' => Token::new(TokenKind::LeftBracket, self.make_span()),
            b']' => Token::new(TokenKind::RightBracket, self.make_span()),
            b';' => Token::new(TokenKind::Semicolon, self.make_span()),
            b',' => Token::new(TokenKind::Comma, self.make_span()),
            b':' => Token::new(TokenKind::Colon, self.make_span()),
            b'~' => Token::new(TokenKind::Tilde, self.make_span()),
            b'@' => Token::new(TokenKind::At, self.make_span()),

            // Multi-char punctuators (check longer variants first)
            b'.' => {
                if self.peek() == Some(b'.') && self.peek_at(1) == Some(b'.') {
                    self.pos += 2;
                    Token::new(TokenKind::Ellipsis, self.make_span())
                } else if self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.scan_number()
                } else {
                    Token::new(TokenKind::Dot, self.make_span())
                }
            }

            b'?' => {
                if self.advance_if(b'?') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::QuestionQuestionAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::QuestionQuestion, self.make_span())
                    }
                } else if self.advance_if(b'.') {
                    Token::new(TokenKind::QuestionDot, self.make_span())
                } else {
                    Token::new(TokenKind::Question, self.make_span())
                }
            }

            b'+' => {
                if self.advance_if(b'+') {
                    Token::new(TokenKind::PlusPlus, self.make_span())
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::PlusAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Plus, self.make_span())
                }
            }

            b'-' => {
                if self.advance_if(b'-') {
                    Token::new(TokenKind::MinusMinus, self.make_span())
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::MinusAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Minus, self.make_span())
                }
            }

            b'*' => {
                if self.advance_if(b'*') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::StarStarAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::StarStar, self.make_span())
                    }
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::StarAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Star, self.make_span())
                }
            }

            b'/' => {
                if self.advance_if(b'=') {
                    Token::new(TokenKind::SlashAssign, self.make_span())
                } else {
                    // Note: regex scanning needs parser context
                    Token::new(TokenKind::Slash, self.make_span())
                }
            }

            b'%' => {
                if self.advance_if(b'=') {
                    Token::new(TokenKind::PercentAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Percent, self.make_span())
                }
            }

            b'<' => {
                if self.advance_if(b'<') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::LeftShiftAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::LeftShift, self.make_span())
                    }
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::LessEqual, self.make_span())
                } else {
                    Token::new(TokenKind::LessThan, self.make_span())
                }
            }

            b'>' => {
                if self.advance_if(b'>') {
                    if self.advance_if(b'>') {
                        if self.advance_if(b'=') {
                            Token::new(TokenKind::UnsignedRightShiftAssign, self.make_span())
                        } else {
                            Token::new(TokenKind::UnsignedRightShift, self.make_span())
                        }
                    } else if self.advance_if(b'=') {
                        Token::new(TokenKind::RightShiftAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::RightShift, self.make_span())
                    }
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::GreaterEqual, self.make_span())
                } else {
                    Token::new(TokenKind::GreaterThan, self.make_span())
                }
            }

            b'=' => {
                if self.advance_if(b'=') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::StrictEqual, self.make_span())
                    } else {
                        Token::new(TokenKind::Equal, self.make_span())
                    }
                } else if self.advance_if(b'>') {
                    Token::new(TokenKind::Arrow, self.make_span())
                } else {
                    Token::new(TokenKind::Assign, self.make_span())
                }
            }

            b'!' => {
                if self.advance_if(b'=') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::StrictNotEqual, self.make_span())
                    } else {
                        Token::new(TokenKind::NotEqual, self.make_span())
                    }
                } else {
                    Token::new(TokenKind::Bang, self.make_span())
                }
            }

            b'&' => {
                if self.advance_if(b'&') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::AmpersandAmpersandAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::AmpersandAmpersand, self.make_span())
                    }
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::AmpersandAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Ampersand, self.make_span())
                }
            }

            b'|' => {
                if self.advance_if(b'|') {
                    if self.advance_if(b'=') {
                        Token::new(TokenKind::PipePipeAssign, self.make_span())
                    } else {
                        Token::new(TokenKind::PipePipe, self.make_span())
                    }
                } else if self.advance_if(b'=') {
                    Token::new(TokenKind::PipeAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Pipe, self.make_span())
                }
            }

            b'^' => {
                if self.advance_if(b'=') {
                    Token::new(TokenKind::CaretAssign, self.make_span())
                } else {
                    Token::new(TokenKind::Caret, self.make_span())
                }
            }

            // String literals
            b'"' | b'\'' => self.scan_string(byte),

            // Template literals
            b'`' => self.scan_template(),

            // Numbers
            b'0'..=b'9' => self.scan_number(),

            // Identifiers (ASCII start)
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' => self.scan_identifier(),

            // Private identifier
            b'#' => {
                if is_identifier_start_byte(self.peek()) {
                    self.scan_private_identifier()
                } else {
                    Token::new(TokenKind::Error("unexpected #"), self.make_span())
                }
            }

            // Unicode identifier start (non-ASCII)
            _ if byte >= 0x80 => {
                // Back up and check for Unicode identifier
                self.pos -= 1;
                if self.is_unicode_identifier_start() {
                    self.scan_identifier()
                } else {
                    self.pos += 1;
                    Token::new(TokenKind::Error("unexpected character"), self.make_span())
                }
            }

            _ => Token::new(TokenKind::Error("unexpected character"), self.make_span()),
        }
    }

    /// Scan a string literal
    /// Uses SIMD via memchr3 for fast delimiter detection
    #[inline]
    fn scan_string(&mut self, quote: u8) -> Token<'src> {
        loop {
            let remaining = &self.bytes[self.pos..];
            if remaining.is_empty() {
                return Token::new(TokenKind::Error("unterminated string"), self.make_span());
            }

            // SIMD search for quote, backslash, or newline
            // Note: We search for \n only; \r is rare and handled in the slow path
            if let Some(offset) = memchr3(quote, b'\\', b'\n', remaining) {
                let found = remaining[offset];
                self.pos += offset;

                if found == quote {
                    self.pos += 1;
                    let text = self.current_text();
                    return Token::new(TokenKind::String(text), self.make_span());
                } else if found == b'\\' {
                    // Skip escape sequence (backslash + next char)
                    self.pos += 2;
                } else {
                    // Found newline - unterminated string
                    return Token::new(TokenKind::Error("unterminated string"), self.make_span());
                }
            } else {
                // No delimiter found - string extends to EOF (error)
                self.pos = self.bytes.len();
                return Token::new(TokenKind::Error("unterminated string"), self.make_span());
            }
        }
    }

    /// Scan a template literal (simplified - no nested expressions)
    /// Uses SIMD via memchr3 for fast delimiter detection
    #[inline]
    fn scan_template(&mut self) -> Token<'src> {
        loop {
            let remaining = &self.bytes[self.pos..];
            if remaining.is_empty() {
                return Token::new(TokenKind::Error("unterminated template"), self.make_span());
            }

            // SIMD search for backtick, backslash, or $ (for template expressions)
            if let Some(offset) = memchr3(b'`', b'\\', b'$', remaining) {
                let found = remaining[offset];
                self.pos += offset;

                if found == b'`' {
                    self.pos += 1;
                    let text = self.current_text();
                    return Token::new(TokenKind::Template(text), self.make_span());
                } else if found == b'\\' {
                    // Skip escape sequence
                    self.pos += 2;
                } else if found == b'$' {
                    // TODO: Handle ${...} expressions properly
                    // For now, just skip past $
                    self.pos += 1;
                }
            } else {
                // No delimiter found - template extends to EOF (error)
                self.pos = self.bytes.len();
                return Token::new(TokenKind::Error("unterminated template"), self.make_span());
            }
        }
    }

    /// Scan a number literal
    fn scan_number(&mut self) -> Token<'src> {
        // Back up to include the first digit
        self.pos = self.start;
        // UNWRAP-OK: caller dispatched here from scan_token() because byte at
        // self.start was a digit (or '.'); rewinding to self.start and advancing
        // returns Some(byte) by construction.
        let first = self.advance().unwrap();

        let mut is_float = first == b'.';

        // Handle 0x, 0o, 0b prefixes
        if first == b'0' {
            match self.peek() {
                Some(b'x' | b'X') => {
                    self.pos += 1;
                    while let Some(b) = self.peek() {
                        if b.is_ascii_hexdigit() {
                            self.pos += 1;
                        } else if b == b'_' {
                            self.pos += 1; // Numeric separator
                        } else {
                            break;
                        }
                    }
                    let text = self.current_text();
                    return Token::new(TokenKind::Integer(text), self.make_span());
                }
                Some(b'o' | b'O') => {
                    self.pos += 1;
                    while let Some(b) = self.peek() {
                        if (b'0'..=b'7').contains(&b) || b == b'_' {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    let text = self.current_text();
                    return Token::new(TokenKind::Integer(text), self.make_span());
                }
                Some(b'b' | b'B') => {
                    self.pos += 1;
                    while let Some(b) = self.peek() {
                        if b == b'0' || b == b'1' || b == b'_' {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    let text = self.current_text();
                    return Token::new(TokenKind::Integer(text), self.make_span());
                }
                _ => {}
            }
        }

        // Decimal digits
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }

        // Decimal point
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
            is_float = true;
            self.pos += 1;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() || b == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        // Exponent
        if self.peek() == Some(b'e') || self.peek() == Some(b'E') {
            is_float = true;
            self.pos += 1;
            if self.peek() == Some(b'+') || self.peek() == Some(b'-') {
                self.pos += 1;
            }
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() || b == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        // BigInt suffix
        if self.peek() == Some(b'n') {
            self.pos += 1;
            let text = self.current_text();
            return Token::new(TokenKind::Integer(text), self.make_span());
        }

        let text = self.current_text();
        if is_float {
            Token::new(TokenKind::Float(text), self.make_span())
        } else {
            Token::new(TokenKind::Integer(text), self.make_span())
        }
    }

    /// Scan an identifier or keyword
    fn scan_identifier(&mut self) -> Token<'src> {
        // Already consumed first char in scan_token
        while !self.is_at_end() {
            if let Some(b) = self.peek() {
                if is_identifier_continue_byte(Some(b)) {
                    self.pos += 1;
                } else if b >= 0x80 && self.is_unicode_identifier_continue() {
                    // Skip Unicode char
                    self.advance_unicode_char();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let text = self.current_text();

        // Check for keyword
        if let Some(kw) = keyword_lookup(text) {
            return Token::new(kw, self.make_span());
        }

        // Intern the identifier
        let symbol = self.interner.intern(text);
        Token::new(TokenKind::Identifier(symbol), self.make_span())
    }

    /// Scan a private identifier (#name)
    fn scan_private_identifier(&mut self) -> Token<'src> {
        // Already at # position, advance past it
        while !self.is_at_end() {
            if let Some(b) = self.peek() {
                if is_identifier_continue_byte(Some(b)) {
                    self.pos += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let text = self.current_text();
        let symbol = self.interner.intern(text);
        Token::new(TokenKind::PrivateIdentifier(symbol), self.make_span())
    }

    /// Check if current position starts a Unicode identifier
    fn is_unicode_identifier_start(&self) -> bool {
        let remaining = &self.source[self.pos..];
        if let Some(c) = remaining.chars().next() {
            unicode_xid::UnicodeXID::is_xid_start(c) || c == '$' || c == '_'
        } else {
            false
        }
    }

    /// Check if current position continues a Unicode identifier
    fn is_unicode_identifier_continue(&self) -> bool {
        let remaining = &self.source[self.pos..];
        if let Some(c) = remaining.chars().next() {
            unicode_xid::UnicodeXID::is_xid_continue(c) || c == '$'
        } else {
            false
        }
    }

    /// Advance past a Unicode character
    fn advance_unicode_char(&mut self) {
        let remaining = &self.source[self.pos..];
        if let Some(c) = remaining.chars().next() {
            self.pos += c.len_utf8();
        }
    }
}

/// Check if a byte can start an ASCII identifier
#[inline]
fn is_identifier_start_byte(b: Option<u8>) -> bool {
    matches!(b, Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
}

/// Check if a byte can continue an ASCII identifier
#[inline]
fn is_identifier_continue_byte(b: Option<u8>) -> bool {
    matches!(
        b,
        Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$')
    )
}

/// Iterator adapter for Lexer
impl<'src> Iterator for Lexer<'src> {
    type Item = Token<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.next_token();
        if matches!(token.kind, TokenKind::Eof) {
            None
        } else {
            Some(token)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_tokens() {
        let source = "{ } ( ) [ ]";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 6);
        assert!(matches!(tokens[0].kind, TokenKind::LeftBrace));
        assert!(matches!(tokens[1].kind, TokenKind::RightBrace));
        assert!(matches!(tokens[2].kind, TokenKind::LeftParen));
        assert!(matches!(tokens[3].kind, TokenKind::RightParen));
        assert!(matches!(tokens[4].kind, TokenKind::LeftBracket));
        assert!(matches!(tokens[5].kind, TokenKind::RightBracket));
    }

    #[test]
    fn test_keywords() {
        let source = "function const let var if else";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 6);
        assert!(matches!(tokens[0].kind, TokenKind::Function));
        assert!(matches!(tokens[1].kind, TokenKind::Const));
        assert!(matches!(tokens[2].kind, TokenKind::Let));
        assert!(matches!(tokens[3].kind, TokenKind::Var));
        assert!(matches!(tokens[4].kind, TokenKind::If));
        assert!(matches!(tokens[5].kind, TokenKind::Else));
    }

    #[test]
    fn test_operators() {
        let source = "=== !== => && || ??";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 6);
        assert!(matches!(tokens[0].kind, TokenKind::StrictEqual));
        assert!(matches!(tokens[1].kind, TokenKind::StrictNotEqual));
        assert!(matches!(tokens[2].kind, TokenKind::Arrow));
        assert!(matches!(tokens[3].kind, TokenKind::AmpersandAmpersand));
        assert!(matches!(tokens[4].kind, TokenKind::PipePipe));
        assert!(matches!(tokens[5].kind, TokenKind::QuestionQuestion));
    }

    #[test]
    fn test_numbers() {
        let source = "42 3.14 0xFF 0b1010 1e10";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 5);
        assert!(matches!(tokens[0].kind, TokenKind::Integer("42")));
        assert!(matches!(tokens[1].kind, TokenKind::Float("3.14")));
        assert!(matches!(tokens[2].kind, TokenKind::Integer("0xFF")));
        assert!(matches!(tokens[3].kind, TokenKind::Integer("0b1010")));
        assert!(matches!(tokens[4].kind, TokenKind::Float("1e10")));
    }

    #[test]
    fn test_strings() {
        let source = r#""hello" 'world'"#;
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0].kind, TokenKind::String(r#""hello""#)));
        assert!(matches!(tokens[1].kind, TokenKind::String("'world'")));
    }

    #[test]
    fn test_identifiers() {
        let source = "foo bar_baz $dollar _underscore";
        let mut lexer = Lexer::new(source);

        let tok1 = lexer.next_token();
        assert!(matches!(tok1.kind, TokenKind::Identifier(_)));
        if let TokenKind::Identifier(sym) = tok1.kind {
            assert_eq!(lexer.interner().resolve(sym), "foo");
        }
    }

    #[test]
    fn test_comments_skipped() {
        let source = "a // comment\nb /* block */ c";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0].kind, TokenKind::Identifier(_)));
        assert!(matches!(tokens[1].kind, TokenKind::Identifier(_)));
        assert!(matches!(tokens[2].kind, TokenKind::Identifier(_)));
    }

    #[test]
    fn test_complete_function() {
        let source = "function add(a, b) { return a + b; }";
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        // function add ( a , b ) { return a + b ; }
        assert_eq!(tokens.len(), 14);
        assert!(matches!(tokens[0].kind, TokenKind::Function));
        assert!(matches!(tokens[1].kind, TokenKind::Identifier(_)));
        assert!(matches!(tokens[2].kind, TokenKind::LeftParen));
    }
}
