//! CSS syntax, cascade, and computed values (cleanroom).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssToken {
    AtKeyword(String),
    Ident(String),
    Function(String),
    Hash(String),
    String(String),
    Number(String),
    Percentage(String),
    Dimension { value: String, unit: String },
    Delim(char),
    Colon,
    Semicolon,
    Comma,
    CurlyOpen,
    CurlyClose,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    Whitespace,
    Cdo,
    Cdc,
    Url(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssError {
    pub offset: usize,
    pub message: String,
}

pub struct CssTokenizer {
    buffer: String,
    cursor: usize,
}

impl CssTokenizer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
        }
    }

    pub fn feed(&mut self, input: &str) -> Result<Vec<CssToken>, CssError> {
        if !input.is_empty() {
            self.buffer.push_str(input);
        }

        let mut tokens = Vec::new();
        while self.cursor < self.buffer.len() {
            let bytes = self.buffer.as_bytes();
            let current = bytes[self.cursor];

            if is_whitespace(current) {
                let start = self.cursor;
                self.cursor += 1;
                while self.cursor < bytes.len() && is_whitespace(bytes[self.cursor]) {
                    self.cursor += 1;
                }
                tokens.push(CssToken::Whitespace);
                continue;
            }

            if current == b'/' && self.cursor + 1 < bytes.len() && bytes[self.cursor + 1] == b'*' {
                let comment_start = self.cursor + 2;
                if let Some(end) = find_subsequence(bytes, comment_start, b"*/") {
                    self.cursor = end + 2;
                    tokens.push(CssToken::Whitespace);
                    continue;
                }
                break;
            }

            if current == b'<' && self.cursor + 3 < bytes.len() {
                if &bytes[self.cursor..self.cursor + 4] == b"<!--" {
                    self.cursor += 4;
                    tokens.push(CssToken::Cdo);
                    continue;
                }
            }

            if current == b'-' && self.cursor + 2 < bytes.len() {
                if &bytes[self.cursor..self.cursor + 3] == b"-->" {
                    self.cursor += 3;
                    tokens.push(CssToken::Cdc);
                    continue;
                }
            }

            match current {
                b'"' | b'\'' => {
                    let quote = current;
                    let string_start = self.cursor + 1;
                    let mut cursor = string_start;
                    while cursor < bytes.len() && bytes[cursor] != quote {
                        cursor += 1;
                    }
                    if cursor >= bytes.len() {
                        break;
                    }
                    let value = self.buffer[string_start..cursor].to_string();
                    self.cursor = cursor + 1;
                    tokens.push(CssToken::String(value));
                }
                b'#' => {
                    let mut cursor = self.cursor + 1;
                    while cursor < bytes.len() && is_ident_char(bytes[cursor]) {
                        cursor += 1;
                    }
                    if cursor == self.cursor + 1 {
                        self.cursor += 1;
                        tokens.push(CssToken::Delim('#'));
                    } else {
                        let name = self.buffer[self.cursor + 1..cursor].to_string();
                        self.cursor = cursor;
                        tokens.push(CssToken::Hash(name));
                    }
                }
                b'@' => {
                    if let Some((name, next)) = self.parse_ident(self.cursor + 1) {
                        self.cursor = next;
                        tokens.push(CssToken::AtKeyword(name));
                    } else {
                        self.cursor += 1;
                        tokens.push(CssToken::Delim('@'));
                    }
                }
                b':' => {
                    self.cursor += 1;
                    tokens.push(CssToken::Colon);
                }
                b';' => {
                    self.cursor += 1;
                    tokens.push(CssToken::Semicolon);
                }
                b',' => {
                    self.cursor += 1;
                    tokens.push(CssToken::Comma);
                }
                b'{' => {
                    self.cursor += 1;
                    tokens.push(CssToken::CurlyOpen);
                }
                b'}' => {
                    self.cursor += 1;
                    tokens.push(CssToken::CurlyClose);
                }
                b'(' => {
                    self.cursor += 1;
                    tokens.push(CssToken::ParenOpen);
                }
                b')' => {
                    self.cursor += 1;
                    tokens.push(CssToken::ParenClose);
                }
                b'[' => {
                    self.cursor += 1;
                    tokens.push(CssToken::BracketOpen);
                }
                b']' => {
                    self.cursor += 1;
                    tokens.push(CssToken::BracketClose);
                }
                b'0'..=b'9' | b'.' | b'+' | b'-' => {
                    if let Some((number, mut cursor)) = self.parse_number(self.cursor) {
                        if cursor < bytes.len() && bytes[cursor] == b'%' {
                            cursor += 1;
                            self.cursor = cursor;
                            tokens.push(CssToken::Percentage(number));
                            continue;
                        }
                        if let Some((unit, next)) = self.parse_ident(cursor) {
                            self.cursor = next;
                            tokens.push(CssToken::Dimension { value: number, unit });
                            continue;
                        }
                        self.cursor = cursor;
                        tokens.push(CssToken::Number(number));
                    } else {
                        let delim = self.buffer[self.cursor..].chars().next().unwrap();
                        self.cursor += delim.len_utf8();
                        tokens.push(CssToken::Delim(delim));
                    }
                }
                _ if is_ident_start(current) => {
                    let start = self.cursor;
                    let mut cursor = self.cursor + 1;
                    while cursor < bytes.len() && is_ident_char(bytes[cursor]) {
                        cursor += 1;
                    }
                    let ident = self.buffer[start..cursor].to_string();
                    if cursor < bytes.len() && bytes[cursor] == b'(' {
                        if ident.eq_ignore_ascii_case("url") {
                            match self.parse_url(cursor + 1) {
                                UrlParse::Parsed(value, next) => {
                                    self.cursor = next;
                                    tokens.push(CssToken::Url(value));
                                }
                                UrlParse::Incomplete => break,
                            }
                        } else {
                            self.cursor = cursor + 1;
                            tokens.push(CssToken::Function(ident));
                        }
                    } else {
                        self.cursor = cursor;
                        tokens.push(CssToken::Ident(ident));
                    }
                }
                _ => {
                    let delim = self.buffer[self.cursor..].chars().next().unwrap();
                    self.cursor += delim.len_utf8();
                    tokens.push(CssToken::Delim(delim));
                }
            }
        }

        if self.cursor > 0 {
            self.buffer.drain(..self.cursor);
            self.cursor = 0;
        }

        Ok(tokens)
    }

    pub fn finish(&mut self) -> Result<Vec<CssToken>, CssError> {
        let mut tokens = self.feed("")?;
        tokens.push(CssToken::Eof);
        Ok(tokens)
    }

    fn parse_ident(&self, start: usize) -> Option<(String, usize)> {
        let bytes = self.buffer.as_bytes();
        if start >= bytes.len() || !is_ident_start(bytes[start]) {
            return None;
        }
        let mut cursor = start + 1;
        while cursor < bytes.len() && is_ident_char(bytes[cursor]) {
            cursor += 1;
        }
        Some((self.buffer[start..cursor].to_string(), cursor))
    }

    fn parse_number(&self, start: usize) -> Option<(String, usize)> {
        let bytes = self.buffer.as_bytes();
        if start >= bytes.len() {
            return None;
        }
        let mut cursor = start;
        if matches!(bytes[cursor], b'+' | b'-') {
            cursor += 1;
            if cursor >= bytes.len() {
                return None;
            }
        }
        let mut has_digit = false;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
            has_digit = true;
        }
        if cursor < bytes.len() && bytes[cursor] == b'.' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
                has_digit = true;
            }
        }
        if !has_digit {
            return None;
        }
        Some((self.buffer[start..cursor].to_string(), cursor))
    }

    fn parse_url(&self, start: usize) -> UrlParse {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start;
        while cursor < bytes.len() && is_whitespace(bytes[cursor]) {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return UrlParse::Incomplete;
        }
        let value;
        match bytes[cursor] {
            b'"' | b'\'' => {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                if cursor >= bytes.len() {
                    return UrlParse::Incomplete;
                }
                value = self.buffer[value_start..cursor].to_string();
                cursor += 1;
            }
            _ => {
                let value_start = cursor;
                while cursor < bytes.len()
                    && bytes[cursor] != b')'
                    && !is_whitespace(bytes[cursor])
                {
                    cursor += 1;
                }
                if cursor == value_start {
                    value = String::new();
                } else {
                    value = self.buffer[value_start..cursor].to_string();
                }
            }
        }

        while cursor < bytes.len() && is_whitespace(bytes[cursor]) {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return UrlParse::Incomplete;
        }
        if bytes[cursor] != b')' {
            return UrlParse::Incomplete;
        }
        UrlParse::Parsed(value, cursor + 1)
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\t' | b'\r' | b'\x0c')
}

fn is_ident_start(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'-')
}

fn is_ident_char(byte: u8) -> bool {
    is_ident_start(byte) || matches!(byte, b'0'..=b'9')
}

fn find_subsequence(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|pos| start + pos)
}

enum UrlParse {
    Parsed(String, usize),
    Incomplete,
}
