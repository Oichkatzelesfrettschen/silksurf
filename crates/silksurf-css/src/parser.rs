use crate::selector::{SelectorList, parse_selector_list};
use crate::{CssError, CssToken, CssTokenizer};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use silksurf_core::SilkInterner;
use smol_str::SmolStr;
use std::borrow::Cow;

const MAX_CSS_BYTES: usize = 128 * 1024;
const MAX_INLINE_STYLE_BYTES: usize = 16 * 1024;
const MAX_NESTED_AT_RULE_BLOCK_TOKENS: usize = 4096;
const MAX_QUALIFIED_RULE_SELECTOR_TOKENS: usize = 1024;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Rule {
    Style(StyleRule),
    At(AtRule),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StyleRule {
    pub selectors: SelectorList,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AtRule {
    pub name: SmolStr,
    pub prelude: Vec<CssToken>,
    pub block: Option<AtRuleBlock>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AtRuleBlock {
    Rules(Vec<Rule>),
    Declarations(Vec<Declaration>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Declaration {
    pub name: SmolStr,
    pub value: Vec<CssToken>,
    pub important: bool,
    /// Pre-computed property ID for O(1) cascade dispatch.
    /// Computed once during parsing; eliminates string matching in cascade.
    pub property_id: crate::property_id::PropertyId,
}

pub struct CssParser {
    tokens: Vec<CssToken>,
    cursor: usize,
}

impl CssParser {
    #[must_use]
    pub fn new(mut tokens: Vec<CssToken>) -> Self {
        if !matches!(tokens.last(), Some(CssToken::Eof)) {
            tokens.push(CssToken::Eof);
        }
        Self { tokens, cursor: 0 }
    }

    pub fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        self.consume_ignorable();
        while !self.is_eof() {
            if let Some(rule) = self.parse_rule() {
                rules.push(rule);
            } else {
                self.next();
            }
            self.consume_ignorable();
        }
        Stylesheet { rules }
    }

    fn parse_rule(&mut self) -> Option<Rule> {
        match self.peek() {
            Some(CssToken::AtKeyword(_)) => self.parse_at_rule(),
            Some(CssToken::Eof) | None => None,
            _ => self.parse_qualified_rule(),
        }
    }

    fn parse_at_rule(&mut self) -> Option<Rule> {
        let Some(CssToken::AtKeyword(name)) = self.next() else {
            return None;
        };
        let mut prelude = Vec::new();
        loop {
            match self.peek() {
                Some(CssToken::Semicolon) => {
                    self.next();
                    return Some(Rule::At(AtRule {
                        name,
                        prelude,
                        block: None,
                    }));
                }
                Some(CssToken::CurlyOpen) => {
                    self.next();
                    let block_tokens = self.consume_block();
                    let block = Some(parse_at_rule_block(block_tokens));
                    return Some(Rule::At(AtRule {
                        name,
                        prelude,
                        block,
                    }));
                }
                Some(CssToken::Eof) | None => {
                    return Some(Rule::At(AtRule {
                        name,
                        prelude,
                        block: None,
                    }));
                }
                _ => {
                    // UNWRAP-OK: peek() above returned Some(non-Eof) so next() is guaranteed Some.
                    prelude.push(self.next().unwrap());
                }
            }
        }
    }
    fn parse_qualified_rule(&mut self) -> Option<Rule> {
        let mut selector_tokens = Vec::new();
        loop {
            match self.peek() {
                Some(CssToken::CurlyOpen) => {
                    self.next();
                    let block_tokens = self.consume_block();
                    let declarations = parse_declarations(&block_tokens);
                    let selectors = parse_bounded_selector_list(
                        selector_tokens,
                        MAX_QUALIFIED_RULE_SELECTOR_TOKENS,
                    );
                    return Some(Rule::Style(StyleRule {
                        selectors,
                        declarations,
                    }));
                }
                Some(CssToken::Eof) | None => return None,
                _ => {
                    // UNWRAP-OK: peek() above returned Some(non-Eof) so next() is guaranteed Some.
                    selector_tokens.push(self.next().unwrap());
                }
            }
        }
    }
    fn consume_block(&mut self) -> Vec<CssToken> {
        let mut depth = 1usize;
        let mut tokens = Vec::new();
        while let Some(token) = self.next() {
            match token {
                CssToken::CurlyOpen => {
                    depth += 1;
                    tokens.push(token);
                }
                CssToken::CurlyClose => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                    tokens.push(token);
                }
                CssToken::Eof => break,
                _ => tokens.push(token),
            }
        }
        tokens
    }

    fn consume_ignorable(&mut self) {
        while matches!(
            self.peek(),
            Some(CssToken::Whitespace | CssToken::Cdo | CssToken::Cdc)
        ) {
            self.next();
        }
    }
    fn peek(&self) -> Option<&CssToken> {
        self.tokens.get(self.cursor)
    }

    fn next(&mut self) -> Option<CssToken> {
        let token = self.tokens.get(self.cursor).cloned();
        if token.is_some() {
            self.cursor += 1;
        }
        token
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek(), Some(CssToken::Eof) | None)
    }
}

pub fn parse_stylesheet(input: &str) -> Result<Stylesheet, CssError> {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);
    let mut parser = CssParser::new(tokens);
    let stylesheet = parser.parse_stylesheet();
    /*
     * MAX_CSS_RULES bounds the top-level rule vector before cascade and
     * matching receive the stylesheet.
     */
    if stylesheet.rules.len() > crate::MAX_CSS_RULES {
        return Err(CssError {
            offset: 0,
            message: format!(
                "stylesheet rule count {} exceeds MAX_CSS_RULES {}",
                stylesheet.rules.len(),
                crate::MAX_CSS_RULES
            ),
        });
    }
    Ok(stylesheet)
}

pub fn parse_stylesheet_bytes(input: &[u8]) -> Result<Stylesheet, CssError> {
    let decoded = decode_stylesheet_bytes(input);
    parse_stylesheet(decoded.as_ref())
}

pub fn parse_declaration_list(input: &str) -> Result<Vec<Declaration>, CssError> {
    let truncated = truncate_at_declaration_boundary(input, MAX_INLINE_STYLE_BYTES);
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(truncated)?;
    tokens.extend(tokenizer.finish()?);
    Ok(parse_declarations(&tokens))
}

pub fn parse_stylesheet_with_interner(
    input: &str,
    interner: &mut SilkInterner,
) -> Result<Stylesheet, CssError> {
    let truncated = truncate_at_rule_boundary(input, MAX_CSS_BYTES);

    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(truncated)?;
    tokens.extend(tokenizer.finish()?);
    #[cfg(debug_assertions)]
    {
        let t0 = std::time::Instant::now();
        eprintln!(
            "[CSS] Tokenized {} bytes -> {} tokens in {:?}",
            truncated.len(),
            tokens.len(),
            t0.elapsed()
        );
    }
    let mut parser = CssParser::new(tokens);
    let mut sheet = parser.parse_stylesheet();
    #[cfg(debug_assertions)]
    {
        eprintln!("[CSS] Parsed {} rules", sheet.rules.len());
    }
    intern_rules(&mut sheet.rules, interner);
    Ok(sheet)
}

fn truncate_at_declaration_boundary(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let prefix = utf8_prefix(input, max_bytes);
    prefix.rfind(';').map_or(prefix, |pos| &prefix[..=pos])
}

fn truncate_at_rule_boundary(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let prefix = utf8_prefix(input, max_bytes);
    prefix.rfind('}').map_or(prefix, |pos| &prefix[..=pos])
}

fn utf8_prefix(input: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(input.len());
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

fn decode_stylesheet_bytes(input: &[u8]) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed("");
    }

    if let Some((encoding, _)) = Encoding::for_bom(input) {
        let (decoded, _) = encoding.decode_with_bom_removal(input);
        return decoded;
    }

    if let Some(encoding) =
        sniff_declared_encoding(input).or_else(|| sniff_utf16_without_bom(input))
    {
        let (decoded, _) = encoding.decode_without_bom_handling(input);
        return decoded;
    }

    let (decoded, _) = UTF_8.decode_without_bom_handling(input);
    decoded
}

fn sniff_declared_encoding(input: &[u8]) -> Option<&'static Encoding> {
    const PREFIX: &[u8] = b"@charset";

    let mut cursor = 0usize;
    while cursor < input.len() && is_css_whitespace(input[cursor]) {
        cursor += 1;
    }
    let bytes = &input[cursor..];

    if bytes.len() < PREFIX.len() || !bytes[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        return None;
    }

    let mut index = PREFIX.len();
    while index < bytes.len() && is_css_whitespace(bytes[index]) {
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    index += 1;

    let label_start = index;
    while index < bytes.len() && bytes[index] != b'"' {
        index += 1;
    }
    if index >= bytes.len() {
        return None;
    }
    let label = &bytes[label_start..index];
    index += 1;

    while index < bytes.len() && is_css_whitespace(bytes[index]) {
        index += 1;
    }
    if bytes.get(index) != Some(&b';') {
        return None;
    }

    Encoding::for_label(label)
}

fn sniff_utf16_without_bom(input: &[u8]) -> Option<&'static Encoding> {
    if input.len() < 4 {
        return None;
    }

    let sample = &input[..input.len().min(128)];
    let even_zero_count = sample.iter().step_by(2).filter(|&&byte| byte == 0).count();
    let odd_zero_count = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|&&byte| byte == 0)
        .count();

    if odd_zero_count >= 2 && odd_zero_count >= even_zero_count.saturating_mul(2) {
        return Some(UTF_16LE);
    }

    if even_zero_count >= 2 && even_zero_count >= odd_zero_count.saturating_mul(2) {
        return Some(UTF_16BE);
    }

    None
}

fn is_css_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn parse_at_rule_block(tokens: Vec<CssToken>) -> AtRuleBlock {
    if looks_like_declarations(&tokens) {
        AtRuleBlock::Declarations(parse_declarations(&tokens))
    } else if tokens.len() > MAX_NESTED_AT_RULE_BLOCK_TOKENS {
        AtRuleBlock::Rules(Vec::new())
    } else {
        let mut parser = CssParser::new(tokens);
        AtRuleBlock::Rules(parser.parse_stylesheet().rules)
    }
}

fn parse_bounded_selector_list(tokens: Vec<CssToken>, max_tokens: usize) -> SelectorList {
    if tokens.len() > max_tokens {
        parse_selector_list(Vec::new())
    } else {
        parse_selector_list(tokens)
    }
}

fn intern_rules(rules: &mut [Rule], interner: &mut SilkInterner) {
    for rule in rules {
        match rule {
            Rule::Style(style) => {
                style.selectors.intern_with(interner);
            }
            Rule::At(at_rule) => {
                if let Some(AtRuleBlock::Rules(children)) = &mut at_rule.block {
                    intern_rules(children, interner);
                }
            }
        }
    }
}

fn looks_like_declarations(tokens: &[CssToken]) -> bool {
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index] {
            CssToken::CurlyOpen => depth += 1,
            CssToken::CurlyClose => depth = depth.saturating_sub(1),
            CssToken::Ident(_) if depth == 0 => {
                let mut lookahead = index + 1;
                while lookahead < tokens.len() && matches!(tokens[lookahead], CssToken::Whitespace)
                {
                    lookahead += 1;
                }
                if matches!(tokens.get(lookahead), Some(CssToken::Colon)) {
                    return true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}
fn parse_declarations(tokens: &[CssToken]) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    let mut cursor = 0usize;
    while cursor < tokens.len() {
        while cursor < tokens.len()
            && matches!(tokens[cursor], CssToken::Whitespace | CssToken::Semicolon)
        {
            cursor += 1;
        }
        let name = match tokens.get(cursor) {
            Some(CssToken::Ident(name)) => {
                cursor += 1;
                name.clone()
            }
            Some(CssToken::AtKeyword(_) | CssToken::CurlyOpen) | None => break,
            _ => {
                cursor = skip_component_value(tokens, cursor);
                continue;
            }
        };
        while cursor < tokens.len() && matches!(tokens[cursor], CssToken::Whitespace) {
            cursor += 1;
        }
        if !matches!(tokens.get(cursor), Some(CssToken::Colon)) {
            cursor = skip_component_value(tokens, cursor);
            continue;
        }
        cursor += 1;
        let mut value = Vec::new();
        while cursor < tokens.len() {
            match tokens[cursor] {
                CssToken::Semicolon => {
                    cursor += 1;
                    break;
                }
                CssToken::CurlyClose => break,
                _ => {
                    value.push(tokens[cursor].clone());
                    cursor += 1;
                }
            }
        }
        let important = consume_important(&mut value);
        trim_whitespace(&mut value);
        let property_id = crate::property_id::lookup_property_id(&name);
        declarations.push(Declaration {
            name,
            value,
            important,
            property_id,
        });
    }
    declarations
}
fn skip_component_value(tokens: &[CssToken], mut cursor: usize) -> usize {
    let mut depth = 0usize;
    while cursor < tokens.len() {
        match tokens[cursor] {
            CssToken::Semicolon if depth == 0 => return cursor + 1,
            CssToken::CurlyOpen | CssToken::ParenOpen | CssToken::BracketOpen => depth += 1,
            CssToken::CurlyClose | CssToken::ParenClose | CssToken::BracketClose => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        cursor += 1;
    }
    cursor
}
fn consume_important(value: &mut Vec<CssToken>) -> bool {
    trim_whitespace(value);
    if value.len() < 2 {
        return false;
    }
    let mut end = value.len();
    if let Some(CssToken::Ident(ident)) = value.get(end - 1) {
        if ident.eq_ignore_ascii_case("important") {
            end -= 1;
            while end > 0 && matches!(value[end - 1], CssToken::Whitespace) {
                end -= 1;
            }
            if end > 0 && matches!(value[end - 1], CssToken::Delim('!')) {
                value.truncate(end - 1);
                trim_whitespace(value);
                return true;
            }
        }
    }
    false
}

fn trim_whitespace(tokens: &mut Vec<CssToken>) {
    let start = tokens
        .iter()
        .position(|token| !matches!(token, CssToken::Whitespace));
    let end = tokens
        .iter()
        .rposition(|token| !matches!(token, CssToken::Whitespace));
    match (start, end) {
        (Some(start), Some(end)) => {
            let keep_len = end + 1 - start;
            if start > 0 {
                tokens.drain(0..start);
            }
            if tokens.len() > keep_len {
                tokens.truncate(keep_len);
            }
        }
        _ => tokens.clear(),
    }
}
