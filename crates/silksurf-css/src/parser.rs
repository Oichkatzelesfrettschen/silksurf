use crate::selector::{parse_selector_list, SelectorList};
use crate::{CssError, CssToken, CssTokenizer};
use silksurf_core::SilkInterner;

#[derive(Debug, Clone, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    Style(StyleRule),
    At(AtRule),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleRule {
    pub selectors: SelectorList,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AtRule {
    pub name: String,
    pub prelude: Vec<CssToken>,
    pub block: Option<AtRuleBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AtRuleBlock {
    Rules(Vec<Rule>),
    Declarations(Vec<Declaration>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: Vec<CssToken>,
    pub important: bool,
}

pub struct CssParser {
    tokens: Vec<CssToken>,
    cursor: usize,
}

impl CssParser {
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
        let name = match self.next() {
            Some(CssToken::AtKeyword(name)) => name,
            _ => return None,
        };
        let mut prelude = Vec::new();
        loop {
            match self.peek() {
                Some(CssToken::Semicolon) => {
                    self.next();
                    return Some(Rule::At(AtRule { name, prelude, block: None }));
                }
                Some(CssToken::CurlyOpen) => {
                    self.next();
                    let block_tokens = self.consume_block();
                    let block = Some(parse_at_rule_block(block_tokens));
                    return Some(Rule::At(AtRule { name, prelude, block }));
                }
                Some(CssToken::Eof) | None => {
                    return Some(Rule::At(AtRule { name, prelude, block: None }));
                }
                _ => {
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
                    let declarations = parse_declarations(block_tokens);
                    let selectors = parse_selector_list(selector_tokens);
                    return Some(Rule::Style(StyleRule { selectors, declarations }));
                }
                Some(CssToken::Eof) | None => return None,
                _ => {
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
        while matches!(self.peek(), Some(CssToken::Whitespace | CssToken::Cdo | CssToken::Cdc)) {
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
        matches!(self.peek(), Some(CssToken::Eof))
    }
}

pub fn parse_stylesheet(input: &str) -> Result<Stylesheet, CssError> {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);
    let mut parser = CssParser::new(tokens);
    Ok(parser.parse_stylesheet())
}

pub fn parse_stylesheet_with_interner(
    input: &str,
    interner: &mut SilkInterner,
) -> Result<Stylesheet, CssError> {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);
    let mut parser = CssParser::new(tokens);
    let mut sheet = parser.parse_stylesheet();
    intern_rules(&mut sheet.rules, interner);
    Ok(sheet)
}
fn parse_at_rule_block(tokens: Vec<CssToken>) -> AtRuleBlock {
    if looks_like_declarations(&tokens) {
        AtRuleBlock::Declarations(parse_declarations(tokens))
    } else {
        let mut parser = CssParser::new(tokens);
        AtRuleBlock::Rules(parser.parse_stylesheet().rules)
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
                while lookahead < tokens.len()
                    && matches!(tokens[lookahead], CssToken::Whitespace)
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
fn parse_declarations(tokens: Vec<CssToken>) -> Vec<Declaration> {
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
            Some(CssToken::AtKeyword(_)) | Some(CssToken::CurlyOpen) | None => break,
            _ => {
                cursor = skip_component_value(&tokens, cursor);
                continue;
            }
        };
        while cursor < tokens.len() && matches!(tokens[cursor], CssToken::Whitespace) {
            cursor += 1;
        }
        if !matches!(tokens.get(cursor), Some(CssToken::Colon)) {
            cursor = skip_component_value(&tokens, cursor);
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
        declarations.push(Declaration {
            name,
            value,
            important,
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
