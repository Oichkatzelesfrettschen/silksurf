//! CSS `calc()` expression evaluation.
//!
//! Parses and evaluates `calc()` expressions like:
//! - calc(100% - 20px)
//! - calc(50px + 2em)
//! - calc(100vw / 3)

use crate::{CssToken, Length};

/// A calc expression AST node.
#[derive(Debug, Clone, PartialEq)]
pub enum CalcExpr {
    Value(Length),
    Number(f32),
    Add(Box<CalcExpr>, Box<CalcExpr>),
    Sub(Box<CalcExpr>, Box<CalcExpr>),
    Mul(Box<CalcExpr>, Box<CalcExpr>),
    Div(Box<CalcExpr>, Box<CalcExpr>),
}

impl CalcExpr {
    /// Evaluate the expression to a concrete px value.
    ///
    /// `percentage_basis_px` is the containing-block or font-size basis for
    /// percentage values. Relative units use the bases supplied by the
    /// resolve pass, so viewport units remain unresolved until that pass.
    #[must_use]
    pub fn evaluate(
        &self,
        percentage_basis_px: f32,
        em_px: f32,
        rem_px: f32,
        viewport: (f32, f32),
    ) -> f32 {
        match self {
            CalcExpr::Value(Length::Px(v)) => *v,
            CalcExpr::Value(Length::Percent(p)) => percentage_basis_px * p / 100.0,
            CalcExpr::Value(Length::Em(v)) => em_px * v,
            CalcExpr::Value(Length::Rem(v)) => rem_px * v,
            CalcExpr::Value(Length::Vw(v)) => viewport.0 * v / 100.0,
            CalcExpr::Value(Length::Vh(v)) => viewport.1 * v / 100.0,
            CalcExpr::Value(Length::Vmin(v)) => viewport.0.min(viewport.1) * v / 100.0,
            CalcExpr::Value(Length::Vmax(v)) => viewport.0.max(viewport.1) * v / 100.0,
            CalcExpr::Value(Length::Calc(_)) => {
                unreachable!("calc AST leaves do not contain nested length handles")
            }
            CalcExpr::Number(n) => *n,
            CalcExpr::Add(a, b) => {
                a.evaluate(percentage_basis_px, em_px, rem_px, viewport)
                    + b.evaluate(percentage_basis_px, em_px, rem_px, viewport)
            }
            CalcExpr::Sub(a, b) => {
                a.evaluate(percentage_basis_px, em_px, rem_px, viewport)
                    - b.evaluate(percentage_basis_px, em_px, rem_px, viewport)
            }
            CalcExpr::Mul(a, b) => {
                a.evaluate(percentage_basis_px, em_px, rem_px, viewport)
                    * b.evaluate(percentage_basis_px, em_px, rem_px, viewport)
            }
            CalcExpr::Div(a, b) => {
                let divisor = b.evaluate(percentage_basis_px, em_px, rem_px, viewport);
                if divisor == 0.0 {
                    0.0
                } else {
                    a.evaluate(percentage_basis_px, em_px, rem_px, viewport) / divisor
                }
            }
        }
    }

    /// Return whether the expression still depends on a percentage basis.
    #[must_use]
    pub(crate) fn contains_percentage(&self) -> bool {
        match self {
            CalcExpr::Value(Length::Percent(_)) => true,
            CalcExpr::Value(_) | CalcExpr::Number(_) => false,
            CalcExpr::Add(a, b)
            | CalcExpr::Sub(a, b)
            | CalcExpr::Mul(a, b)
            | CalcExpr::Div(a, b) => a.contains_percentage() || b.contains_percentage(),
        }
    }

    /// Replace relative units with pixels while preserving percentage terms.
    #[must_use]
    pub(crate) fn resolve_units(&self, em_px: f32, rem_px: f32, viewport: (f32, f32)) -> Self {
        match self {
            CalcExpr::Value(Length::Px(value) | Length::Percent(value)) => {
                CalcExpr::Value(if matches!(self, CalcExpr::Value(Length::Percent(_))) {
                    Length::Percent(*value)
                } else {
                    Length::Px(*value)
                })
            }
            CalcExpr::Value(Length::Em(value)) => CalcExpr::Value(Length::Px(em_px * value)),
            CalcExpr::Value(Length::Rem(value)) => CalcExpr::Value(Length::Px(rem_px * value)),
            CalcExpr::Value(Length::Vw(value)) => {
                CalcExpr::Value(Length::Px(viewport.0 * value / 100.0))
            }
            CalcExpr::Value(Length::Vh(value)) => {
                CalcExpr::Value(Length::Px(viewport.1 * value / 100.0))
            }
            CalcExpr::Value(Length::Vmin(value)) => {
                CalcExpr::Value(Length::Px(viewport.0.min(viewport.1) * value / 100.0))
            }
            CalcExpr::Value(Length::Vmax(value)) => {
                CalcExpr::Value(Length::Px(viewport.0.max(viewport.1) * value / 100.0))
            }
            CalcExpr::Value(Length::Calc(_)) => {
                unreachable!("calc AST leaves do not contain nested length handles")
            }
            CalcExpr::Number(value) => CalcExpr::Number(*value),
            CalcExpr::Add(a, b) => CalcExpr::Add(
                Box::new(a.resolve_units(em_px, rem_px, viewport)),
                Box::new(b.resolve_units(em_px, rem_px, viewport)),
            ),
            CalcExpr::Sub(a, b) => CalcExpr::Sub(
                Box::new(a.resolve_units(em_px, rem_px, viewport)),
                Box::new(b.resolve_units(em_px, rem_px, viewport)),
            ),
            CalcExpr::Mul(a, b) => CalcExpr::Mul(
                Box::new(a.resolve_units(em_px, rem_px, viewport)),
                Box::new(b.resolve_units(em_px, rem_px, viewport)),
            ),
            CalcExpr::Div(a, b) => CalcExpr::Div(
                Box::new(a.resolve_units(em_px, rem_px, viewport)),
                Box::new(b.resolve_units(em_px, rem_px, viewport)),
            ),
        }
    }
}

/// Parse a `calc()` expression from CSS tokens (tokens inside the `calc()` parens).
#[must_use]
pub fn parse_calc(tokens: &[CssToken]) -> Option<CalcExpr> {
    let filtered: Vec<&CssToken> = tokens
        .iter()
        .filter(|t| !matches!(t, CssToken::Whitespace))
        .collect();
    let mut pos = 0;
    let expression = parse_additive(&filtered, &mut pos)?;
    (pos == filtered.len()).then_some(expression)
}

fn parse_additive(tokens: &[&CssToken], pos: &mut usize) -> Option<CalcExpr> {
    let mut left = parse_multiplicative(tokens, pos)?;
    while *pos < tokens.len() {
        match tokens.get(*pos) {
            Some(CssToken::Delim('+')) => {
                *pos += 1;
                let right = parse_multiplicative(tokens, pos)?;
                left = CalcExpr::Add(Box::new(left), Box::new(right));
            }
            Some(CssToken::Delim('-')) => {
                *pos += 1;
                let right = parse_multiplicative(tokens, pos)?;
                left = CalcExpr::Sub(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Some(left)
}

fn parse_multiplicative(tokens: &[&CssToken], pos: &mut usize) -> Option<CalcExpr> {
    let mut left = parse_primary(tokens, pos)?;
    while *pos < tokens.len() {
        match tokens.get(*pos) {
            Some(CssToken::Delim('*')) => {
                *pos += 1;
                let right = parse_primary(tokens, pos)?;
                left = CalcExpr::Mul(Box::new(left), Box::new(right));
            }
            Some(CssToken::Delim('/')) => {
                *pos += 1;
                let right = parse_primary(tokens, pos)?;
                left = CalcExpr::Div(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Some(left)
}

fn parse_primary(tokens: &[&CssToken], pos: &mut usize) -> Option<CalcExpr> {
    if *pos >= tokens.len() {
        return None;
    }
    match tokens[*pos] {
        CssToken::ParenOpen => {
            *pos += 1;
            let expr = parse_additive(tokens, pos)?;
            if !matches!(tokens.get(*pos), Some(CssToken::ParenClose)) {
                return None;
            }
            *pos += 1;
            Some(expr)
        }
        CssToken::Dimension { value, unit } => {
            *pos += 1;
            let v = value.parse::<f32>().ok()?;
            let length = match unit.to_ascii_lowercase().as_str() {
                "px" => Length::Px(v),
                "em" => Length::Em(v),
                "rem" => Length::Rem(v),
                "vw" | "vi" => Length::Vw(v),
                "vh" | "vb" => Length::Vh(v),
                "vmin" => Length::Vmin(v),
                "vmax" => Length::Vmax(v),
                "dvw" | "lvw" | "svw" => Length::Vw(v),
                "dvh" | "lvh" | "svh" => Length::Vh(v),
                "dvi" | "lvi" | "svi" => Length::Vw(v),
                "dvb" | "lvb" | "svb" => Length::Vh(v),
                _ => return None,
            };
            Some(CalcExpr::Value(length))
        }
        CssToken::Percentage(value) => {
            *pos += 1;
            let v = value.parse::<f32>().ok()?;
            Some(CalcExpr::Value(Length::Percent(v)))
        }
        CssToken::Number(value) => {
            *pos += 1;
            let v = value.parse::<f32>().ok()?;
            Some(CalcExpr::Number(v))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tokens(s: &str) -> Vec<CssToken> {
        // Simple tokenizer for tests
        let mut tokens = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&ch) = chars.peek() {
            match ch {
                ' ' | '\t' => {
                    chars.next();
                    tokens.push(CssToken::Whitespace);
                }
                '+' | '-' | '*' | '/' => {
                    chars.next();
                    tokens.push(CssToken::Delim(ch));
                }
                '(' => {
                    chars.next();
                    tokens.push(CssToken::ParenOpen);
                }
                ')' => {
                    chars.next();
                    tokens.push(CssToken::ParenClose);
                }
                '0'..='9' | '.' => {
                    let mut num = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() || c == '.' {
                            num.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(&'%') = chars.peek() {
                        chars.next();
                        tokens.push(CssToken::Percentage(num.as_str().into()));
                    } else if let Some(&'p') = chars.peek() {
                        chars.next();
                        if let Some(&'x') = chars.peek() {
                            chars.next();
                        }
                        tokens.push(CssToken::Dimension {
                            value: num.as_str().into(),
                            unit: "px".into(),
                        });
                    } else {
                        tokens.push(CssToken::Number(num.as_str().into()));
                    }
                }
                _ => {
                    chars.next();
                }
            }
        }
        tokens
    }

    #[test]
    fn test_simple_addition() {
        let tokens = make_tokens("100px + 50px");
        let expr = parse_calc(&tokens).unwrap();
        assert!((expr.evaluate(0.0, 16.0, 16.0, (1280.0, 800.0)) - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_percentage_subtraction() {
        let tokens = make_tokens("100% - 20px");
        let expr = parse_calc(&tokens).unwrap();
        // Context 500px: 100% = 500px, 500 - 20 = 480
        assert!((expr.evaluate(500.0, 16.0, 16.0, (1280.0, 800.0)) - 480.0).abs() < 0.01);
    }

    #[test]
    fn test_multiplication() {
        let tokens = make_tokens("10px * 3");
        let expr = parse_calc(&tokens).unwrap();
        assert!((expr.evaluate(0.0, 16.0, 16.0, (1280.0, 800.0)) - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_division() {
        let tokens = make_tokens("100px / 4");
        let expr = parse_calc(&tokens).unwrap();
        assert!((expr.evaluate(0.0, 16.0, 16.0, (1280.0, 800.0)) - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_complex() {
        // (100% - 40px) / 2
        let tokens = make_tokens("(100% - 40px) / 2");
        let expr = parse_calc(&tokens).unwrap();
        // Context 800px: (800 - 40) / 2 = 380
        assert!((expr.evaluate(800.0, 16.0, 16.0, (1280.0, 800.0)) - 380.0).abs() < 0.01);
    }

    #[test]
    fn malformed_expression_with_a_missing_operand_is_rejected() {
        let tokens = make_tokens("10px +");
        assert!(parse_calc(&tokens).is_none());
    }

    #[test]
    fn malformed_expression_with_an_unclosed_parenthesis_is_rejected() {
        let tokens = make_tokens("(10px + 5px");
        assert!(parse_calc(&tokens).is_none());
    }
}
