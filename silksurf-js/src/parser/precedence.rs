//! Operator precedence for Pratt parsing
//!
//! Pratt parsing handles left-recursive grammars elegantly.
//! Each operator has binding power (precedence) and associativity.
//! Higher binding power = binds tighter.

use crate::lexer::TokenKind;
use crate::parser::ast::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};

/// Binding power for operators (higher = binds tighter)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BindingPower(pub u8);

impl BindingPower {
    pub const MIN: Self = Self(0);
    pub const COMMA: Self = Self(1);
    pub const ASSIGNMENT: Self = Self(3);
    pub const CONDITIONAL: Self = Self(5);
    pub const NULLISH: Self = Self(6);
    pub const LOGICAL_OR: Self = Self(7);
    pub const LOGICAL_AND: Self = Self(8);
    pub const BITWISE_OR: Self = Self(9);
    pub const BITWISE_XOR: Self = Self(10);
    pub const BITWISE_AND: Self = Self(11);
    pub const EQUALITY: Self = Self(12);
    pub const RELATIONAL: Self = Self(13);
    pub const SHIFT: Self = Self(14);
    pub const ADDITIVE: Self = Self(15);
    pub const MULTIPLICATIVE: Self = Self(16);
    pub const EXPONENTIATION: Self = Self(17);
    pub const UNARY: Self = Self(18);
    pub const UPDATE: Self = Self(19);
    pub const CALL: Self = Self(20);
    pub const NEW: Self = Self(21);
    pub const PRIMARY: Self = Self(22);
}

/// Get infix binding power: (`left_bp`, `right_bp`)
/// Left-associative: `left_bp` < `right_bp`
/// Right-associative: `left_bp` > `right_bp`
#[must_use]
pub fn infix_binding_power(kind: &TokenKind) -> Option<(BindingPower, BindingPower)> {
    let bp = match kind {
        // Comma - left associative
        TokenKind::Comma => (BindingPower::COMMA, BindingPower(2)),

        // Assignment - right associative
        TokenKind::Assign
        | TokenKind::PlusAssign
        | TokenKind::MinusAssign
        | TokenKind::StarAssign
        | TokenKind::SlashAssign
        | TokenKind::PercentAssign
        | TokenKind::StarStarAssign
        | TokenKind::LeftShiftAssign
        | TokenKind::RightShiftAssign
        | TokenKind::UnsignedRightShiftAssign
        | TokenKind::AmpersandAssign
        | TokenKind::PipeAssign
        | TokenKind::CaretAssign
        | TokenKind::AmpersandAmpersandAssign
        | TokenKind::PipePipeAssign
        | TokenKind::QuestionQuestionAssign => (BindingPower(4), BindingPower::ASSIGNMENT),

        // Conditional - right associative
        TokenKind::Question => (BindingPower::CONDITIONAL, BindingPower(4)),

        // Nullish coalescing - left associative
        TokenKind::QuestionQuestion => (BindingPower::NULLISH, BindingPower(7)),

        // Logical OR
        TokenKind::PipePipe => (BindingPower::LOGICAL_OR, BindingPower(8)),

        // Logical AND
        TokenKind::AmpersandAmpersand => (BindingPower::LOGICAL_AND, BindingPower(9)),

        // Bitwise OR
        TokenKind::Pipe => (BindingPower::BITWISE_OR, BindingPower(10)),

        // Bitwise XOR
        TokenKind::Caret => (BindingPower::BITWISE_XOR, BindingPower(11)),

        // Bitwise AND
        TokenKind::Ampersand => (BindingPower::BITWISE_AND, BindingPower(12)),

        // Equality
        TokenKind::Equal
        | TokenKind::NotEqual
        | TokenKind::StrictEqual
        | TokenKind::StrictNotEqual => (BindingPower::EQUALITY, BindingPower(13)),

        // Relational
        TokenKind::LessThan
        | TokenKind::GreaterThan
        | TokenKind::LessEqual
        | TokenKind::GreaterEqual
        | TokenKind::In
        | TokenKind::Instanceof => (BindingPower::RELATIONAL, BindingPower(14)),

        // Shift
        TokenKind::LeftShift | TokenKind::RightShift | TokenKind::UnsignedRightShift => {
            (BindingPower::SHIFT, BindingPower(15))
        }

        // Additive
        TokenKind::Plus | TokenKind::Minus => (BindingPower::ADDITIVE, BindingPower(16)),

        // Multiplicative
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => {
            (BindingPower::MULTIPLICATIVE, BindingPower(17))
        }

        // Exponentiation - right associative
        TokenKind::StarStar => (BindingPower(18), BindingPower::EXPONENTIATION),

        _ => return None,
    };

    Some(bp)
}

/// Get prefix binding power
#[must_use]
pub fn prefix_binding_power(kind: &TokenKind) -> Option<BindingPower> {
    match kind {
        // Unary operators
        TokenKind::Bang
        | TokenKind::Tilde
        | TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Typeof
        | TokenKind::Void
        | TokenKind::Delete
        | TokenKind::Await => Some(BindingPower::UNARY),

        // Prefix update operators
        TokenKind::PlusPlus | TokenKind::MinusMinus => Some(BindingPower::UPDATE),
        TokenKind::Yield => Some(BindingPower(4)),

        // New
        TokenKind::New => Some(BindingPower::NEW),

        _ => None,
    }
}

/// Get postfix binding power
#[must_use]
pub fn postfix_binding_power(kind: &TokenKind) -> Option<BindingPower> {
    match kind {
        TokenKind::PlusPlus | TokenKind::MinusMinus => Some(BindingPower::UPDATE),
        TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::Dot | TokenKind::QuestionDot => {
            Some(BindingPower::CALL)
        }
        TokenKind::Template(_) => Some(BindingPower::CALL),
        _ => None,
    }
}

/// Convert token to binary operator
#[must_use]
pub fn token_to_binary_op(kind: &TokenKind) -> Option<BinaryOperator> {
    match kind {
        TokenKind::Plus => Some(BinaryOperator::Add),
        TokenKind::Minus => Some(BinaryOperator::Sub),
        TokenKind::Star => Some(BinaryOperator::Mul),
        TokenKind::Slash => Some(BinaryOperator::Div),
        TokenKind::Percent => Some(BinaryOperator::Mod),
        TokenKind::StarStar => Some(BinaryOperator::Pow),
        TokenKind::Equal => Some(BinaryOperator::Eq),
        TokenKind::NotEqual => Some(BinaryOperator::Ne),
        TokenKind::StrictEqual => Some(BinaryOperator::StrictEq),
        TokenKind::StrictNotEqual => Some(BinaryOperator::StrictNe),
        TokenKind::LessThan => Some(BinaryOperator::Lt),
        TokenKind::LessEqual => Some(BinaryOperator::Le),
        TokenKind::GreaterThan => Some(BinaryOperator::Gt),
        TokenKind::GreaterEqual => Some(BinaryOperator::Ge),
        TokenKind::Pipe => Some(BinaryOperator::BitwiseOr),
        TokenKind::Caret => Some(BinaryOperator::BitwiseXor),
        TokenKind::Ampersand => Some(BinaryOperator::BitwiseAnd),
        TokenKind::LeftShift => Some(BinaryOperator::ShiftLeft),
        TokenKind::RightShift => Some(BinaryOperator::ShiftRight),
        TokenKind::UnsignedRightShift => Some(BinaryOperator::UnsignedShiftRight),
        TokenKind::In => Some(BinaryOperator::In),
        TokenKind::Instanceof => Some(BinaryOperator::InstanceOf),
        _ => None,
    }
}

/// Convert token to logical operator
#[must_use]
pub fn token_to_logical_op(kind: &TokenKind) -> Option<LogicalOperator> {
    match kind {
        TokenKind::AmpersandAmpersand => Some(LogicalOperator::And),
        TokenKind::PipePipe => Some(LogicalOperator::Or),
        TokenKind::QuestionQuestion => Some(LogicalOperator::NullishCoalescing),
        _ => None,
    }
}

/// Convert token to assignment operator
#[must_use]
pub fn token_to_assignment_op(kind: &TokenKind) -> Option<AssignmentOperator> {
    match kind {
        TokenKind::Assign => Some(AssignmentOperator::Assign),
        TokenKind::PlusAssign => Some(AssignmentOperator::AddAssign),
        TokenKind::MinusAssign => Some(AssignmentOperator::SubAssign),
        TokenKind::StarAssign => Some(AssignmentOperator::MulAssign),
        TokenKind::SlashAssign => Some(AssignmentOperator::DivAssign),
        TokenKind::PercentAssign => Some(AssignmentOperator::ModAssign),
        TokenKind::StarStarAssign => Some(AssignmentOperator::PowAssign),
        TokenKind::LeftShiftAssign => Some(AssignmentOperator::ShiftLeftAssign),
        TokenKind::RightShiftAssign => Some(AssignmentOperator::ShiftRightAssign),
        TokenKind::UnsignedRightShiftAssign => Some(AssignmentOperator::UnsignedShiftRightAssign),
        TokenKind::PipeAssign => Some(AssignmentOperator::BitwiseOrAssign),
        TokenKind::CaretAssign => Some(AssignmentOperator::BitwiseXorAssign),
        TokenKind::AmpersandAssign => Some(AssignmentOperator::BitwiseAndAssign),
        TokenKind::PipePipeAssign => Some(AssignmentOperator::LogicalOrAssign),
        TokenKind::AmpersandAmpersandAssign => Some(AssignmentOperator::LogicalAndAssign),
        TokenKind::QuestionQuestionAssign => Some(AssignmentOperator::NullishAssign),
        _ => None,
    }
}

/// Convert token to unary operator
#[must_use]
pub fn token_to_unary_op(kind: &TokenKind) -> Option<UnaryOperator> {
    match kind {
        TokenKind::Minus => Some(UnaryOperator::Minus),
        TokenKind::Plus => Some(UnaryOperator::Plus),
        TokenKind::Bang => Some(UnaryOperator::Not),
        TokenKind::Tilde => Some(UnaryOperator::BitwiseNot),
        TokenKind::Typeof => Some(UnaryOperator::Typeof),
        TokenKind::Void => Some(UnaryOperator::Void),
        TokenKind::Delete => Some(UnaryOperator::Delete),
        _ => None,
    }
}

/// Convert token to update operator
#[must_use]
pub fn token_to_update_op(kind: &TokenKind) -> Option<UpdateOperator> {
    match kind {
        TokenKind::PlusPlus => Some(UpdateOperator::Increment),
        TokenKind::MinusMinus => Some(UpdateOperator::Decrement),
        _ => None,
    }
}

/// Check if token is an assignment operator
#[must_use]
pub fn is_assignment_op(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Assign
            | TokenKind::PlusAssign
            | TokenKind::MinusAssign
            | TokenKind::StarAssign
            | TokenKind::SlashAssign
            | TokenKind::PercentAssign
            | TokenKind::StarStarAssign
            | TokenKind::LeftShiftAssign
            | TokenKind::RightShiftAssign
            | TokenKind::UnsignedRightShiftAssign
            | TokenKind::PipeAssign
            | TokenKind::CaretAssign
            | TokenKind::AmpersandAssign
            | TokenKind::PipePipeAssign
            | TokenKind::AmpersandAmpersandAssign
            | TokenKind::QuestionQuestionAssign
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_power_ordering() {
        assert!(BindingPower::COMMA < BindingPower::ASSIGNMENT);
        assert!(BindingPower::ASSIGNMENT < BindingPower::LOGICAL_OR);
        assert!(BindingPower::ADDITIVE < BindingPower::MULTIPLICATIVE);
        assert!(BindingPower::MULTIPLICATIVE < BindingPower::UNARY);
    }

    #[test]
    fn test_infix_operators() {
        // Addition is left-associative
        let (left, right) = infix_binding_power(&TokenKind::Plus).unwrap();
        assert!(left < right);

        // Exponentiation is right-associative
        let (left, right) = infix_binding_power(&TokenKind::StarStar).unwrap();
        assert!(left > right);
    }
}
