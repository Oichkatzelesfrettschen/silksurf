use crate::CssToken;
use silksurf_core::{should_intern_identifier, Atom, SilkInterner, SmallString};
use silksurf_dom::{AttributeName, TagName};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct SelectorIdent {
    value: SmallString,
    atom: Option<Atom>,
}

impl SelectorIdent {
    pub fn new(value: &str) -> Self {
        Self {
            value: SmallString::from(value),
            atom: None,
        }
    }

    pub fn new_with_interner(value: &str, interner: &mut SilkInterner) -> Self {
        let value = SmallString::from(value);
        let atom = if should_intern_identifier(value.as_str()) {
            Some(interner.intern(value.as_str()))
        } else {
            None
        };
        Self { value, atom }
    }

    pub fn new_with_atom(value: SmallString, atom: Atom) -> Self {
        Self {
            value,
            atom: Some(atom),
        }
    }

    pub fn intern_with(&mut self, interner: &mut SilkInterner) {
        if self.atom.is_none() && should_intern_identifier(self.value.as_str()) {
            self.atom = Some(interner.intern(self.value.as_str()));
        }
    }

    pub fn as_str(&self) -> &str {
        self.value.as_str()
    }

    pub fn atom(&self) -> Option<Atom> {
        self.atom
    }
}

impl PartialEq for SelectorIdent {
    fn eq(&self, other: &Self) -> bool {
        match (self.atom, other.atom) {
            (Some(left), Some(right)) => left == right,
            _ => self.value == other.value,
        }
    }
}

impl Eq for SelectorIdent {}

impl Hash for SelectorIdent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the string value to keep Hash/Eq consistent when atoms are absent.
        self.value.hash(state);
    }
}

impl From<&str> for SelectorIdent {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<SmallString> for SelectorIdent {
    fn from(value: SmallString) -> Self {
        Self { value, atom: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorList {
    pub selectors: Vec<Selector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    pub steps: Vec<SelectorStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorStep {
    pub combinator: Option<Combinator>,
    pub compound: CompoundSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    pub type_selector: Option<TypeSelector>,
    pub modifiers: Vec<SelectorModifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSelector {
    Any,
    Tag(TagName),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorModifier {
    Class(SelectorIdent),
    Id(SelectorIdent),
    Attribute(AttributeSelector),
    PseudoClass(SelectorIdent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeSelector {
    pub name: AttributeName,
    pub operator: Option<AttributeOperator>,
    pub value: Option<SelectorIdent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeOperator {
    Equals,
    Includes,
    DashMatch,
    PrefixMatch,
    SuffixMatch,
    SubstringMatch,
}

impl SelectorList {
    pub fn intern_with(&mut self, interner: &mut SilkInterner) {
        for selector in &mut self.selectors {
            selector.intern_with(interner);
        }
    }
}

impl Selector {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        for step in &mut self.steps {
            step.intern_with(interner);
        }
    }
}

impl SelectorStep {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        self.compound.intern_with(interner);
    }
}

impl CompoundSelector {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        for modifier in &mut self.modifiers {
            modifier.intern_with(interner);
        }
    }
}

impl SelectorModifier {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        match self {
            SelectorModifier::Class(name)
            | SelectorModifier::Id(name)
            | SelectorModifier::PseudoClass(name) => name.intern_with(interner),
            SelectorModifier::Attribute(_) => {}
        }
    }
}

pub fn parse_selector_list(tokens: Vec<CssToken>) -> SelectorList {
    parse_selector_list_with_interner(tokens, None)
}

pub fn parse_selector_list_with_interner(
    tokens: Vec<CssToken>,
    interner: Option<&mut SilkInterner>,
) -> SelectorList {
    let mut parser = SelectorParser::new(tokens, interner);
    parser.parse_selector_list()
}

struct SelectorParser<'a> {
    tokens: Vec<CssToken>,
    cursor: usize,
    interner: Option<&'a mut SilkInterner>,
}

impl<'a> SelectorParser<'a> {
    fn new(mut tokens: Vec<CssToken>, interner: Option<&'a mut SilkInterner>) -> Self {
        if !matches!(tokens.last(), Some(CssToken::Eof)) {
            tokens.push(CssToken::Eof);
        }
        Self {
            tokens,
            cursor: 0,
            interner,
        }
    }
    fn parse_selector_list(&mut self) -> SelectorList {
        let mut selectors = Vec::new();
        self.consume_whitespace();
        while !self.is_eof() {
            if let Some(selector) = self.parse_selector() {
                selectors.push(selector);
            } else {
                self.next();
            }
            self.consume_whitespace();
            if matches!(self.peek(), Some(CssToken::Comma)) {
                self.next();
                self.consume_whitespace();
            }
        }
        SelectorList { selectors }
    }
    fn parse_selector(&mut self) -> Option<Selector> {
        let mut steps = Vec::new();
        let mut combinator = None;
        loop {
            let saw_whitespace = self.consume_whitespace();
            if !steps.is_empty() && saw_whitespace {
                combinator = Some(Combinator::Descendant);
            }
            if let Some(explicit) = self.consume_combinator() {
                combinator = Some(explicit);
                self.consume_whitespace();
            }
            let compound = match self.parse_compound_selector() {
                Some(compound) => compound,
                None => break,
            };
            steps.push(SelectorStep { combinator, compound });
            combinator = None;
            if matches!(self.peek(), Some(CssToken::Comma | CssToken::Eof)) {
                break;
            }
        }
        if steps.is_empty() {
            None
        } else {
            Some(Selector { steps })
        }
    }
    fn parse_compound_selector(&mut self) -> Option<CompoundSelector> {
        let mut type_selector = None;
        match self.peek() {
            Some(CssToken::Ident(name)) => {
                type_selector = Some(TypeSelector::Tag(TagName::from_str(name)));
                self.next();
            }
            Some(CssToken::Delim('*')) => {
                type_selector = Some(TypeSelector::Any);
                self.next();
            }
            _ => {}
        }
        let mut modifiers = Vec::new();
        while let Some(modifier) = self.parse_modifier() {
            modifiers.push(modifier);
        }
        if type_selector.is_none() && modifiers.is_empty() {
            None
        } else {
            Some(CompoundSelector {
                type_selector,
                modifiers,
            })
        }
    }
    fn parse_modifier(&mut self) -> Option<SelectorModifier> {
        match self.peek() {
            Some(CssToken::Delim('.')) => self.parse_class_selector(),
            Some(CssToken::Hash(_)) => self.parse_id_selector(),
            Some(CssToken::BracketOpen) => self.parse_attribute_selector(),
            Some(CssToken::Colon) => self.parse_pseudo_class(),
            _ => None,
        }
    }
    fn parse_class_selector(&mut self) -> Option<SelectorModifier> {
        self.next();
        match self.next() {
            Some(CssToken::Ident(name)) => Some(SelectorModifier::Class(self.make_ident(&name))),
            _ => None,
        }
    }

    fn parse_id_selector(&mut self) -> Option<SelectorModifier> {
        match self.next() {
            Some(CssToken::Hash(name)) => Some(SelectorModifier::Id(self.make_ident(&name))),
            _ => None,
        }
    }

    fn parse_attribute_selector(&mut self) -> Option<SelectorModifier> {
        self.next();
        self.consume_whitespace();
        let name = match self.next() {
            Some(CssToken::Ident(name)) => AttributeName::from_str(&name),
            _ => {
                self.skip_to_bracket_close();
                return None;
            }
        };
        self.consume_whitespace();
        let mut operator = None;
        let mut value = None;
        if let Some(op) = self.parse_attribute_operator() {
            operator = Some(op);
            self.consume_whitespace();
            value = self.parse_attribute_value();
            self.consume_whitespace();
        }
        if !matches!(self.peek(), Some(CssToken::BracketClose)) {
            self.skip_to_bracket_close();
            return None;
        }
        self.next();
        Some(SelectorModifier::Attribute(AttributeSelector {
            name,
            operator,
            value,
        }))
    }
    fn parse_pseudo_class(&mut self) -> Option<SelectorModifier> {
        self.next();
        if matches!(self.peek(), Some(CssToken::Colon)) {
            self.next();
        }
        match self.next() {
            Some(CssToken::Ident(name)) => {
                Some(SelectorModifier::PseudoClass(self.make_ident(&name)))
            }
            Some(CssToken::Function(name)) => {
                self.skip_parens();
                Some(SelectorModifier::PseudoClass(self.make_ident(&name)))
            }
            _ => None,
        }
    }

    fn parse_attribute_operator(&mut self) -> Option<AttributeOperator> {
        match self.peek() {
            Some(CssToken::Delim('=')) => {
                self.next();
                Some(AttributeOperator::Equals)
            }
            Some(CssToken::Delim('~')) if matches!(self.peek_n(1), Some(CssToken::Delim('='))) => {
                self.next();
                self.next();
                Some(AttributeOperator::Includes)
            }
            Some(CssToken::Delim('|')) if matches!(self.peek_n(1), Some(CssToken::Delim('='))) => {
                self.next();
                self.next();
                Some(AttributeOperator::DashMatch)
            }
            Some(CssToken::Delim('^')) if matches!(self.peek_n(1), Some(CssToken::Delim('='))) => {
                self.next();
                self.next();
                Some(AttributeOperator::PrefixMatch)
            }
            Some(CssToken::Delim('$')) if matches!(self.peek_n(1), Some(CssToken::Delim('='))) => {
                self.next();
                self.next();
                Some(AttributeOperator::SuffixMatch)
            }
            Some(CssToken::Delim('*')) if matches!(self.peek_n(1), Some(CssToken::Delim('='))) => {
                self.next();
                self.next();
                Some(AttributeOperator::SubstringMatch)
            }
            _ => None,
        }
    }
    fn parse_attribute_value(&mut self) -> Option<SelectorIdent> {
        match self.next() {
            Some(CssToken::Ident(value)) => Some(self.make_ident(&value)),
            Some(CssToken::String(value)) => Some(self.make_ident(&value)),
            Some(CssToken::Number(value)) => Some(self.make_ident(&value)),
            Some(CssToken::Dimension { value, unit }) => {
                Some(self.make_ident(&format!("{}{}", value, unit)))
            }
            _ => None,
        }
    }

    fn make_ident(&mut self, value: &str) -> SelectorIdent {
        match self.interner.as_deref_mut() {
            Some(interner) => SelectorIdent::new_with_interner(value, interner),
            None => SelectorIdent::new(value),
        }
    }

    fn consume_combinator(&mut self) -> Option<Combinator> {
        match self.peek() {
            Some(CssToken::Delim('>')) => {
                self.next();
                Some(Combinator::Child)
            }
            Some(CssToken::Delim('+')) => {
                self.next();
                Some(Combinator::NextSibling)
            }
            Some(CssToken::Delim('~')) => {
                self.next();
                Some(Combinator::SubsequentSibling)
            }
            _ => None,
        }
    }
    fn consume_whitespace(&mut self) -> bool {
        let mut consumed = false;
        while matches!(self.peek(), Some(CssToken::Whitespace)) {
            self.next();
            consumed = true;
        }
        consumed
    }

    fn peek(&self) -> Option<&CssToken> {
        self.tokens.get(self.cursor)
    }

    fn peek_n(&self, offset: usize) -> Option<&CssToken> {
        self.tokens.get(self.cursor + offset)
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
    fn skip_to_bracket_close(&mut self) {
        while let Some(token) = self.next() {
            if matches!(token, CssToken::BracketClose | CssToken::Eof) {
                break;
            }
        }
    }

    fn skip_parens(&mut self) {
        let mut depth = 0usize;
        while let Some(token) = self.next() {
            match token {
                CssToken::ParenOpen => depth += 1,
                CssToken::ParenClose => {
                    if depth == 0 {
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                CssToken::Eof => break,
                _ => {}
            }
        }
    }
}
