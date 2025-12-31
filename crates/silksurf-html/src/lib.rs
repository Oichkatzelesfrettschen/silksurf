//! HTML5 tokenizer and parser (cleanroom).

use memchr::memchr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Doctype {
        name: Option<String>,
        public_id: Option<String>,
        system_id: Option<String>,
        force_quirks: bool,
    },
    StartTag {
        name: String,
        attributes: Vec<Attribute>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Comment {
        data: String,
    },
    Character {
        data: String,
    },
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    AttributeName,
    AttributeValue,
    SelfClosingStartTag,
    MarkupDeclarationOpen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizeError {
    pub state: State,
    pub offset: usize,
    pub message: String,
}

pub struct Tokenizer {
    buffer: String,
    cursor: usize,
}

impl Tokenizer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
        }
    }

    pub fn feed(&mut self, input: &str) -> Result<Vec<Token>, TokenizeError> {
        if !input.is_empty() {
            self.buffer.push_str(input);
        }

        let mut tokens = Vec::new();
        loop {
            if self.cursor >= self.buffer.len() {
                break;
            }

            let remainder = &self.buffer[self.cursor..];
            match memchr(b'<', remainder.as_bytes()) {
                Some(0) => {
                    if !self.parse_tag(&mut tokens)? {
                        break;
                    }
                }
                Some(pos) => {
                    let text = remainder[..pos].to_string();
                    tokens.push(Token::Character { data: text });
                    self.cursor += pos;
                }
                None => {
                    let text = remainder.to_string();
                    tokens.push(Token::Character { data: text });
                    self.cursor = self.buffer.len();
                    break;
                }
            }
        }

        if self.cursor > 0 {
            self.buffer.drain(..self.cursor);
            self.cursor = 0;
        }

        Ok(tokens)
    }

    pub fn finish(&mut self) -> Result<Vec<Token>, TokenizeError> {
        let mut tokens = self.feed("")?;
        tokens.push(Token::Eof);
        Ok(tokens)
    }

    fn parse_tag(&mut self, tokens: &mut Vec<Token>) -> Result<bool, TokenizeError> {
        let start = self.cursor;
        let bytes = self.buffer.as_bytes();
        if bytes[start] != b'<' {
            return Err(self.error(State::Data, start, "expected '<'"));
        }

        if start + 1 >= bytes.len() {
            return Ok(false);
        }

        match bytes[start + 1] {
            b'!' => self.parse_markup_declaration(tokens, start),
            b'/' => self.parse_end_tag(tokens, start),
            b'a'..=b'z' | b'A'..=b'Z' => self.parse_start_tag(tokens, start),
            _ => Err(self.error(State::TagOpen, start + 1, "unexpected tag opener")),
        }
    }

    fn parse_markup_declaration(
        &mut self,
        tokens: &mut Vec<Token>,
        start: usize,
    ) -> Result<bool, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        if start + 2 >= bytes.len() {
            return Ok(false);
        }

        if self.starts_with_at(start + 2, "--") {
            let comment_start = start + 4;
            if comment_start > bytes.len() {
                return Ok(false);
            }
            if let Some(end) = self.find_subsequence(comment_start, b"-->") {
                let data = self.buffer[comment_start..end].to_string();
                tokens.push(Token::Comment { data });
                self.cursor = end + 3;
                return Ok(true);
            }
            return Ok(false);
        }

        if self.starts_with_case_insensitive(start + 2, "doctype") {
            return self.parse_doctype(tokens, start);
        }

        Err(self.error(
            State::MarkupDeclarationOpen,
            start + 1,
            "unsupported markup declaration",
        ))
    }

    fn parse_doctype(&mut self, tokens: &mut Vec<Token>, start: usize) -> Result<bool, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start + 2 + "doctype".len();
        cursor = self.skip_whitespace(cursor);
        if cursor >= bytes.len() {
            return Ok(false);
        }

        let name_start = cursor;
        while cursor < bytes.len() && !is_whitespace(bytes[cursor]) && bytes[cursor] != b'>' {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Ok(false);
        }

        let name = if cursor > name_start {
            Some(self.buffer[name_start..cursor].to_string())
        } else {
            None
        };

        let mut public_id = None;
        let mut system_id = None;
        let mut force_quirks = name.is_none();

        cursor = self.skip_whitespace(cursor);
        if cursor >= bytes.len() {
            return Ok(false);
        }

        if bytes[cursor] != b'>' {
            if self.starts_with_case_insensitive(cursor, "public") {
                cursor += "public".len();
                cursor = self.skip_whitespace(cursor);
                match self.parse_quoted_string(cursor) {
                    QuotedParse::Parsed(value, next) => {
                        public_id = Some(value);
                        cursor = self.skip_whitespace(next);
                    }
                    QuotedParse::Incomplete => return Ok(false),
                    QuotedParse::MissingQuote => {
                        force_quirks = true;
                        cursor = self.skip_to_gt(cursor);
                    }
                }

                if cursor < bytes.len() && bytes[cursor] != b'>' {
                    match self.parse_quoted_string(cursor) {
                        QuotedParse::Parsed(value, next) => {
                            system_id = Some(value);
                            cursor = next;
                        }
                        QuotedParse::Incomplete => return Ok(false),
                        QuotedParse::MissingQuote => {
                            force_quirks = true;
                            cursor = self.skip_to_gt(cursor);
                        }
                    }
                }
            } else if self.starts_with_case_insensitive(cursor, "system") {
                cursor += "system".len();
                cursor = self.skip_whitespace(cursor);
                match self.parse_quoted_string(cursor) {
                    QuotedParse::Parsed(value, next) => {
                        system_id = Some(value);
                        cursor = next;
                    }
                    QuotedParse::Incomplete => return Ok(false),
                    QuotedParse::MissingQuote => {
                        force_quirks = true;
                        cursor = self.skip_to_gt(cursor);
                    }
                }
            } else {
                force_quirks = true;
                cursor = self.skip_to_gt(cursor);
            }
        }

        cursor = self.skip_to_gt(cursor);
        if cursor >= bytes.len() {
            return Ok(false);
        }
        cursor += 1;

        tokens.push(Token::Doctype {
            name,
            public_id,
            system_id,
            force_quirks,
        });
        self.cursor = cursor;
        Ok(true)
    }

    fn parse_end_tag(&mut self, tokens: &mut Vec<Token>, start: usize) -> Result<bool, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start + 2;
        if cursor >= bytes.len() {
            return Ok(false);
        }
        let name_start = cursor;
        while cursor < bytes.len() && is_tag_name_char(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            return Err(self.error(State::EndTagOpen, cursor, "missing end tag name"));
        }
        let name = self.buffer[name_start..cursor].to_string();
        cursor = self.skip_whitespace(cursor);
        if cursor >= bytes.len() {
            return Ok(false);
        }
        if bytes[cursor] != b'>' {
            return Err(self.error(State::EndTagOpen, cursor, "expected '>'"));
        }
        cursor += 1;
        tokens.push(Token::EndTag {
            name: normalize_tag_name(&name),
        });
        self.cursor = cursor;
        Ok(true)
    }

    fn parse_start_tag(
        &mut self,
        tokens: &mut Vec<Token>,
        start: usize,
    ) -> Result<bool, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start + 1;
        if cursor >= bytes.len() {
            return Ok(false);
        }

        let name_start = cursor;
        while cursor < bytes.len() && is_tag_name_char(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            return Err(self.error(State::TagName, cursor, "missing tag name"));
        }
        let name = normalize_tag_name(&self.buffer[name_start..cursor]);
        let mut attributes = Vec::new();
        let mut self_closing = false;

        loop {
            cursor = self.skip_whitespace(cursor);
            if cursor >= bytes.len() {
                return Ok(false);
            }
            match bytes[cursor] {
                b'>' => {
                    cursor += 1;
                    break;
                }
                b'/' => {
                    if cursor + 1 >= bytes.len() {
                        return Ok(false);
                    }
                    if bytes[cursor + 1] != b'>' {
                        return Err(self.error(State::SelfClosingStartTag, cursor, "expected '/>'"));
                    }
                    self_closing = true;
                    cursor += 2;
                    break;
                }
                _ => match self.parse_attribute(cursor)? {
                    AttributeParse::Parsed(attr, next_cursor) => {
                        attributes.push(attr);
                        cursor = next_cursor;
                    }
                    AttributeParse::Incomplete => return Ok(false),
                },
            }
        }

        tokens.push(Token::StartTag {
            name,
            attributes,
            self_closing,
        });
        self.cursor = cursor;
        Ok(true)
    }

    fn parse_attribute(&self, start: usize) -> Result<AttributeParse, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start;
        let name_start = cursor;
        while cursor < bytes.len() && is_attr_name_char(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            return Err(self.error(State::AttributeName, cursor, "invalid attribute name"));
        }
        let name = self.buffer[name_start..cursor].to_string();
        cursor = self.skip_whitespace(cursor);
        if cursor >= bytes.len() {
            return Ok(AttributeParse::Incomplete);
        }
        if bytes[cursor] != b'=' {
            return Ok(AttributeParse::Parsed(
                Attribute { name, value: None },
                cursor,
            ));
        }
        cursor += 1;
        cursor = self.skip_whitespace(cursor);
        if cursor >= bytes.len() {
            return Ok(AttributeParse::Incomplete);
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
                    return Ok(AttributeParse::Incomplete);
                }
                value = self.buffer[value_start..cursor].to_string();
                cursor += 1;
            }
            _ => {
                let value_start = cursor;
                while cursor < bytes.len()
                    && !is_whitespace(bytes[cursor])
                    && bytes[cursor] != b'>'
                    && bytes[cursor] != b'/'
                {
                    cursor += 1;
                }
                value = self.buffer[value_start..cursor].to_string();
            }
        }

        Ok(AttributeParse::Parsed(
            Attribute {
                name,
                value: Some(value),
            },
            cursor,
        ))
    }

    fn skip_whitespace(&self, mut cursor: usize) -> usize {
        let bytes = self.buffer.as_bytes();
        while cursor < bytes.len() && is_whitespace(bytes[cursor]) {
            cursor += 1;
        }
        cursor
    }

    fn skip_to_gt(&self, mut cursor: usize) -> usize {
        let bytes = self.buffer.as_bytes();
        while cursor < bytes.len() && bytes[cursor] != b'>' {
            cursor += 1;
        }
        cursor
    }

    fn find_subsequence(&self, start: usize, needle: &[u8]) -> Option<usize> {
        let bytes = self.buffer.as_bytes();
        bytes[start..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|pos| start + pos)
    }

    fn starts_with_at(&self, start: usize, needle: &str) -> bool {
        self.buffer[start..].starts_with(needle)
    }

    fn starts_with_case_insensitive(&self, start: usize, needle: &str) -> bool {
        let end = start + needle.len();
        if end > self.buffer.len() {
            return false;
        }
        self.buffer[start..end]
            .bytes()
            .zip(needle.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    }

    fn parse_quoted_string(&self, cursor: usize) -> QuotedParse {
        let bytes = self.buffer.as_bytes();
        if cursor >= bytes.len() {
            return QuotedParse::Incomplete;
        }
        let quote = bytes[cursor];
        if quote != b'"' && quote != b'\'' {
            return QuotedParse::MissingQuote;
        }
        let mut end = cursor + 1;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        if end >= bytes.len() {
            return QuotedParse::Incomplete;
        }
        let value = self.buffer[cursor + 1..end].to_string();
        QuotedParse::Parsed(value, end + 1)
    }

    fn error(&self, state: State, offset: usize, message: &str) -> TokenizeError {
        TokenizeError {
            state,
            offset,
            message: message.to_string(),
        }
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\t' | b'\r' | b'\x0c')
}

fn is_tag_name_char(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-')
}

fn is_attr_name_char(byte: u8) -> bool {
    is_tag_name_char(byte) || byte == b'_' || byte == b':'
}

fn normalize_tag_name(name: &str) -> String {
    name.bytes()
        .map(|b| b.to_ascii_lowercase() as char)
        .collect()
}

enum AttributeParse {
    Parsed(Attribute, usize),
    Incomplete,
}

enum QuotedParse {
    Parsed(String, usize),
    Incomplete,
    MissingQuote,
}
