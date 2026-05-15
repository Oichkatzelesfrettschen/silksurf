//! Kani verification proofs for `SilkSurfJS`
//!
//! Uses SMT solvers (bitwuzla, STP, z3) via Kani for formal verification.
//! Run with: cargo kani --solver bitwuzla

#[cfg(kani)]
mod proofs {
    use crate::lexer::{Lexer, Span, TokenKind};
    use crate::parser::Parser;
    use crate::parser::ast_arena::AstArena;

    /// Verify that Span creation never panics for valid inputs
    #[kani::proof]
    fn verify_span_creation() {
        let start: u32 = kani::any();
        let end: u32 = kani::any();

        // Assume valid range (start <= end)
        kani::assume(start <= end);

        let span = Span::new(start, end);

        // Verify invariants
        assert!(span.start <= span.end);
        assert_eq!(span.start, start);
        assert_eq!(span.end, end);
    }

    /// Verify that Span length calculation is correct
    #[kani::proof]
    fn verify_span_length() {
        let start: u32 = kani::any();
        let end: u32 = kani::any();

        kani::assume(start <= end);
        kani::assume(end - start <= 1000); // Bound for tractability

        let span = Span::new(start, end);
        let len = span.len();

        assert_eq!(len, end - start);
    }

    /// Verify lexer never panics on arbitrary ASCII input
    #[kani::proof]
    #[kani::unwind(16)]
    fn verify_lexer_no_panic_ascii() {
        // Generate small ASCII input
        let len: usize = kani::any();
        kani::assume(len <= 8);

        let mut bytes = [0u8; 8];
        for i in 0..len {
            bytes[i] = kani::any();
            // Restrict to printable ASCII + whitespace
            kani::assume(
                bytes[i] >= 0x20 && bytes[i] <= 0x7E || bytes[i] == b'\n' || bytes[i] == b'\t',
            );
        }

        // Convert to string (safe since we restricted to ASCII)
        if let Ok(source) = std::str::from_utf8(&bytes[..len]) {
            let mut lexer = Lexer::new(source);

            // Scan all tokens - should not panic
            loop {
                let token = lexer.next_token();
                if matches!(token.kind, TokenKind::Eof) {
                    break;
                }
            }
        }
    }

    /// Verify parser produces valid AST for simple expressions
    #[kani::proof]
    #[kani::unwind(8)]
    fn verify_parser_simple_expression() {
        // Test with a simple numeric literal
        let source = "42;";
        let arena = AstArena::new();
        let parser = Parser::new(source, &arena);
        let (program, errors) = parser.parse();

        // Should parse without errors
        assert!(errors.is_empty());
        // Should produce one statement
        assert_eq!(program.body.len(), 1);
    }

    /// Verify that token span is always within source bounds
    #[kani::proof]
    #[kani::unwind(16)]
    fn verify_token_span_bounds() {
        let len: usize = kani::any();
        kani::assume(len > 0 && len <= 8);

        let bytes = [b'a'; 8]; // Simple identifier chars

        if let Ok(source) = std::str::from_utf8(&bytes[..len]) {
            let mut lexer = Lexer::new(source);

            loop {
                let token = lexer.next_token();

                // Verify span is within source bounds
                assert!(token.span.start as usize <= source.len());
                assert!(token.span.end as usize <= source.len());
                assert!(token.span.start <= token.span.end);

                if matches!(token.kind, TokenKind::Eof) {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_verification_module_compiles() {
        // Just verify the module compiles correctly -- no assertion needed
    }
}
