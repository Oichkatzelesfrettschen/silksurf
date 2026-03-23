//! Core orchestration for the SilkSurf engine.
//!
//! This crate wires together parsing, layout, rendering, networking,
//! and the JavaScript runtime. Concrete implementations are introduced
//! in staged migration phases per the cleanroom plan.

pub mod fused_pipeline;
#[cfg(feature = "net")]
pub mod speculative;
mod js;

use rustc_hash::FxHashMap;
use silksurf_core::SilkArena;
use silksurf_css::{
    ComputedStyle, CssError, StyleCache, Stylesheet, parse_stylesheet_with_interner,
};
use silksurf_dom::{Dom, NodeId};
use silksurf_html::{TokenizeError, Tokenizer, TreeBuildError, TreeBuilder};
use silksurf_layout::{LayoutTree, Rect, build_layout_tree, build_layout_tree_incremental};
use silksurf_render::{DisplayList, build_display_list};
use std::sync::Arc;

#[derive(Debug)]
pub enum EngineError {
    Tokenize(TokenizeError),
    TreeBuild(TreeBuildError),
    Css(CssError),
    Layout(&'static str),
}

pub use js::{JsError, JsRuntime, JsTask, JsValue, NoopJsRuntime};

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

impl From<CssError> for EngineError {
    fn from(error: CssError) -> Self {
        EngineError::Css(error)
    }
}

pub struct ParsedDocument {
    pub dom: Dom,
    pub document: NodeId,
}

pub struct RenderOutput<'a> {
    pub dom: Dom,
    pub document: NodeId,
    pub stylesheet: Stylesheet,
    pub styles: Arc<FxHashMap<NodeId, ComputedStyle>>,
    pub style_generation: u64,
    pub layout: LayoutTree<'a>,
    pub display_list: DisplayList,
}

pub struct EnginePipeline {
    style_cache: StyleCache,
}

impl EnginePipeline {
    pub fn new() -> Self {
        Self {
            style_cache: StyleCache::new(),
        }
    }

    pub fn style_generation(&self) -> u64 {
        self.style_cache.generation()
    }

    pub fn render_document<'a>(
        &mut self,
        document: ParsedDocument,
        stylesheet: Stylesheet,
        viewport: Rect,
        arena: &'a SilkArena,
    ) -> Result<RenderOutput<'a>, EngineError> {
        let ParsedDocument { dom, document } = document;
        let styles = self.style_cache.compute(&dom, document, &stylesheet);
        let layout = build_layout_tree(arena, &dom, styles.as_ref(), document, viewport)
            .ok_or(EngineError::Layout("layout root missing"))?;
        let width = viewport.width.max(0.0).ceil() as u32;
        let height = viewport.height.max(0.0).ceil() as u32;
        let display_list =
            build_display_list(&dom, styles.as_ref(), &layout).with_tiles(width, height, 64);
        Ok(RenderOutput {
            dom,
            document,
            stylesheet,
            styles,
            style_generation: self.style_cache.generation(),
            layout,
            display_list,
        })
    }

    pub fn render_document_incremental<'a>(
        &mut self,
        document: ParsedDocument,
        stylesheet: Stylesheet,
        viewport: Rect,
        arena: &'a SilkArena,
        dirty_nodes: &[NodeId],
    ) -> Result<RenderOutput<'a>, EngineError> {
        let ParsedDocument { dom, document } = document;
        let styles = self
            .style_cache
            .compute_incremental(&dom, document, &stylesheet, dirty_nodes);
        let layout = build_layout_tree_incremental(
            arena,
            &dom,
            styles.as_ref(),
            document,
            viewport,
            dirty_nodes,
        )
        .ok_or(EngineError::Layout("layout root missing"))?;
        let width = viewport.width.max(0.0).ceil() as u32;
        let height = viewport.height.max(0.0).ceil() as u32;
        let display_list =
            build_display_list(&dom, styles.as_ref(), &layout).with_tiles(width, height, 64);
        Ok(RenderOutput {
            dom,
            document,
            stylesheet,
            styles,
            style_generation: self.style_cache.generation(),
            layout,
            display_list,
        })
    }

    pub fn render_document_incremental_from_dom<'a>(
        &mut self,
        mut dom: Dom,
        document: NodeId,
        stylesheet: Stylesheet,
        viewport: Rect,
        arena: &'a SilkArena,
    ) -> Result<RenderOutput<'a>, EngineError> {
        let dirty_nodes = dom.take_dirty_nodes();
        self.render_document_incremental(
            ParsedDocument { dom, document },
            stylesheet,
            viewport,
            arena,
            &dirty_nodes,
        )
    }
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

pub fn render_document<'a>(
    document: ParsedDocument,
    stylesheet: Stylesheet,
    viewport: Rect,
    arena: &'a SilkArena,
) -> Result<RenderOutput<'a>, EngineError> {
    let mut pipeline = EnginePipeline::new();
    pipeline.render_document(document, stylesheet, viewport, arena)
}

pub fn render<'a>(
    html: &str,
    css: &str,
    viewport: Rect,
    arena: &'a SilkArena,
) -> Result<RenderOutput<'a>, EngineError> {
    let document = parse_html(html)?;
    let stylesheet = document
        .dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(css, interner))?;
    render_document(document, stylesheet, viewport, arena)
}
