use crate::CssToken;
use silksurf_core::{Atom, SilkInterner, SmallString, should_intern_identifier};
use silksurf_dom::{AttributeName, TagName};
use smallvec::SmallVec;
use std::hash::{Hash, Hasher};

/*
 * SelectorIdent -- internable CSS identifier with SmallString + optional Atom.
 *
 * Serialization: only `value` is stored; `atom` is always None after
 * deserialization and repopulated by intern_rules() at render time.
 * This keeps the serialized form interner-agnostic.
 */
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectorIdent {
    value: SmallString,
    #[serde(skip)]
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

    /// Clear the interned atom so this SelectorIdent can be stored in the
    /// stylesheet cache without holding a reference to a specific interner.
    /// The `value` SmallString is retained for string-equality fallback.
    pub fn clear_atom(&mut self) {
        self.atom = None;
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

/*
 * NthIndex -- parsed An+B value for :nth-child and related pseudo-classes.
 *
 * The CSS Selectors L4 An+B notation: element at 1-based position p matches
 * when p == a*n + b for some integer n >= 0.
 *
 * Special keyword mappings:
 *   odd  -> a=2, b=1   even -> a=2, b=0
 *   n    -> a=1, b=0   -n   -> a=-1, b=0
 */
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NthIndex {
    pub a: i32,
    pub b: i32,
}

impl NthIndex {
    pub fn matches(&self, position: usize) -> bool {
        if position == 0 {
            return false;
        }
        let p = position as i32;
        if self.a == 0 {
            return p == self.b;
        }
        // n = (p - b) / a must be a non-negative integer.
        let diff = p - self.b;
        diff % self.a == 0 && diff / self.a >= 0
    }
}

/*
 * PseudoClassArg -- argument for functional pseudo-classes.
 *
 * Nth: an+b argument for :nth-child, :nth-of-type, etc.
 * SelectorList: selector argument for :not, :is, :where, :has.
 *
 * Box<SelectorList> breaks the recursive type: SelectorList contains
 * Selector which contains CompoundSelector which contains SelectorModifier
 * which contains FunctionalPseudoClass which contains PseudoClassArg which
 * would otherwise recursively embed SelectorList.
 */
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PseudoClassArg {
    Nth(NthIndex),
    SelectorList(Box<SelectorList>),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectorList {
    pub selectors: SmallVec<[Selector; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Selector {
    pub steps: SmallVec<[SelectorStep; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectorStep {
    pub combinator: Option<Combinator>,
    pub compound: CompoundSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompoundSelector {
    pub type_selector: Option<TypeSelector>,
    pub modifiers: SmallVec<[SelectorModifier; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeSelector {
    Any,
    Tag(TagName),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectorModifier {
    Class(SelectorIdent),
    Id(SelectorIdent),
    Attribute(AttributeSelector),
    PseudoClass(SelectorIdent),
    FunctionalPseudoClass { name: SelectorIdent, arg: PseudoClassArg },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttributeSelector {
    pub name: AttributeName,
    pub operator: Option<AttributeOperator>,
    pub value: Option<SelectorIdent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    fn strip_atoms(&mut self) {
        for selector in &mut self.selectors {
            selector.strip_atoms();
        }
    }
}

impl Selector {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        for step in &mut self.steps {
            step.intern_with(interner);
        }
    }

    fn strip_atoms(&mut self) {
        for step in &mut self.steps {
            step.strip_atoms();
        }
    }
}

impl SelectorStep {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        self.compound.intern_with(interner);
    }

    fn strip_atoms(&mut self) {
        self.compound.strip_atoms();
    }
}

impl CompoundSelector {
    fn intern_with(&mut self, interner: &mut SilkInterner) {
        for modifier in &mut self.modifiers {
            modifier.intern_with(interner);
        }
    }

    fn strip_atoms(&mut self) {
        for modifier in &mut self.modifiers {
            modifier.strip_atoms();
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
            SelectorModifier::FunctionalPseudoClass { name, arg } => {
                name.intern_with(interner);
                if let PseudoClassArg::SelectorList(list) = arg {
                    list.intern_with(interner);
                }
            }
        }
    }

    fn strip_atoms(&mut self) {
        match self {
            SelectorModifier::Class(name)
            | SelectorModifier::Id(name)
            | SelectorModifier::PseudoClass(name) => name.clear_atom(),
            SelectorModifier::Attribute(attr) => {
                if let Some(ref mut v) = attr.value {
                    v.clear_atom();
                }
            }
            SelectorModifier::FunctionalPseudoClass { name, arg } => {
                name.clear_atom();
                if let PseudoClassArg::SelectorList(list) = arg {
                    list.strip_atoms();
                }
            }
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

/*
 * intern_rules -- populate Atom fields in all StyleRule selectors.
 *
 * WHY: Called after cloning a cached Stylesheet (which has atom=None).
 * Re-interning is O(N_selectors) interner lookups -- ~100-200us for ChatGPT
 * scale CSS, vs 2.5ms for a full re-parse. This is the core of Phase B.
 *
 * INVARIANT: after intern_rules, all SelectorIdents that pass
 * should_intern_identifier() have atom=Some. AtRule prelude tokens
 * do not contain SelectorIdents and are unchanged.
 *
 * See: StylesheetCache.get_or_parse_stylesheet in speculative.rs
 */
pub fn intern_rules(rules: &mut Vec<crate::Rule>, interner: &mut SilkInterner) {
    for rule in rules {
        match rule {
            crate::Rule::Style(sr) => sr.selectors.intern_with(interner),
            crate::Rule::At(ar) => {
                if let Some(crate::AtRuleBlock::Rules(nested)) = &mut ar.block {
                    intern_rules(nested, interner);
                }
            }
        }
    }
}

/*
 * strip_selector_atoms -- clear all interned Atom fields in selector lists.
 *
 * WHY: Before storing a Stylesheet in the StylesheetCache, we strip atoms
 * so the cached copy has no interner-specific state. The SmallString values
 * are preserved for equality fallback. On cache hit, intern_rules repopulates
 * atoms against the current DOM's interner.
 *
 * See: StylesheetCache.get_or_parse_stylesheet in speculative.rs
 */
pub fn strip_selector_atoms(rules: &mut Vec<crate::Rule>) {
    for rule in rules {
        match rule {
            crate::Rule::Style(sr) => sr.selectors.strip_atoms(),
            crate::Rule::At(ar) => {
                if let Some(crate::AtRuleBlock::Rules(nested)) = &mut ar.block {
                    strip_selector_atoms(nested);
                }
            }
        }
    }
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
        let mut selectors = SmallVec::new();
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
        let mut steps = SmallVec::new();
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
            steps.push(SelectorStep {
                combinator,
                compound,
            });
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
        let mut modifiers = SmallVec::new();
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
        self.next(); // consume ':'
        if matches!(self.peek(), Some(CssToken::Colon)) {
            self.next(); // consume second ':' for pseudo-elements
        }
        match self.next() {
            Some(CssToken::Ident(name)) => {
                Some(SelectorModifier::PseudoClass(self.make_ident(&name)))
            }
            Some(CssToken::Function(name)) => {
                // Route functional pseudo-classes to specific parsers; fall back
                // to skip_parens for unrecognized functions (pseudo-elements etc.).
                let lower = name.to_ascii_lowercase();
                match lower.as_str() {
                    "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                        let nth = self.parse_nth_index();
                        let ident = self.make_ident(&name);
                        Some(SelectorModifier::FunctionalPseudoClass {
                            name: ident,
                            arg: PseudoClassArg::Nth(nth),
                        })
                    }
                    "not" | "is" | "where" | "has" => {
                        let inner = self.collect_paren_tokens();
                        let list = parse_selector_list(inner);
                        let ident = self.make_ident(&name);
                        Some(SelectorModifier::FunctionalPseudoClass {
                            name: ident,
                            arg: PseudoClassArg::SelectorList(Box::new(list)),
                        })
                    }
                    _ => {
                        self.skip_parens();
                        Some(SelectorModifier::PseudoClass(self.make_ident(&name)))
                    }
                }
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

    // Consume and discard optional trailing whitespace + closing paren.
    fn skip_parens_close(&mut self) {
        self.consume_whitespace();
        if matches!(self.peek(), Some(CssToken::ParenClose)) {
            self.next();
        }
    }

    // Collect tokens inside matching parens (the opening paren was already
    // consumed by the Function token). Stops and discards the closing paren.
    fn collect_paren_tokens(&mut self) -> Vec<CssToken> {
        let mut tokens = Vec::new();
        let mut depth = 0usize;
        while let Some(token) = self.next() {
            match token {
                CssToken::ParenOpen => {
                    depth += 1;
                    tokens.push(CssToken::ParenOpen);
                }
                CssToken::ParenClose => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    tokens.push(CssToken::ParenClose);
                }
                CssToken::Eof => break,
                _ => tokens.push(token),
            }
        }
        tokens
    }

    // Parse CSS An+B micro-syntax from the current token stream.
    // Called after the Function token (which consumed the opening paren).
    // Consumes up to and including the closing ParenClose.
    fn parse_nth_index(&mut self) -> NthIndex {
        self.consume_whitespace();
        match self.peek().cloned() {
            Some(CssToken::Ident(ident)) => {
                let lower = ident.to_ascii_lowercase();
                match lower.as_str() {
                    "odd" => {
                        self.next();
                        self.skip_parens_close();
                        NthIndex { a: 2, b: 1 }
                    }
                    "even" => {
                        self.next();
                        self.skip_parens_close();
                        NthIndex { a: 2, b: 0 }
                    }
                    "n" => {
                        self.next();
                        let b = self.parse_nth_b_part();
                        self.skip_parens_close();
                        NthIndex { a: 1, b }
                    }
                    "-n" => {
                        self.next();
                        let b = self.parse_nth_b_part();
                        self.skip_parens_close();
                        NthIndex { a: -1, b }
                    }
                    _ => {
                        self.skip_parens();
                        NthIndex { a: 0, b: 0 }
                    }
                }
            }
            Some(CssToken::Number(n)) => {
                let b = n.parse::<i32>().unwrap_or(0);
                self.next();
                self.skip_parens_close();
                NthIndex { a: 0, b }
            }
            // "2n", "-3n", etc.
            Some(CssToken::Dimension { value, unit }) if unit.eq_ignore_ascii_case("n") => {
                let a = value.parse::<i32>().unwrap_or(0);
                self.next();
                let b = self.parse_nth_b_part();
                self.skip_parens_close();
                NthIndex { a, b }
            }
            _ => {
                self.skip_parens();
                NthIndex { a: 0, b: 0 }
            }
        }
    }

    // Parse the optional ['+' | '-'] <integer> suffix after the 'n' part.
    //
    // The CSS tokenizer may produce either Delim('+') + Number("1") or a
    // single signed Number("+1") token -- handle both forms.
    fn parse_nth_b_part(&mut self) -> i32 {
        self.consume_whitespace();
        match self.peek().cloned() {
            Some(CssToken::Delim('+')) => {
                self.next();
                self.consume_whitespace();
                match self.next() {
                    Some(CssToken::Number(n)) => n.parse::<i32>().unwrap_or(0),
                    _ => 0,
                }
            }
            Some(CssToken::Delim('-')) => {
                self.next();
                self.consume_whitespace();
                match self.next() {
                    Some(CssToken::Number(n)) => -(n.parse::<i32>().unwrap_or(0)),
                    _ => 0,
                }
            }
            // Signed number emitted as a single token (e.g. "+1" or "-2")
            Some(CssToken::Number(ref n))
                if n.starts_with('+') || n.starts_with('-') =>
            {
                let val = n.parse::<i32>().unwrap_or(0);
                self.next();
                val
            }
            _ => 0,
        }
    }
}
