/*
 * stylesheet_set tracks the document's stylesheets as the DOM presents them
 * rather than as the parse-time snapshot recorded them.
 *
 * CSS Cascade orders author sheets by document position, so the collector
 * walks the tree in order and records one source per `<style>` element and
 * per `<link rel=stylesheet>`. The owning NodeId plus the resolved href is
 * the key: a page that appends a `<style>`, rewrites a link's rel from
 * preload to stylesheet, or swaps an href produces a different key list, and
 * comparing lists is what detects the change.
 *
 * A `media` attribute wraps the source in `@media`, which the cascade
 * evaluates through collect_active_rules, so a print-only sheet contributes
 * no screen rules without a second code path.
 *
 * Link bodies fetch on a worker thread and arrive over an mpsc channel, so
 * the repaint tick never blocks on the network. A URL fetched once stays in
 * `bodies` across recollections, so a list that reorders costs no refetch.
 */

// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Upper bound on link sources a document contributes to the cascade. A
/// document that lists more is truncated at collection, which bounds both the
/// fetch fan-out and the concatenated text the parser sees.
pub(crate) const MAX_DOCUMENT_STYLE_SOURCES: usize = 64;

/// Where one stylesheet's text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StyleSourceKind {
    /// A `<style>` element; the text is the element's concatenated children.
    Inline(String),
    /// A `<link rel=stylesheet>`; the text arrives from the network by URL.
    Link(String),
}

/// One entry in the document's ordered stylesheet list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyleSource {
    pub(crate) owner: silksurf_dom::NodeId,
    pub(crate) kind: StyleSourceKind,
    /// The `media` attribute, empty when the sheet applies unconditionally.
    pub(crate) media: String,
}

impl StyleSource {
    /// The URL this source fetches, for a link source.
    pub(crate) fn link_url(&self) -> Option<&str> {
        match &self.kind {
            StyleSourceKind::Link(url) => Some(url.as_str()),
            StyleSourceKind::Inline(_) => None,
        }
    }
}

/*
 * collect_style_sources -- the document's stylesheets in tree order.
 *
 * Walks from `root` and records a source per `<style>` element and per
 * `<link>` whose rel token list holds `stylesheet`. Tree order is cascade
 * order, so the returned order is the order the concatenated text keeps.
 */
pub(crate) fn collect_style_sources(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<StyleSource> {
    let mut sources = Vec::new();
    collect_style_sources_from(dom, root, base_url, &mut sources);
    sources
}

fn collect_style_sources_from(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    sources: &mut Vec<StyleSource>,
) {
    if sources.len() >= MAX_DOCUMENT_STYLE_SOURCES {
        return;
    }
    if let Some(source) = style_source_for_node(dom, node, base_url) {
        sources.push(source);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_style_sources_from(dom, child, base_url, sources);
        }
    }
}

fn style_source_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<StyleSource> {
    let name = dom.element_name(node).ok().flatten()?;
    let media = element_media_attribute(dom, node);
    match name {
        "style" => Some(StyleSource {
            owner: node,
            kind: StyleSourceKind::Inline(style_element_text(dom, node)),
            media,
        }),
        "link" => {
            link_resource_url_for_node(dom, node, base_url, "stylesheet").map(|url| StyleSource {
                owner: node,
                kind: StyleSourceKind::Link(url),
                media,
            })
        }
        _ => None,
    }
}

/// Whether a node is an element whose attributes can add or remove a
/// stylesheet source. A tick whose dirty set holds none of these skips the
/// tree walk entirely, which is what keeps a React reconcile off it.
pub(crate) fn is_style_source_element(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    matches!(
        dom.element_name(node).ok().flatten(),
        Some("style" | "link")
    )
}

/// The concatenated text children of a `<style>` element.
pub(crate) fn style_element_text(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    let mut text = String::new();
    if let Ok(children) = dom.children(node) {
        for &child in children {
            if let Ok(n) = dom.node(child)
                && let silksurf_dom::NodeKind::Text { text: child_text } = n.kind()
            {
                text.push_str(child_text);
                text.push('\n');
            }
        }
    }
    text
}

fn element_media_attribute(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    let Ok(attrs) = dom.attributes(node) else {
        return String::new();
    };
    let media = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("media"))
        .map_or("", |attr| attr.value.as_str())
        .trim();
    if media.eq_ignore_ascii_case("all") {
        return String::new();
    }
    media.to_string()
}

/*
 * StyleSheetSet -- the live stylesheet list plus the bodies fetched for it.
 *
 * `refresh` gates on Dom::generation so an unchanged document costs one
 * integer comparison; a changed one costs a tree walk and a key comparison.
 * Fetches for link sources without a body run on a worker thread and land
 * through `completions`.
 */
pub(crate) struct StyleSheetSet {
    sources: Vec<StyleSource>,
    bodies: HashMap<String, String>,
    requested: HashSet<String>,
    completions: Receiver<StyleSheetFetch>,
    sender: Sender<StyleSheetFetch>,
    /// Dom::structure_generation at the last collection. A `<style>` or
    /// `<link>` enters the document through a tree-shape edit, so a stable
    /// counter means the source list can only have changed through an
    /// attribute on a node the caller reports as dirty.
    structure_generation: u64,
    /// Dom::generation at the last inline-text read. Text-only edits advance
    /// it alone, so a `<style>` rewritten through textContent is caught by
    /// re-reading the owners already collected rather than by a tree walk.
    dom_generation: u64,
    base_url: String,
    config: BrowserRenderConfig,
}

struct StyleSheetFetch {
    url: String,
    body: Result<String, String>,
}

impl StyleSheetSet {
    /// Build the set from an already-collected source list and the bodies the
    /// blocking initial fetch produced.
    pub(crate) fn new(
        sources: Vec<StyleSource>,
        bodies: Vec<(String, String)>,
        base_url: &str,
        config: &BrowserRenderConfig,
        dom: &silksurf_dom::Dom,
    ) -> Self {
        let mut requested: HashSet<String> = HashSet::new();
        let mut body_map: HashMap<String, String> = HashMap::new();
        for (url, css) in bodies {
            requested.insert(url.clone());
            body_map.insert(url, css);
        }
        let (sender, completions) = channel();
        Self {
            sources,
            bodies: body_map,
            requested,
            completions,
            sender,
            structure_generation: dom.structure_generation(),
            dom_generation: dom.generation(),
            base_url: base_url.to_string(),
            config: config.clone(),
        }
    }

    /// A set holding no sources, for a runtime assembled without a document
    /// fetch. The first refresh collects from the DOM it is handed.
    #[cfg(test)]
    pub(crate) fn empty(base_url: &str) -> Self {
        Self::new(
            Vec::new(),
            Vec::new(),
            base_url,
            &BrowserRenderConfig::default(),
            &silksurf_dom::Dom::new(),
        )
    }

    /// Whether a link source is still waiting on its body. The event loop
    /// polls while this holds, because a fetch landing is the only signal
    /// that would otherwise never wake it.
    pub(crate) fn has_pending_fetches(&self) -> bool {
        self.sources
            .iter()
            .filter_map(StyleSource::link_url)
            .any(|url| !self.bodies.contains_key(url))
    }

    /// How many stylesheets the document currently contributes.
    pub(crate) fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// The concatenated author text, user-agent defaults first, sources in
    /// document order. A link source without a body yet contributes nothing,
    /// so the text is always parseable.
    pub(crate) fn css_text(&self) -> String {
        let mut css = String::from(DEFAULT_USER_AGENT_STYLESHEET);
        css.push('\n');
        for source in &self.sources {
            let text = match &source.kind {
                StyleSourceKind::Inline(text) => text.as_str(),
                StyleSourceKind::Link(url) => match self.bodies.get(url) {
                    Some(body) => body.as_str(),
                    None => continue,
                },
            };
            if text.trim().is_empty() {
                continue;
            }
            if source.media.is_empty() {
                css.push_str(text);
            } else {
                css.push_str("@media ");
                css.push_str(&source.media);
                css.push_str(" {\n");
                css.push_str(text);
                css.push_str("\n}");
            }
            css.push('\n');
        }
        css
    }

    /*
     * refresh -- recollect when the document moved, and report whether the
     * cascade input changed.
     *
     * Returns true when the source list differs or a fetched body landed,
     * which is the condition for rebuilding Stylesheet and StyleIndex.
     */
    pub(crate) fn refresh(
        &mut self,
        dom: &silksurf_dom::Dom,
        root: silksurf_dom::NodeId,
        dirty_nodes: &[silksurf_dom::NodeId],
    ) -> bool {
        let mut changed = self.drain_completions();
        let structure_moved = dom.structure_generation() != self.structure_generation;
        if structure_moved
            || dirty_nodes
                .iter()
                .any(|&node| is_style_source_element(dom, node))
        {
            self.structure_generation = dom.structure_generation();
            let sources = collect_style_sources(dom, root, &self.base_url);
            if sources != self.sources {
                self.sources = sources;
                changed = true;
            }
        }
        if dom.generation() != self.dom_generation {
            self.dom_generation = dom.generation();
            changed |= self.reread_inline_text(dom);
        }
        self.request_missing_bodies();
        changed
    }

    /// Re-read the text of every `<style>` already collected. A textContent
    /// write advances Dom::generation alone, so this costs one pass over the
    /// style elements rather than a walk of the tree.
    fn reread_inline_text(&mut self, dom: &silksurf_dom::Dom) -> bool {
        let mut changed = false;
        for source in &mut self.sources {
            let StyleSourceKind::Inline(text) = &mut source.kind else {
                continue;
            };
            let current = style_element_text(dom, source.owner);
            if current != *text {
                *text = current;
                changed = true;
            }
        }
        changed
    }

    /// Pull every fetch that finished since the last call.
    fn drain_completions(&mut self) -> bool {
        let mut landed = false;
        while let Ok(fetch) = self.completions.try_recv() {
            match fetch.body {
                Ok(css) => {
                    self.bodies.insert(fetch.url, css);
                    landed = true;
                }
                Err(message) => {
                    eprintln!("[SilkSurf] Stylesheet {}: {message}", fetch.url);
                    // An empty body keeps the URL out of the retry set; a
                    // failed sheet stays absent rather than refetching each
                    // tick.
                    self.bodies.insert(fetch.url, String::new());
                }
            }
        }
        landed
    }

    /// Spawn a fetch for every link source whose body is neither held nor
    /// already in flight.
    fn request_missing_bodies(&mut self) {
        let missing: Vec<String> = self
            .sources
            .iter()
            .filter_map(StyleSource::link_url)
            .filter(|url| !self.requested.contains(*url))
            .map(str::to_string)
            .collect();
        for url in missing {
            self.requested.insert(url.clone());
            self.spawn_body_fetch(url);
        }
    }

    fn spawn_body_fetch(&self, url: String) {
        let sender = self.sender.clone();
        let config = self.config.clone();
        eprintln!("[SilkSurf] Stylesheet {url}: scheduled");
        if let Err(err) = thread::Builder::new()
            .name("silksurf-stylesheet".to_string())
            .spawn(move || {
                let body = fetch_stylesheet_body(&config, &url);
                let _ = sender.send(StyleSheetFetch { url, body });
            })
        {
            eprintln!("[SilkSurf] Stylesheet thread: {err}");
        }
    }
}

/// Fetch one stylesheet body with a renderer of this thread's own.
fn fetch_stylesheet_body(config: &BrowserRenderConfig, url: &str) -> Result<String, String> {
    let mut renderer = renderer_from_config(config)?;
    let accept = [("Accept".to_string(), "text/css,*/*".to_string())];
    match renderer.fetch_or_speculate(url, &accept, None) {
        Ok((response, _, _)) if response.status == 200 => {
            Ok(String::from_utf8_lossy(&response.body).to_string())
        }
        Ok((response, _, _)) => Err(format!("HTTP {}", response.status)),
        Err(err) => Err(err.message),
    }
}

/*
 * fetch_style_source_bodies -- the link bodies for a source list, fetched in
 * one batch on the calling thread.
 *
 * The initial page build already holds a renderer and must produce a
 * paintable stylesheet before the first frame, so it fetches synchronously.
 * Runtime additions take the worker path in StyleSheetSet instead.
 */
pub(crate) fn fetch_style_source_bodies(
    renderer: &mut SpeculativeRenderer,
    sources: &[StyleSource],
    label: &str,
) -> Vec<(String, String)> {
    let urls = dedupe_resource_urls(
        &sources
            .iter()
            .filter_map(StyleSource::link_url)
            .map(str::to_string)
            .collect::<Vec<String>>(),
    );
    if urls.is_empty() {
        return Vec::new();
    }
    let accept = [("Accept".to_string(), "text/css,*/*".to_string())];
    let requests: Vec<(&str, &[(String, String)])> = urls
        .iter()
        .map(|url| (url.as_str(), accept.as_slice()))
        .collect();
    renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(urls.iter())
        .filter_map(|(result, url)| match result {
            Ok((response, origin, elapsed)) if response.status == 200 => {
                eprintln!(
                    "[SilkSurf] {label} {url}: {} bytes ({origin:?} {elapsed:?})",
                    response.body.len()
                );
                Some((
                    url.clone(),
                    String::from_utf8_lossy(&response.body).to_string(),
                ))
            }
            Ok((response, _, _)) => {
                eprintln!("[SilkSurf] {label} {url}: HTTP {}", response.status);
                None
            }
            Err(err) => {
                eprintln!("[SilkSurf] {label} {url}: fetch error: {}", err.message);
                None
            }
        })
        .collect()
}

/*
 * document_stylesheet_set -- collect, fetch, and assemble in one step.
 *
 * The static render path and the navigation builder both need the same three
 * operations in the same order, and both hold a renderer while doing it.
 */
pub(crate) fn document_stylesheet_set(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
    config: &BrowserRenderConfig,
    label: &str,
) -> StyleSheetSet {
    let sources = collect_style_sources(dom, root, base_url);
    let bodies = fetch_style_source_bodies(renderer, &sources, label);
    StyleSheetSet::new(sources, bodies, base_url, config, dom)
}

/// How long the event loop sleeps while a stylesheet fetch is outstanding.
/// A worker completion posts no wake, so this bounds the delay between a body
/// landing and the cascade rebuilding on it.
pub(crate) const STYLESHEET_FETCH_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(8);

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

    fn sources_of(html: &str) -> Vec<StyleSource> {
        let document = parse_html(html).expect("fixture parses");
        collect_style_sources(&document.dom, document.document, "https://example.com/")
    }

    fn set_of(html: &str, bodies: Vec<(String, String)>) -> StyleSheetSet {
        let document = parse_html(html).expect("fixture parses");
        let sources =
            collect_style_sources(&document.dom, document.document, "https://example.com/");
        StyleSheetSet::new(
            sources,
            bodies,
            "https://example.com/",
            &BrowserRenderConfig::default(),
            &document.dom,
        )
    }

    #[test]
    fn style_elements_and_stylesheet_links_collect_in_tree_order() {
        let sources = sources_of(
            "<!doctype html><html><head>\
             <style>a{color:red}</style>\
             <link rel=\"stylesheet\" href=\"/one.css\">\
             <style>b{color:blue}</style>\
             </head><body></body></html>",
        );
        assert_eq!(sources.len(), 3);
        assert!(matches!(sources[0].kind, StyleSourceKind::Inline(_)));
        assert_eq!(sources[1].link_url(), Some("https://example.com/one.css"));
        assert!(matches!(sources[2].kind, StyleSourceKind::Inline(_)));
    }

    #[test]
    fn a_preload_link_is_not_a_stylesheet_source() {
        let sources = sources_of(
            "<!doctype html><html><head>\
             <link rel=\"preload\" as=\"style\" href=\"/deferred.css\">\
             </head><body></body></html>",
        );
        assert!(sources.is_empty(), "collected {sources:?}");
    }

    #[test]
    fn a_rel_token_list_holding_stylesheet_collects() {
        let sources = sources_of(
            "<!doctype html><html><head>\
             <link rel=\"alternate stylesheet\" href=\"/alt.css\">\
             </head><body></body></html>",
        );
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].link_url(), Some("https://example.com/alt.css"));
    }

    #[test]
    fn the_media_attribute_wraps_the_source_in_an_at_media_block() {
        let set = set_of(
            "<!doctype html><html><head>\
             <style media=\"print\">a{color:red}</style>\
             </head><body></body></html>",
            Vec::new(),
        );
        let css = set.css_text();
        assert!(css.contains("@media print {"), "{css}");
        assert!(css.contains("a{color:red}"), "{css}");
    }

    #[test]
    fn media_all_contributes_no_wrapper() {
        let set = set_of(
            "<!doctype html><html><head>\
             <style media=\"all\">a{color:red}</style>\
             </head><body></body></html>",
            Vec::new(),
        );
        assert!(!set.css_text().contains("@media"));
    }

    #[test]
    fn a_link_without_a_body_contributes_nothing_yet() {
        let set = set_of(
            "<!doctype html><html><head>\
             <link rel=\"stylesheet\" href=\"/one.css\">\
             </head><body></body></html>",
            Vec::new(),
        );
        assert!(set.has_pending_fetches());
        assert_eq!(set.source_count(), 1);
        assert!(!set.css_text().contains("body{color:green}"));
    }

    #[test]
    fn a_seeded_body_lands_in_the_concatenated_text() {
        let set = set_of(
            "<!doctype html><html><head>\
             <link rel=\"stylesheet\" href=\"/one.css\">\
             </head><body></body></html>",
            vec![(
                "https://example.com/one.css".to_string(),
                "body{color:green}".to_string(),
            )],
        );
        assert!(!set.has_pending_fetches());
        assert!(set.css_text().contains("body{color:green}"));
    }

    #[test]
    fn the_user_agent_sheet_leads_the_author_text() {
        let set = set_of(
            "<!doctype html><html><head><style>a{color:red}</style></head></html>",
            Vec::new(),
        );
        let css = set.css_text();
        let ua = css
            .find("html, body { display: block; }")
            .expect("ua sheet");
        let author = css.find("a{color:red}").expect("author sheet");
        assert!(ua < author, "user-agent defaults follow the author sheet");
    }

    #[test]
    fn a_rel_swapped_to_stylesheet_changes_the_source_list() {
        let mut document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"preload\" as=\"style\" href=\"/one.css\">\
             </head><body></body></html>",
        )
        .expect("fixture parses");
        let link = find_element_by_tag(&document.dom, document.document, "link")
            .expect("fixture holds a link");
        let mut set = StyleSheetSet::new(
            collect_style_sources(&document.dom, document.document, "https://example.com/"),
            Vec::new(),
            "https://example.com/",
            &BrowserRenderConfig::default(),
            &document.dom,
        );
        assert_eq!(set.source_count(), 0);

        document
            .dom
            .set_attribute(link, "rel", "stylesheet")
            .expect("rel rewrites");
        assert!(set.refresh(&document.dom, document.document, &[link]));
        assert_eq!(set.source_count(), 1);
    }

    #[test]
    fn an_unchanged_document_reports_no_refresh() {
        let document =
            parse_html("<!doctype html><html><head><style>a{color:red}</style></head></html>")
                .expect("fixture parses");
        let mut set = StyleSheetSet::new(
            collect_style_sources(&document.dom, document.document, "https://example.com/"),
            Vec::new(),
            "https://example.com/",
            &BrowserRenderConfig::default(),
            &document.dom,
        );
        assert!(!set.refresh(&document.dom, document.document, &[]));
    }

    #[test]
    fn a_style_element_appended_later_joins_the_source_list() {
        let mut document =
            parse_html("<!doctype html><html><head></head><body></body></html>").expect("parses");
        let head =
            find_element_by_tag(&document.dom, document.document, "head").expect("head exists");
        let mut set = StyleSheetSet::new(
            collect_style_sources(&document.dom, document.document, "https://example.com/"),
            Vec::new(),
            "https://example.com/",
            &BrowserRenderConfig::default(),
            &document.dom,
        );
        assert_eq!(set.source_count(), 0);

        let style = document.dom.create_element("style");
        let text = document.dom.create_text("p{color:teal}");
        document
            .dom
            .append_child(style, text)
            .expect("text attaches");
        document
            .dom
            .append_child(head, style)
            .expect("style attaches");
        assert!(set.refresh(&document.dom, document.document, &[]));
        assert!(set.css_text().contains("p{color:teal}"));
    }

    #[test]
    fn rewriting_a_style_element_text_rebuilds_the_cascade_input() {
        let mut document =
            parse_html("<!doctype html><html><head><style>a{color:red}</style></head></html>")
                .expect("parses");
        let style =
            find_element_by_tag(&document.dom, document.document, "style").expect("style exists");
        let mut set = StyleSheetSet::new(
            collect_style_sources(&document.dom, document.document, "https://example.com/"),
            Vec::new(),
            "https://example.com/",
            &BrowserRenderConfig::default(),
            &document.dom,
        );
        assert!(set.css_text().contains("a{color:red}"));

        document
            .dom
            .set_text_content(style, "a{color:lime}")
            .expect("text rewrites");
        assert!(set.refresh(&document.dom, document.document, &[style]));
        let css = set.css_text();
        assert!(css.contains("a{color:lime}"), "{css}");
        assert!(!css.contains("a{color:red}"), "{css}");
    }

    fn find_element_by_tag(
        dom: &silksurf_dom::Dom,
        node: silksurf_dom::NodeId,
        tag: &str,
    ) -> Option<silksurf_dom::NodeId> {
        if dom.element_name(node).ok().flatten() == Some(tag) {
            return Some(node);
        }
        for &child in dom.children(node).ok()? {
            if let Some(found) = find_element_by_tag(dom, child, tag) {
                return Some(found);
            }
        }
        None
    }
}
