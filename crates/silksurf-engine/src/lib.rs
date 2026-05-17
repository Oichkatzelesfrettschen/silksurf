//! Core orchestration for the `SilkSurf` engine.
//!
//! This crate wires together parsing, layout, rendering, networking,
//! and the JavaScript runtime. Concrete implementations are introduced
//! in staged migration phases per the cleanroom plan.

pub mod fused_pipeline;
mod js;
pub mod privacy;
pub mod sandbox;
#[cfg(feature = "net")]
pub mod speculative;

use rustc_hash::FxHashMap;
use silksurf_core::SilkArena;
use silksurf_css::{
    ComputedStyle, CssError, StyleCache, Stylesheet, parse_stylesheet_with_interner,
};
use silksurf_dom::{Dom, NodeId};
use silksurf_html::parse_html as html5ever_parse;
use silksurf_layout::{LayoutTree, Rect, build_layout_tree, build_layout_tree_incremental};
use silksurf_render::{DisplayList, build_display_list};
use std::sync::Arc;

#[derive(Debug)]
pub enum EngineError {
    Css(CssError),
    Layout(&'static str),
}

pub use js::{JsError, JsRuntime, JsTask, JsValue, NoopJsRuntime};

impl From<CssError> for EngineError {
    fn from(error: CssError) -> Self {
        EngineError::Css(error)
    }
}

impl From<EngineError> for silksurf_core::SilkError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Css(c) => silksurf_core::SilkError::Css {
                offset: c.offset,
                message: c.message,
            },
            EngineError::Layout(msg) => silksurf_core::SilkError::Engine(format!("layout: {msg}")),
        }
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
    #[must_use] 
    pub fn new() -> Self {
        Self {
            style_cache: StyleCache::new(),
        }
    }

    #[must_use] 
    pub fn style_generation(&self) -> u64 {
        self.style_cache.generation()
    }

    /*
     * P8.S6 -- coarse-grained tracing span at the pipeline boundary.
     *
     * WHY: We want structured timing for the three pipeline stages (style /
     * layout / display-list) without paying span-overhead per node.  An
     * outer `info_span!` here bounds the cost to one span enter/exit per
     * render_document call; the sub-stages remain uninstrumented so their
     * tight loops keep their cache behaviour.
     *
     * `skip(self, arena)` keeps the span fields cheap -- arenas/caches do
     * not implement `Debug` and would either be huge or fail to format.
     */
    #[tracing::instrument(
        skip_all,
        fields(viewport_w = viewport.width, viewport_h = viewport.height)
    )]
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

    /*
     * P8.S6 -- incremental render boundary span.
     *
     * Mirrors render_document; we additionally record `dirty_count` because
     * dirty-set size is the primary driver of incremental cost and a
     * useful filter when scanning logs ("show me re-renders with > 100
     * dirty nodes").
     */
    #[tracing::instrument(
        skip_all,
        fields(
            dirty_count = dirty_nodes.len(),
            viewport_w = viewport.width,
            viewport_h = viewport.height
        )
    )]
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

impl Default for EnginePipeline {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_html(input: &str) -> Result<ParsedDocument, EngineError> {
    let dom = html5ever_parse(input);
    // html5ever always produces a well-formed tree rooted at NodeId(0).
    let document = NodeId::from_raw(0);
    Ok(ParsedDocument { dom, document })
}

pub fn render_document(
    document: ParsedDocument,
    stylesheet: Stylesheet,
    viewport: Rect,
    arena: &SilkArena,
) -> Result<RenderOutput<'_>, EngineError> {
    let mut pipeline = EnginePipeline::new();
    pipeline.render_document(document, stylesheet, viewport, arena)
}

/*
 * P8.S6 -- top-level render entry span.
 *
 * `skip_all` covers the large `&str` HTML/CSS payloads (would bloat span
 * fields) and the arena reference.  We include `html_len` and `css_len`
 * as cheap, useful scalars for log filtering.
 */
#[tracing::instrument(skip_all, fields(html_len = html.len(), css_len = css.len()))]
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
