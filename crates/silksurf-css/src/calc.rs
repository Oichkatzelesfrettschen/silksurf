//! CSS calc() expression evaluation.
//!
//! Parses and evaluates calc() expressions like:
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
    /// `context_px` is the reference size for percentage values (e.g., parent width).
    pub fn evaluate(&self, context_px: f32) -> f32 {
        match self {
            CalcExpr::Value(Length::Px(v)) => *v,
            CalcExpr::Value(Length::Percent(p)) => context_px * p / 100.0,
            CalcExpr::Value(Length::Em(v)) | CalcExpr::Value(Length::Rem(v)) => *v,
            CalcExpr::Number(n) => *n,
            CalcExpr::Add(a, b) => a.evaluate(context_px) + b.evaluate(context_px),
            CalcExpr::Sub(a, b) => a.evaluate(context_px) - b.evaluate(context_px),
            CalcExpr::Mul(a, b) => a.evaluate(context_px) * b.evaluate(context_px),
            CalcExpr::Div(a, b) => {
                let divisor = b.evaluate(context_px);
                if divisor == 0.0 {
                    0.0
                } else {
                    a.evaluate(context_px) / divisor
                }
            }
        }
    }
}

/// Parse a calc() expression from CSS tokens (tokens inside the calc() parens).
pub fn parse_calc(tokens: &[CssToken]) -> Option<CalcExpr> {
    let filtered: Vec<&CssToken> = tokens
        .iter()
        .filter(|t| !matches!(t, CssToken::Whitespace))
        .collect();
    parse_additive(&filtered, &mut 0)
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
            if matches!(tokens.get(*pos), Some(CssToken::ParenClose)) {
                *pos += 1;
            }
            Some(expr)
        }
        CssToken::Dimension { value, unit } => {
            *pos += 1;
            let v = value.parse::<f32>().ok()?;
            if unit.eq_ignore_ascii_case("px") {
                Some(CalcExpr::Value(Length::Px(v)))
            } else if unit == "%" {
                Some(CalcExpr::Value(Length::Percent(v)))
            } else {
                Some(CalcExpr::Value(Length::Px(v))) // Treat unknown units as px
            }
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
        assert!((expr.evaluate(0.0) - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_percentage_subtraction() {
        let tokens = make_tokens("100% - 20px");
        let expr = parse_calc(&tokens).unwrap();
        // Context 500px: 100% = 500px, 500 - 20 = 480
        assert!((expr.evaluate(500.0) - 480.0).abs() < 0.01);
    }

    #[test]
    fn test_multiplication() {
        let tokens = make_tokens("10px * 3");
        let expr = parse_calc(&tokens).unwrap();
        assert!((expr.evaluate(0.0) - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_division() {
        let tokens = make_tokens("100px / 4");
        let expr = parse_calc(&tokens).unwrap();
        assert!((expr.evaluate(0.0) - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_complex() {
        // (100% - 40px) / 2
        let tokens = make_tokens("(100% - 40px) / 2");
        let expr = parse_calc(&tokens).unwrap();
        // Context 800px: (800 - 40) / 2 = 380
        assert!((expr.evaluate(800.0) - 380.0).abs() < 0.01);
    }
}
