/*
 * supports.rs -- @supports condition evaluation (CSS Conditional Rules 3, 2.4).
 *
 * A declaration condition asks whether this engine honors `property: value`.
 * The answer comes from the cascade itself: `style::engine_applies_declaration`
 * runs the declaration through `apply_declaration` against a fresh cascaded
 * style and reports whether any slot was set. A property outside `PropertyId`
 * or a value the parser rejects sets nothing, so the condition is false and the
 * author's fallback branch applies.
 *
 * Every other `<supports-in-parens>` production -- `selector()`, `font-tech()`,
 * `font-format()`, and general-enclosed forms -- evaluates false, because a
 * true answer would promise a surface the engine does not implement.
 */

use crate::CssToken;

/// Evaluate an `@supports` prelude. An empty prelude is false: a condition-less
/// `@supports` is invalid, and its block does not apply.
#[must_use]
pub fn evaluate_supports_condition(prelude: &[CssToken]) -> bool {
    let mut cursor = SupportsCursor::new(prelude);
    let Some(value) = cursor.parse_condition() else {
        return false;
    };
    cursor.skip_whitespace();
    // Trailing tokens mean the prelude did not parse as one condition.
    cursor.at_end() && value
}

struct SupportsCursor<'a> {
    tokens: &'a [CssToken],
    index: usize,
}

impl<'a> SupportsCursor<'a> {
    fn new(tokens: &'a [CssToken]) -> Self {
        Self { tokens, index: 0 }
    }

    fn at_end(&self) -> bool {
        self.index >= self.tokens.len()
            || self.tokens[self.index..]
                .iter()
                .all(|t| matches!(t, CssToken::Eof))
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.tokens.get(self.index),
            Some(CssToken::Whitespace | CssToken::Cdo | CssToken::Cdc)
        ) {
            self.index += 1;
        }
    }

    fn peek_keyword(&mut self) -> Option<String> {
        self.skip_whitespace();
        match self.tokens.get(self.index) {
            Some(CssToken::Ident(name)) => Some(name.to_ascii_lowercase()),
            _ => None,
        }
    }

    /// `not <in-parens>` | `<in-parens> [ and <in-parens> ]*`
    /// | `<in-parens> [ or <in-parens> ]*`
    fn parse_condition(&mut self) -> Option<bool> {
        if self.peek_keyword().as_deref() == Some("not") {
            self.index += 1;
            return self.parse_in_parens().map(|value| !value);
        }
        let mut value = self.parse_in_parens()?;
        let mut combinator: Option<String> = None;
        loop {
            let Some(keyword) = self.peek_keyword() else {
                return Some(value);
            };
            if keyword != "and" && keyword != "or" {
                return Some(value);
            }
            // CSS forbids mixing `and` with `or` at one level without nesting.
            if combinator.get_or_insert(keyword.clone()) != &keyword {
                return None;
            }
            self.index += 1;
            let operand = self.parse_in_parens()?;
            value = if keyword == "and" {
                value && operand
            } else {
                value || operand
            };
        }
    }

    /// `( <condition> )` | `( <declaration> )` | `<function>( ... )`
    fn parse_in_parens(&mut self) -> Option<bool> {
        self.skip_whitespace();
        match self.tokens.get(self.index) {
            Some(CssToken::ParenOpen) => {
                self.index += 1;
                let inner = self.take_balanced_group()?;
                Some(evaluate_group(&inner))
            }
            // selector(), font-tech(), font-format(), and any unknown function
            // are general-enclosed: the engine reports no support.
            Some(CssToken::Function(_)) => {
                self.index += 1;
                self.take_balanced_group()?;
                Some(false)
            }
            _ => None,
        }
    }

    /// Consume tokens through the matching `)`, returning the interior.
    fn take_balanced_group(&mut self) -> Option<Vec<CssToken>> {
        let mut depth = 1usize;
        let mut inner = Vec::new();
        while let Some(token) = self.tokens.get(self.index) {
            self.index += 1;
            match token {
                CssToken::ParenOpen | CssToken::Function(_) => depth += 1,
                CssToken::ParenClose => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(inner);
                    }
                }
                CssToken::Eof => return None,
                _ => {}
            }
            inner.push(token.clone());
        }
        None
    }
}

/// Decide whether a parenthesized group is a declaration or a nested condition.
/// A declaration opens with `<ident> :`; anything else re-enters the grammar.
fn evaluate_group(inner: &[CssToken]) -> bool {
    let mut index = 0usize;
    while matches!(inner.get(index), Some(CssToken::Whitespace)) {
        index += 1;
    }
    let is_declaration = matches!(inner.get(index), Some(CssToken::Ident(_))) && {
        let mut lookahead = index + 1;
        while matches!(inner.get(lookahead), Some(CssToken::Whitespace)) {
            lookahead += 1;
        }
        matches!(inner.get(lookahead), Some(CssToken::Colon))
    };
    if is_declaration {
        return crate::parser::parse_declarations_from_tokens(inner)
            .iter()
            .any(crate::style::engine_applies_declaration);
    }
    let mut cursor = SupportsCursor::new(inner);
    let value = cursor.parse_condition().unwrap_or(false);
    cursor.skip_whitespace();
    cursor.at_end() && value
}

#[cfg(test)]
mod tests {
    use super::evaluate_supports_condition;
    use crate::CssTokenizer;

    fn prelude(text: &str) -> Vec<crate::CssToken> {
        let mut tokenizer = CssTokenizer::new();
        let mut tokens = tokenizer.feed(text).expect("tokenize");
        tokens.extend(tokenizer.finish().expect("finish"));
        tokens.retain(|t| !matches!(t, crate::CssToken::Eof));
        tokens
    }

    #[test]
    fn supported_declaration_evaluates_true() {
        assert!(evaluate_supports_condition(&prelude("(display: flex)")));
    }

    #[test]
    fn unknown_property_evaluates_false() {
        assert!(!evaluate_supports_condition(&prelude(
            "(field-sizing: content)"
        )));
    }

    #[test]
    fn unparsed_value_evaluates_false() {
        assert!(!evaluate_supports_condition(&prelude("(height: 100dvb)")));
    }

    #[test]
    fn negation_inverts_the_declaration_result() {
        assert!(evaluate_supports_condition(&prelude(
            "not (height: 100dvb)"
        )));
        assert!(!evaluate_supports_condition(&prelude(
            "not (display: flex)"
        )));
    }

    #[test]
    fn conjunction_requires_both_operands() {
        assert!(evaluate_supports_condition(&prelude(
            "(display: flex) and (color: red)"
        )));
        assert!(!evaluate_supports_condition(&prelude(
            "(display: flex) and (field-sizing: content)"
        )));
    }

    #[test]
    fn disjunction_accepts_one_operand() {
        assert!(evaluate_supports_condition(&prelude(
            "(field-sizing: content) or (display: flex)"
        )));
    }

    #[test]
    fn nested_negation_inside_conjunction_parses() {
        assert!(evaluate_supports_condition(&prelude(
            "(display: flex) and (not (field-sizing: content))"
        )));
    }

    #[test]
    fn selector_function_reports_no_support() {
        assert!(!evaluate_supports_condition(&prelude("selector(a:hover)")));
    }

    #[test]
    fn empty_prelude_reports_no_support() {
        assert!(!evaluate_supports_condition(&prelude("")));
    }

    #[test]
    fn mixed_and_or_without_nesting_reports_no_support() {
        assert!(!evaluate_supports_condition(&prelude(
            "(display: flex) and (color: red) or (display: block)"
        )));
    }
}
