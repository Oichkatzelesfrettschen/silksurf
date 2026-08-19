/*
 * link_preload fetches `<link rel=preload>` resources and fires the load event
 * the page waits on.
 *
 * HTML defines a preload link as a fetch the document starts early and reports
 * through the element's `load` event. Pages build on exactly that: chatgpt.com
 * ships its stylesheet as `<link rel=preload as=style>` and upgrades the rel to
 * `stylesheet` from the load handler, so a browser that never fetches the
 * preload never enters the sheet into the cascade.
 *
 * The fetch runs on a worker thread and its completion arrives over an mpsc
 * channel; the repaint tick dispatches `load` or `error` at the owning element
 * through SilkContext::dispatch_dom_event, which runs the handler with full
 * propagation and drains microtasks. The response body is discarded -- the
 * warmed HTTP cache is what the follow-on stylesheet or script fetch reads.
 */

// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

use silksurf_js::SyntheticEvent;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Upper bound on preload links a document may start. A document listing more
/// bounds its fetch fan-out here rather than in the network layer.
pub(crate) const MAX_PRELOAD_LINKS: usize = 32;

/// One `<link rel=preload>` and the URL it fetches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreloadLink {
    pub(crate) owner: silksurf_dom::NodeId,
    pub(crate) url: String,
}

/// The document's preload links in tree order.
pub(crate) fn collect_preload_links(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<PreloadLink> {
    let mut links = Vec::new();
    collect_preload_links_from(dom, root, base_url, &mut links);
    links
}

fn collect_preload_links_from(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    links: &mut Vec<PreloadLink>,
) {
    if links.len() >= MAX_PRELOAD_LINKS {
        return;
    }
    if let Some(url) = link_resource_url_for_node(dom, node, base_url, "preload") {
        links.push(PreloadLink { owner: node, url });
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_preload_links_from(dom, child, base_url, links);
        }
    }
}

/// The `as` attribute, which selects the Accept header the fetch sends.
pub(crate) fn preload_accept_header(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> &'static str {
    let as_value = dom
        .attributes(node)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("as"))
                .map(|attr| attr.value.as_str().to_ascii_lowercase())
        })
        .unwrap_or_default();
    match as_value.as_str() {
        "style" => "text/css,*/*",
        "script" => "text/javascript,application/javascript,*/*",
        "font" => "font/woff2,font/woff,*/*",
        "image" => "image/webp,image/png,image/*,*/*",
        _ => "*/*",
    }
}

/*
 * PreloadLinks -- the document's preload fetches and their pending events.
 *
 * `refresh` gates on Dom::style_generation, so a document whose links are
 * unchanged costs one integer comparison. A URL started once is never started
 * again, which is what keeps a re-collection from refetching.
 */
pub(crate) struct PreloadLinks {
    started: HashSet<silksurf_dom::NodeId>,
    /// Fetches spawned but not yet drained. The event loop polls while this is
    /// non-zero, because a worker completion posts no wake of its own.
    outstanding: usize,
    completions: Receiver<PreloadCompletion>,
    sender: Sender<PreloadCompletion>,
    style_generation: u64,
    base_url: String,
    config: BrowserRenderConfig,
}

struct PreloadCompletion {
    owner: silksurf_dom::NodeId,
    loaded: bool,
}

impl PreloadLinks {
    pub(crate) fn new(base_url: &str, config: &BrowserRenderConfig) -> Self {
        let (sender, completions) = channel();
        Self {
            started: HashSet::new(),
            outstanding: 0,
            completions,
            sender,
            // A fresh set has collected nothing, so the first refresh always
            // walks the tree whatever the document's generation is.
            style_generation: u64::MAX,
            base_url: base_url.to_string(),
            config: config.clone(),
        }
    }

    /// Whether a fetch is still in flight, which keeps the event loop polling.
    pub(crate) fn has_pending_fetches(&self) -> bool {
        self.outstanding > 0
    }

    /// Start the fetch for every preload link not started yet.
    pub(crate) fn refresh(&mut self, dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) {
        if dom.style_generation() == self.style_generation {
            return;
        }
        self.style_generation = dom.style_generation();
        for link in collect_preload_links(dom, root, &self.base_url) {
            if !self.started.insert(link.owner) {
                continue;
            }
            let accept = preload_accept_header(dom, link.owner);
            self.spawn_fetch(link, accept);
            self.outstanding += 1;
        }
    }

    fn spawn_fetch(&mut self, link: PreloadLink, accept: &'static str) {
        let sender = self.sender.clone();
        let config = self.config.clone();
        eprintln!("[SilkSurf] Preload {}: scheduled", link.url);
        if let Err(err) = thread::Builder::new()
            .name("silksurf-preload".to_string())
            .spawn(move || {
                let loaded = preload_fetch_succeeds(&config, &link.url, accept);
                let _ = sender.send(PreloadCompletion {
                    owner: link.owner,
                    loaded,
                });
            })
        {
            eprintln!("[SilkSurf] Preload thread: {err}");
        }
    }

    /*
     * dispatch_completed -- fire load or error at each finished preload link.
     *
     * Returns the number of events dispatched. A handler runs page script, so
     * the caller treats a non-zero count as a reason to re-examine the DOM.
     */
    pub(crate) fn dispatch_completed(&mut self, js_ctx: &mut SilkContext) -> usize {
        let mut dispatched = 0;
        while let Ok(completion) = self.completions.try_recv() {
            self.outstanding = self.outstanding.saturating_sub(1);
            let event_type = if completion.loaded { "load" } else { "error" };
            let event = SyntheticEvent {
                event_type: event_type.to_string(),
                bubbles: false,
                cancelable: false,
                fields: Vec::new(),
            };
            match js_ctx.dispatch_dom_event(completion.owner, &event) {
                Ok(_) => dispatched += 1,
                Err(message) => eprintln!("[SilkSurf] Preload {event_type}: {message}"),
            }
        }
        dispatched
    }
}

/// Fetch a preload URL and report whether the origin answered 200. The body is
/// discarded; the warmed cache is what the follow-on fetch reads.
fn preload_fetch_succeeds(config: &BrowserRenderConfig, url: &str, accept: &str) -> bool {
    let Ok(mut renderer) = renderer_from_config(config) else {
        return false;
    };
    let headers = [("Accept".to_string(), accept.to_string())];
    match renderer.fetch_or_speculate(url, &headers, None) {
        Ok((response, _, elapsed)) => {
            eprintln!(
                "[SilkSurf] Preload {url}: HTTP {} ({} bytes, {elapsed:?})",
                response.status,
                response.body.len()
            );
            response.status == 200
        }
        Err(err) => {
            eprintln!("[SilkSurf] Preload {url}: fetch error: {}", err.message);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

    fn links_of(html: &str) -> Vec<PreloadLink> {
        let document = parse_html(html).expect("fixture parses");
        collect_preload_links(&document.dom, document.document, "https://example.com/")
    }

    #[test]
    fn a_preload_link_collects_with_its_resolved_url() {
        let links = links_of(
            "<!doctype html><html><head>\
             <link rel=\"preload\" as=\"style\" href=\"/late.css\">\
             </head><body></body></html>",
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/late.css");
    }

    #[test]
    fn a_stylesheet_link_is_not_a_preload() {
        let links = links_of(
            "<!doctype html><html><head>\
             <link rel=\"stylesheet\" href=\"/now.css\">\
             </head><body></body></html>",
        );
        assert!(links.is_empty(), "collected {links:?}");
    }

    #[test]
    fn preload_links_collect_in_tree_order() {
        let links = links_of(
            "<!doctype html><html><head>\
             <link rel=\"preload\" as=\"script\" href=\"/one.js\">\
             <link rel=\"preload\" as=\"font\" href=\"/two.woff2\">\
             </head><body></body></html>",
        );
        assert_eq!(
            links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/one.js",
                "https://example.com/two.woff2"
            ]
        );
    }

    #[test]
    fn a_fresh_set_reports_no_outstanding_fetch() {
        let links = PreloadLinks::new("https://example.com/", &BrowserRenderConfig::default());
        assert!(!links.has_pending_fetches());
    }

    #[test]
    fn the_as_attribute_selects_the_accept_header() {
        let document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"preload\" as=\"style\" href=\"/a.css\">\
             <link rel=\"preload\" as=\"script\" href=\"/b.js\">\
             <link rel=\"preload\" href=\"/c.bin\">\
             </head><body></body></html>",
        )
        .expect("fixture parses");
        let links = collect_preload_links(&document.dom, document.document, "https://example.com/");
        assert_eq!(links.len(), 3);
        assert_eq!(
            preload_accept_header(&document.dom, links[0].owner),
            "text/css,*/*"
        );
        assert_eq!(
            preload_accept_header(&document.dom, links[1].owner),
            "text/javascript,application/javascript,*/*"
        );
        assert_eq!(preload_accept_header(&document.dom, links[2].owner), "*/*");
    }
}
