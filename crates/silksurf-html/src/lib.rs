//! HTML5 tokenizer and parser (cleanroom).
#![allow(
    clippy::collapsible_if,
    clippy::new_without_default,
    clippy::manual_strip
)]

use memchr::{memchr, memchr2, memchr3};

mod tree_builder;

pub use tree_builder::TreeBuildError;
pub use tree_builder::TreeBuilder;

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

impl From<TokenizeError> for silksurf_core::SilkError {
    fn from(e: TokenizeError) -> Self {
        silksurf_core::SilkError::HtmlTokenize {
            offset: e.offset,
            message: e.message,
        }
    }
}

pub struct Tokenizer {
    buffer: String,
    cursor: usize,
    raw_text_tag: Option<String>,
}

impl Tokenizer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            raw_text_tag: None,
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

            if self.raw_text_tag.is_some() {
                if !self.parse_raw_text(&mut tokens)? {
                    break;
                }
                continue;
            }

            let remainder = &self.buffer[self.cursor..];
            match memchr(b'<', remainder.as_bytes()) {
                Some(0) => {
                    if !self.parse_tag(&mut tokens)? {
                        break;
                    }
                }
                Some(pos) => {
                    let text = decode_character_references(&remainder[..pos]);
                    tokens.push(Token::Character { data: text });
                    self.cursor += pos;
                }
                None => {
                    let text = decode_character_references(remainder);
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

    fn parse_doctype(
        &mut self,
        tokens: &mut Vec<Token>,
        start: usize,
    ) -> Result<bool, TokenizeError> {
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

    fn parse_end_tag(
        &mut self,
        tokens: &mut Vec<Token>,
        start: usize,
    ) -> Result<bool, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start + 2;
        if cursor >= bytes.len() {
            return Ok(false);
        }
        let name_start = cursor;
        let name_end = self.scan_tag_name_end(name_start);
        if name_end == name_start {
            return Err(self.error(State::EndTagOpen, name_start, "missing end tag name"));
        }
        if let Some(offset) = bytes[name_start..name_end]
            .iter()
            .position(|&b| !is_tag_name_char(b))
        {
            return Err(self.error(
                State::EndTagOpen,
                name_start + offset,
                "invalid end tag name",
            ));
        }
        cursor = name_end;
        let name = self.buffer[name_start..name_end].to_string();
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
        let name_end = self.scan_tag_name_end(name_start);
        if name_end == name_start {
            return Err(self.error(State::TagName, name_start, "missing tag name"));
        }
        if let Some(offset) = bytes[name_start..name_end]
            .iter()
            .position(|&b| !is_tag_name_char(b))
        {
            return Err(self.error(State::TagName, name_start + offset, "invalid tag name"));
        }
        cursor = name_end;
        let name = normalize_tag_name(&self.buffer[name_start..name_end]);
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
                        return Err(self.error(
                            State::SelfClosingStartTag,
                            cursor,
                            "expected '/>'",
                        ));
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

        let tag_name = name.clone();
        tokens.push(Token::StartTag {
            name,
            attributes,
            self_closing,
        });
        self.cursor = cursor;
        if !self_closing && (tag_name == "script" || tag_name == "style") {
            self.raw_text_tag = Some(tag_name);
        }
        Ok(true)
    }

    fn parse_raw_text(&mut self, tokens: &mut Vec<Token>) -> Result<bool, TokenizeError> {
        let tag = match self.raw_text_tag.clone() {
            Some(tag) => tag,
            None => return Ok(true),
        };
        let bytes = self.buffer.as_bytes();
        let mut cursor = self.cursor;
        while cursor < bytes.len() {
            let next = match memchr(b'<', &bytes[cursor..]) {
                Some(pos) => cursor + pos,
                None => break,
            };
            cursor = next;
            if cursor + 1 < bytes.len() && bytes[cursor + 1] == b'/' {
                let name_start = cursor + 2;
                if self.starts_with_case_insensitive(name_start, &tag) {
                    let end = name_start + tag.len();
                    if end < bytes.len() && bytes[end] == b'>' {
                        let data = self.buffer[self.cursor..cursor].to_string();
                        if !data.is_empty() {
                            tokens.push(Token::Character { data });
                        }
                        tokens.push(Token::EndTag {
                            name: normalize_tag_name(&tag),
                        });
                        self.cursor = end + 1;
                        self.raw_text_tag = None;
                        return Ok(true);
                    }
                }
            }
            cursor += 1;
        }
        Ok(false)
    }

    fn parse_attribute(&self, start: usize) -> Result<AttributeParse, TokenizeError> {
        let bytes = self.buffer.as_bytes();
        let mut cursor = start;
        let name_start = cursor;
        let name_end = self.scan_attr_name_end(name_start);
        if name_end == name_start {
            return Err(self.error(State::AttributeName, name_start, "invalid attribute name"));
        }
        if let Some(offset) = bytes[name_start..name_end]
            .iter()
            .position(|&b| !is_attr_name_char(b))
        {
            return Err(self.error(
                State::AttributeName,
                name_start + offset,
                "invalid attribute name",
            ));
        }
        cursor = name_end;
        let name = self.buffer[name_start..name_end].to_string();
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
                let rest = &bytes[cursor..];
                let rel = match memchr(quote, rest) {
                    Some(pos) => pos,
                    None => return Ok(AttributeParse::Incomplete),
                };
                let value_end = cursor + rel;
                value = decode_character_references(&self.buffer[value_start..value_end]);
                cursor = value_end + 1;
            }
            _ => {
                let value_start = cursor;
                let rest = &bytes[cursor..];
                let rel = memchr2(b'>', b'/', rest).unwrap_or(rest.len());
                let end = cursor + rel;
                while cursor < end && !is_whitespace(bytes[cursor]) {
                    cursor += 1;
                }
                value = decode_character_references(&self.buffer[value_start..cursor]);
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

    fn skip_to_gt(&self, cursor: usize) -> usize {
        let bytes = self.buffer.as_bytes();
        match memchr(b'>', &bytes[cursor..]) {
            Some(pos) => cursor + pos,
            None => bytes.len(),
        }
    }

    fn scan_tag_name_end(&self, start: usize) -> usize {
        let bytes = self.buffer.as_bytes();
        let rest = &bytes[start..];
        let mut end = start + rest.len();
        if let Some(pos) = memchr3(b' ', b'>', b'/', rest) {
            end = start + pos;
        }
        if let Some(pos) = memchr2(b'\n', b'\t', &bytes[start..end]) {
            end = start + pos;
        }
        if let Some(pos) = memchr2(b'\r', b'\x0c', &bytes[start..end]) {
            end = start + pos;
        }
        end
    }

    fn scan_attr_name_end(&self, start: usize) -> usize {
        let bytes = self.buffer.as_bytes();
        let rest = &bytes[start..];
        let mut end = start + rest.len();
        if let Some(pos) = memchr3(b' ', b'=', b'>', rest) {
            end = end.min(start + pos);
        }
        if let Some(pos) = memchr2(b'/', b'\n', rest) {
            end = end.min(start + pos);
        }
        if let Some(pos) = memchr2(b'\t', b'\r', rest) {
            end = end.min(start + pos);
        }
        if let Some(pos) = memchr(b'\x0c', rest) {
            end = end.min(start + pos);
        }
        end
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
        let rest = &bytes[cursor + 1..];
        let rel = match memchr(quote, rest) {
            Some(pos) => pos,
            None => return QuotedParse::Incomplete,
        };
        let end = cursor + 1 + rel;
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

fn decode_character_references(input: &str) -> String {
    let bytes = input.as_bytes();
    if memchr(b'&', bytes).is_none() {
        return input.to_string();
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(pos) = memchr(b'&', &bytes[cursor..]) {
        let amp = cursor + pos;
        output.push_str(&input[cursor..amp]);
        cursor = amp + 1;
        if let Some((decoded, consumed)) = parse_character_reference_at(&input[cursor..]) {
            output.push_str(&decoded);
            cursor += consumed;
        } else {
            output.push('&');
        }
    }
    output.push_str(&input[cursor..]);
    output
}

fn parse_character_reference_at(input: &str) -> Option<(String, usize)> {
    if let Some(rest) = input
        .strip_prefix("#x")
        .or_else(|| input.strip_prefix("#X"))
    {
        return parse_numeric_reference(rest, 16, 2);
    }
    if let Some(rest) = input.strip_prefix('#') {
        return parse_numeric_reference(rest, 10, 1);
    }
    for (name, value) in [
        ("amp", "&"),
        ("lt", "<"),
        ("gt", ">"),
        ("quot", "\""),
        ("apos", "'"),
    ] {
        if input.starts_with(name) {
            let tail = &input[name.len()..];
            if tail.starts_with(';') {
                return Some((value.to_string(), name.len() + 1));
            }
        }
    }
    None
}

fn parse_numeric_reference(input: &str, radix: u32, prefix_len: usize) -> Option<(String, usize)> {
    let mut end = None;
    for (idx, ch) in input.char_indices() {
        if ch == ';' {
            end = Some(idx);
            break;
        }
        if !ch.is_digit(radix) {
            return None;
        }
    }
    let end = end?;
    if end == 0 {
        return None;
    }
    let value = u32::from_str_radix(&input[..end], radix).ok()?;
    let ch = std::char::from_u32(value).unwrap_or('\u{FFFD}');
    Some((ch.to_string(), prefix_len + end + 1))
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
