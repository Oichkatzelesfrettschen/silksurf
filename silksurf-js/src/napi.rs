//! Node.js native bindings via napi-rs
//!
//! Provides a native Node.js module for running SilkSurfJS.
//!
//! # Building
//!
//! ```bash
//! npm install
//! npm run build
//! ```
//!
//! # Usage (JavaScript/TypeScript)
//!
//! ```javascript
//! const { Engine } = require('silksurf-js');
//!
//! const engine = new Engine();
//! const result = engine.eval("1 + 2");
//! console.log(result); // "3"
//! ```

#![cfg(feature = "napi")]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::bytecode::Compiler;
use crate::lexer::Lexer;
use crate::parser::ast_arena::AstArena;
use crate::parser::Parser;
use crate::vm::Vm;

/// SilkSurfJS engine for Node.js
#[napi]
pub struct Engine {
    vm: Vm,
}

#[napi]
impl Engine {
    /// Create a new engine instance
    #[napi(constructor)]
    pub fn new() -> Self {
        Self { vm: Vm::new() }
    }

    /// Evaluate JavaScript source code and return the result as a string
    #[napi]
    pub fn eval(&mut self, source: String) -> Result<String> {
        // Lex
        let lexer = Lexer::new(&source);
        for token in lexer {
            if let crate::lexer::TokenKind::Error(e) = &token.kind {
                return Err(Error::from_reason(format!("Lexer error: {}", e)));
            }
        }

        // Parse
        let ast_arena = AstArena::new();
        let parser = Parser::new(&source, &ast_arena);
        let (ast, errors) = parser.parse();
        if !errors.is_empty() {
            return Err(Error::from_reason(format!("Parse error: {:?}", errors[0])));
        }

        // Compile
        let compiler = Compiler::new();
        let chunk = match compiler.compile(&ast) {
            Ok(c) => c,
            Err(e) => return Err(Error::from_reason(format!("Compile error: {:?}", e))),
        };

        // Execute
        let chunk_idx = self.vm.add_chunk(chunk);
        match self.vm.execute(chunk_idx) {
            Ok(value) => Ok(format!("{:?}", value)),
            Err(e) => Err(Error::from_reason(format!("Runtime error: {:?}", e))),
        }
    }

    /// Check if source code is syntactically valid
    #[napi]
    pub fn check_syntax(&self, source: String) -> bool {
        // Lex
        let lexer = Lexer::new(&source);
        for token in lexer {
            if matches!(token.kind, crate::lexer::TokenKind::Error(_)) {
                return false;
            }
        }

        // Parse
        let ast_arena = AstArena::new();
        let parser = Parser::new(&source, &ast_arena);
        let (_, errors) = parser.parse();
        errors.is_empty()
    }

    /// Get the engine version
    #[napi]
    pub fn version(&self) -> String {
        "0.1.0".to_string()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the SilkSurfJS version
#[napi]
pub fn get_version() -> String {
    "0.1.0".to_string()
}

/// Quick syntax check without creating an engine
#[napi]
pub fn is_valid_syntax(source: String) -> bool {
    // Lex
    let lexer = Lexer::new(&source);
    for token in lexer {
        if matches!(token.kind, crate::lexer::TokenKind::Error(_)) {
            return false;
        }
    }

    // Parse
    let ast_arena = AstArena::new();
    let parser = Parser::new(&source, &ast_arena);
    let (_, errors) = parser.parse();
    errors.is_empty()
}
