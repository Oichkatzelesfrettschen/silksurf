//! Core orchestration for the SilkSurf engine.
//!
//! This crate wires together parsing, layout, rendering, networking,
//! and the JavaScript runtime. Concrete implementations are introduced
//! in staged migration phases per the cleanroom plan.

use silksurf_dom::{Dom, NodeId};
use silksurf_html::{TokenizeError, Tokenizer, TreeBuildError, TreeBuilder};

#[derive(Debug)]
pub enum EngineError {
    Tokenize(TokenizeError),
    TreeBuild(TreeBuildError),
}

impl From<TokenizeError> for EngineError {
    fn from(error: TokenizeError) -> Self {
        EngineError::Tokenize(error)
    }
}

impl From<TreeBuildError> for EngineError {
    fn from(error: TreeBuildError) -> Self {
        EngineError::TreeBuild(error)
    }
}

pub struct ParsedDocument {
    pub dom: Dom,
    pub document: NodeId,
}

pub fn parse_html(input: &str) -> Result<ParsedDocument, EngineError> {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens)?;
    let document = builder.document_id();
    let dom = builder.into_dom();
    Ok(ParsedDocument { dom, document })
}
