//! CSS text serialization for the CSSOM.
//!
//! `CSSRule.cssText`, `CSSStyleRule.selectorText`, and
//! `CSSStyleDeclaration.cssText` each hand script the text form of a value the
//! parser produced, so the CSSOM reads back what `parse_stylesheet` stored.
//! Serialization runs over the parsed tree rather than the source text: a rule
//! a script inserts has no source, and a rule the parser normalized no longer
//! matches the bytes it came from.

use std::fmt::Write;

use crate::CssToken;
use crate::parser::{AtRule, AtRuleBlock, Declaration, Rule, StyleRule};
use crate::selector::{
    AttributeOperator, AttributeSelector, Combinator, CompoundSelector, PseudoClassArg, Selector,
    SelectorList, SelectorModifier, SelectorStep, TypeSelector,
};

/// Serialize one rule as `CSSRule.cssText` returns it.
#[must_use]
pub fn rule_to_css(rule: &Rule) -> String {
    let mut out = String::new();
    write_rule(rule, &mut out);
    out
}

/// Serialize a style rule's selector as `CSSStyleRule.selectorText` returns it.
#[must_use]
pub fn selector_list_to_css(selectors: &SelectorList) -> String {
    let mut out = String::new();
    write_selector_list(selectors, &mut out);
    out
}

/// Serialize a declaration block as `CSSStyleDeclaration.cssText` returns it.
///
/// Each declaration ends in a semicolon, so the result parses back as a
/// declaration list without the caller rejoining anything.
#[must_use]
pub fn declarations_to_css(declarations: &[Declaration]) -> String {
    let mut out = String::new();
    for (index, declaration) in declarations.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        write_declaration(declaration, &mut out);
    }
    out
}

/// Serialize one declaration as `name: value` without its terminator.
#[must_use]
pub fn declaration_to_css(declaration: &Declaration) -> String {
    let mut out = String::new();
    out.push_str(&declaration.name);
    out.push_str(": ");
    out.push_str(&value_to_css(&declaration.value));
    if declaration.important {
        out.push_str(" !important");
    }
    out
}

/// Serialize a declaration value, collapsing the whitespace runs the tokenizer
/// preserved down to the single spaces CSSOM component-value serialization
/// produces.
#[must_use]
pub fn value_to_css(value: &[CssToken]) -> String {
    let mut out = String::new();
    for token in value {
        write_token(token, &mut out);
    }
    collapse_whitespace(&out)
}

fn write_rule(rule: &Rule, out: &mut String) {
    match rule {
        Rule::Style(style) => write_style_rule(style, out),
        Rule::At(at_rule) => write_at_rule(at_rule, out),
    }
}

fn write_style_rule(rule: &StyleRule, out: &mut String) {
    write_selector_list(&rule.selectors, out);
    out.push_str(" { ");
    write_declaration_block(&rule.declarations, out);
    out.push('}');
}

fn write_declaration_block(declarations: &[Declaration], out: &mut String) {
    for declaration in declarations {
        write_declaration(declaration, out);
        out.push(' ');
    }
}

fn write_declaration(declaration: &Declaration, out: &mut String) {
    out.push_str(&declaration_to_css(declaration));
    out.push(';');
}

fn write_at_rule(rule: &AtRule, out: &mut String) {
    out.push('@');
    out.push_str(&rule.name);
    let prelude = value_to_css(&rule.prelude);
    if !prelude.is_empty() {
        out.push(' ');
        out.push_str(&prelude);
    }
    match &rule.block {
        None => out.push(';'),
        Some(AtRuleBlock::Declarations(declarations)) => {
            out.push_str(" { ");
            write_declaration_block(declarations, out);
            out.push('}');
        }
        Some(AtRuleBlock::Rules(rules)) => {
            out.push_str(" { ");
            for nested in rules {
                write_rule(nested, out);
                out.push(' ');
            }
            out.push('}');
        }
    }
}

fn write_selector_list(list: &SelectorList, out: &mut String) {
    for (index, selector) in list.selectors.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write_selector(selector, out);
    }
}

fn write_selector(selector: &Selector, out: &mut String) {
    for step in &selector.steps {
        write_step(step, out);
    }
}

fn write_step(step: &SelectorStep, out: &mut String) {
    match step.combinator {
        None => {}
        Some(Combinator::Descendant) => out.push(' '),
        Some(Combinator::Child) => out.push_str(" > "),
        Some(Combinator::NextSibling) => out.push_str(" + "),
        Some(Combinator::SubsequentSibling) => out.push_str(" ~ "),
    }
    write_compound(&step.compound, out);
}

fn write_compound(compound: &CompoundSelector, out: &mut String) {
    match &compound.type_selector {
        Some(TypeSelector::Any) => out.push('*'),
        Some(TypeSelector::Tag(tag)) => out.push_str(tag.as_str()),
        // A compound carrying only modifiers writes no type selector: the
        // universal selector is implied and CSSOM omits it.
        None => {}
    }
    for modifier in &compound.modifiers {
        write_modifier(modifier, out);
    }
}

fn write_modifier(modifier: &SelectorModifier, out: &mut String) {
    match modifier {
        SelectorModifier::Class(ident) => {
            out.push('.');
            out.push_str(ident.as_str());
        }
        SelectorModifier::Id(ident) => {
            out.push('#');
            out.push_str(ident.as_str());
        }
        SelectorModifier::PseudoClass(ident) => {
            out.push(':');
            out.push_str(ident.as_str());
        }
        SelectorModifier::Attribute(attribute) => write_attribute(attribute, out),
        SelectorModifier::FunctionalPseudoClass { name, arg } => {
            out.push(':');
            out.push_str(name.as_str());
            out.push('(');
            write_pseudo_arg(arg, out);
            out.push(')');
        }
    }
}

fn write_attribute(attribute: &AttributeSelector, out: &mut String) {
    out.push('[');
    out.push_str(attribute.name.as_str());
    if let Some(operator) = attribute.operator {
        out.push_str(match operator {
            AttributeOperator::Equals => "=",
            AttributeOperator::Includes => "~=",
            AttributeOperator::DashMatch => "|=",
            AttributeOperator::PrefixMatch => "^=",
            AttributeOperator::SuffixMatch => "$=",
            AttributeOperator::SubstringMatch => "*=",
        });
        if let Some(value) = &attribute.value {
            out.push('"');
            write_string_body(value.as_str(), out);
            out.push('"');
        }
    }
    out.push(']');
}

fn write_pseudo_arg(arg: &PseudoClassArg, out: &mut String) {
    match arg {
        PseudoClassArg::SelectorList(list) => write_selector_list(list, out),
        PseudoClassArg::Nth(nth) => {
            if nth.a == 0 {
                out.push_str(&nth.b.to_string());
                return;
            }
            match nth.a {
                1 => out.push('n'),
                -1 => out.push_str("-n"),
                a => {
                    out.push_str(&a.to_string());
                    out.push('n');
                }
            }
            if nth.b > 0 {
                let _ = write!(out, "+{}", nth.b);
            } else if nth.b < 0 {
                out.push_str(&nth.b.to_string());
            }
        }
    }
}

fn write_token(token: &CssToken, out: &mut String) {
    match token {
        CssToken::AtKeyword(name) => {
            out.push('@');
            out.push_str(name);
        }
        CssToken::Ident(value) | CssToken::Number(value) => out.push_str(value),
        CssToken::Function(name) => {
            out.push_str(name);
            out.push('(');
        }
        CssToken::Hash(value) => {
            out.push('#');
            out.push_str(value);
        }
        CssToken::String(value) => {
            out.push('"');
            write_string_body(value, out);
            out.push('"');
        }
        CssToken::Percentage(value) => {
            out.push_str(value);
            out.push('%');
        }
        CssToken::Dimension { value, unit } => {
            out.push_str(value);
            out.push_str(unit);
        }
        CssToken::Url(value) => {
            out.push_str("url(\"");
            write_string_body(value, out);
            out.push_str("\")");
        }
        CssToken::Delim(c) => out.push(*c),
        CssToken::Colon => out.push(':'),
        CssToken::Semicolon => out.push(';'),
        CssToken::Comma => out.push(','),
        CssToken::CurlyOpen => out.push('{'),
        CssToken::CurlyClose => out.push('}'),
        CssToken::ParenOpen => out.push('('),
        CssToken::ParenClose => out.push(')'),
        CssToken::BracketOpen => out.push('['),
        CssToken::BracketClose => out.push(']'),
        CssToken::Whitespace => out.push(' '),
        CssToken::Cdo => out.push_str("<!--"),
        CssToken::Cdc => out.push_str("-->"),
        CssToken::UnicodeRange { start, end } => {
            if start == end {
                let _ = write!(out, "U+{start:X}");
            } else {
                let _ = write!(out, "U+{start:X}-{end:X}");
            }
        }
        // A bad string or URL has no round-trippable text, and Eof is the
        // terminator CssTokenizer::finish appends rather than page content.
        CssToken::BadString | CssToken::BadUrl | CssToken::Eof => {}
    }
}

fn write_string_body(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\A "),
            other => out.push(other),
        }
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c == ' ' {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}
