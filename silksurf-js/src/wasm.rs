//! WebAssembly bindings for SilkSurfJS
//!
//! Provides a JavaScript-friendly API for running SilkSurfJS in the browser
//! or Node.js via WebAssembly.
//!
//! # Example (JavaScript)
//!
//! ```javascript
//! import init, { Engine } from './silksurf_js.js';
//!
//! await init();
//! const engine = new Engine();
//! const result = engine.eval("1 + 2");
//! console.log(result); // "3"
//! ```

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

use crate::bytecode::Compiler;
use crate::lexer::Lexer;
use crate::parser::ast_arena::AstArena;
use crate::parser::Parser;
use crate::vm::Vm;

/// JavaScript engine instance for WASM
#[wasm_bindgen]
pub struct Engine {
    vm: Vm,
}

#[wasm_bindgen]
impl Engine {
    /// Create a new engine instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // Set up panic hook for better error messages in browser
        #[cfg(feature = "wasm")]
        console_error_panic_hook::set_once();

        Self { vm: Vm::new() }
    }

    /// Evaluate JavaScript source code and return the result as a string
    #[wasm_bindgen]
    pub fn eval(&mut self, source: &str) -> Result<String, JsValue> {
        // Lex
        let lexer = Lexer::new(source);
        for token in lexer {
            if let crate::lexer::TokenKind::Error(e) = &token.kind {
                return Err(JsValue::from_str(&format!("Lexer error: {}", e)));
            }
        }

        // Parse
        let ast_arena = AstArena::new();
        let parser = Parser::new(source, &ast_arena);
        let (ast, errors) = parser.parse();
        if !errors.is_empty() {
            return Err(JsValue::from_str(&format!("Parse error: {:?}", errors[0])));
        }

        // Compile
        let compiler = Compiler::new();
        let chunk = match compiler.compile(&ast) {
            Ok(c) => c,
            Err(e) => return Err(JsValue::from_str(&format!("Compile error: {:?}", e))),
        };

        // Execute
        let chunk_idx = self.vm.add_chunk(chunk);
        match self.vm.execute(chunk_idx) {
            Ok(value) => Ok(format!("{:?}", value)),
            Err(e) => Err(JsValue::from_str(&format!("Runtime error: {:?}", e))),
        }
    }

    /// Check if source code is syntactically valid
    #[wasm_bindgen]
    pub fn check_syntax(&self, source: &str) -> bool {
        // Lex
        let lexer = Lexer::new(source);
        for token in lexer {
            if matches!(token.kind, crate::lexer::TokenKind::Error(_)) {
                return false;
            }
        }

        // Parse
        let ast_arena = AstArena::new();
        let parser = Parser::new(source, &ast_arena);
        let (_, errors) = parser.parse();
        errors.is_empty()
    }

    /// Get the engine version
    #[wasm_bindgen]
    pub fn version() -> String {
        "0.1.0".to_string()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize the WASM module (called automatically by wasm-bindgen)
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "wasm")]
    console_error_panic_hook::set_once();
}

#[cfg(test)]
mod tests {
    // WASM tests would run in wasm-pack test
}
