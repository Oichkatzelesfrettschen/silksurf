// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) const FRAME_WIDTH: u32 = 1280;
pub(crate) const FRAME_HEIGHT: u32 = 800;
pub(crate) const MIN_INITIAL_WINDOW_HEIGHT: u32 = 320;
pub(crate) const BROWSER_CHROME_HEIGHT: f32 = 44.0;
pub(crate) const FOCUS_VIEWPORT_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(1);
pub(crate) const CURRENT_VIEW_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(2);
pub(crate) const NAVIGATION_START_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(3);
pub(crate) const SCROLL_VIEWPORT_RETAINED_TAG_BASE: u64 = 10_000;
pub(crate) const BROWSER_WHEEL_LINE_PX: f32 = 48.0;
pub(crate) const BROWSER_PAGE_SCROLL_FACTOR: f32 = 0.875;
pub(crate) const HOME_URL: &str = "https://example.com";
pub(crate) const NAV_BUTTON_Y: u32 = 8;
pub(crate) const NAV_BUTTON_WIDTH: u32 = 14;
pub(crate) const NAV_BUTTON_HEIGHT: u32 = 28;
pub(crate) const BACK_BUTTON_X: u32 = 8;
pub(crate) const FORWARD_BUTTON_X: u32 = 28;
pub(crate) const HOME_BUTTON_X: u32 = 48;
pub(crate) const RELOAD_BUTTON_X: u32 = 68;
pub(crate) const STOP_BUTTON_X: u32 = 88;
pub(crate) const ADDRESS_BAR_X: u32 = 108;
pub(crate) const ADDRESS_BAR_Y: u32 = 8;
pub(crate) const ADDRESS_BAR_WIDTH: u32 = 880;
pub(crate) const ADDRESS_BAR_HEIGHT: u32 = 28;
pub(crate) const ADDRESS_TEXT_MAX_CHARS: usize = 2048;
pub(crate) const PAGE_INPUT_TEXT_MAX_CHARS: usize = 4096;
pub(crate) const DOCUMENT_TILE_SIZE: u32 = 128;
// 8 MiB accommodates real-world and benchmark bundles (JetStream/Octane
// payloads run 2-5 MiB); the cap exists to bound memory on hostile pages,
// and SILKSURF_MAX_SCRIPT_BYTES overrides it for experiments.
pub(crate) const DEFAULT_MAX_NAVIGATION_SCRIPT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn max_navigation_script_bytes() -> usize {
    static LIMIT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("SILKSURF_MAX_SCRIPT_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_MAX_NAVIGATION_SCRIPT_BYTES)
    })
}
pub(crate) const MAX_NAVIGATION_MODULE_ROOTS: usize = 4;
pub(crate) const MAX_NAVIGATION_MODULE_GRAPH_BYTES: usize = 512 * 1024;
pub(crate) const MAX_DYNAMIC_SCRIPT_ROUNDS: usize = 8;
pub(crate) const MAX_MODULE_GRAPH_URLS: usize = 64;
pub(crate) const MAX_MODULE_GRAPH_ROUNDS: usize = 8;
pub(crate) const MAX_MODULE_GRAPH_SCAN_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS: usize = 8;
pub(crate) const IMAGE_CACHE_ESTIMATED_ITEMS: usize = 128;
pub(crate) const IMAGE_CACHE_CAPACITY_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_ROOT_ID: u64 = 1;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_ADDRESS_ID: u64 = 2;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_STATUS_ID: u64 = 3;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_BACK_ID: u64 = 10;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_FORWARD_ID: u64 = 11;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_HOME_ID: u64 = 12;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_RELOAD_ID: u64 = 13;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_STOP_ID: u64 = 14;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_LINK_BASE_ID: u64 = 10_000;
#[cfg(feature = "accessibility")]
pub(crate) const ACCESSIBILITY_INPUT_BASE_ID: u64 = 20_000;
pub(crate) const DEFAULT_USER_AGENT_STYLESHEET: &str = "
html, body { display: block; }
head, title, meta, link, style, script { display: none; }
body {
  margin: 8px;
  color: black;
  background-color: white;
  font-size: 16px;
  line-height: 19px;
}
h1 { display: block; font-size: 32px; line-height: 38px; margin: 21px 0; font-weight: bold; }
h2 { display: block; font-size: 24px; line-height: 29px; margin: 20px 0; font-weight: bold; }
h3 { display: block; font-size: 19px; line-height: 23px; margin: 19px 0; font-weight: bold; }
p, div, section, article, main, header, footer, nav { display: block; }
p { margin: 16px 0; }
ul, ol { display: block; margin: 16px 0; padding-left: 40px; }
li { display: block; }
a { color: #0645ad; text-decoration: underline; }
img { display: block; }
button, input, textarea, select { font-size: 13px; line-height: 16px; border: 1px solid #767676; padding: 2px; }
";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkTarget {
    pub(crate) rect: Rect,
    pub(crate) href: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InputTarget {
    pub(crate) rect: Rect,
    pub(crate) node: silksurf_dom::NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormSubmissionTarget {
    Get(String),
    Post(BrowserNavigationRequest),
    UnsupportedMethod(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNavigationRequest {
    pub(crate) method: HttpMethod,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// The site (scheme + registrable domain) of the page that initiated this
    /// navigation, or `None` when the browser initiated it (address bar,
    /// bookmark, history, the initial load). It drives top-level-navigation
    /// SameSite enforcement: a cross-site initiator withholds `Strict` cookies
    /// (and `Lax` for unsafe methods). `None` is same-site, so `Strict` rides.
    /// It is deliberately not part of the HTTP request -- it never becomes a
    /// header -- so `as_http_request` ignores it.
    pub(crate) initiator_site: Option<String>,
}

impl BrowserNavigationRequest {
    pub(crate) fn get(url: String) -> Self {
        Self {
            method: HttpMethod::Get,
            url,
            headers: Vec::new(),
            body: Vec::new(),
            initiator_site: None,
        }
    }

    pub(crate) fn post_form(url: String, body: Vec<u8>) -> Self {
        Self {
            method: HttpMethod::Post,
            url,
            headers: vec![(
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            )],
            body,
            initiator_site: None,
        }
    }

    /// Mark this navigation as initiated by the page at `initiator_url` (a link
    /// click or form submission). The initiator's site keys SameSite
    /// enforcement. A URL that does not parse leaves the initiator unset
    /// (browser-initiated / same-site fallback).
    pub(crate) fn initiated_by(mut self, initiator_url: &str) -> Self {
        self.initiator_site = url::Url::parse(initiator_url)
            .as_ref()
            .map(silksurf_net::cookie::site_of_url)
            .ok();
        self
    }

    pub(crate) fn as_http_request(&self) -> HttpRequest {
        HttpRequest {
            method: self.method,
            url: self.url.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct BrowserRenderConfig {
    pub(crate) insecure: bool,
    pub(crate) platform_verifier: bool,
    pub(crate) tls_ca_file: Option<std::path::PathBuf>,
    /// One partitioned cookie jar per browsing session, shared by the HTTP
    /// client (via the renderer) and the JS `document.cookie` bridge so cookies
    /// round-trip. The `Arc` is cheap to clone; every clone of this config
    /// shares the jar.
    pub(crate) cookie_jar:
        std::sync::Arc<std::sync::Mutex<silksurf_net::cookie::PartitionedCookieStore>>,
    /// The top-level document's site for the current navigation (scheme://host),
    /// set when the navigation URL is known. It keys the cookie partition and
    /// drives SameSite enforcement. Empty means "unknown": cookies fall back to
    /// the unpartitioned store with no enforcement.
    pub(crate) top_level_site: String,
}

pub(crate) struct BrowserFrame {
    pub(crate) url: String,
    pub(crate) argb: Vec<u32>,
    pub(crate) raster_height: u32,
    pub(crate) bitmap_height: u32,
    pub(crate) bitmap_scroll_y: u32,
    pub(crate) focus_viewport_cache: Option<FocusViewportCache>,
    pub(crate) focus_viewport_retained_sent: bool,
    pub(crate) current_view_retained_sent: bool,
    pub(crate) navigation_start_retained_sent: bool,
    pub(crate) scroll_viewport_caches: Vec<ScrollViewportCache>,
    pub(crate) link_targets: Vec<LinkTarget>,
    pub(crate) input_targets: Vec<InputTarget>,
}

pub(crate) struct FocusViewportCache {
    pub(crate) scroll_y: u32,
    pub(crate) bitmap_height: u32,
    pub(crate) argb: Vec<u32>,
}

pub(crate) struct ScrollViewportCache {
    pub(crate) scroll_y: u32,
    pub(crate) bitmap_height: u32,
    pub(crate) tag: silksurf_gui::WinitRetainedBufferTag,
    pub(crate) argb: Vec<u32>,
    pub(crate) retained_sent: bool,
}

pub(crate) struct BrowserPagePayload {
    pub(crate) url: String,
    pub(crate) html: String,
    pub(crate) css_text: String,
    pub(crate) script_texts: Vec<String>,
    pub(crate) module_texts: Vec<(String, String)>,
    pub(crate) images: Vec<DecodedPageImage>,
    pub(crate) render_config: BrowserRenderConfig,
    pub(crate) parsed_document: Option<ParsedDocument>,
}

#[derive(Debug, Default)]
pub(crate) struct BrowserFrameBuffers {
    pub(crate) rgba: Vec<u8>,
    pub(crate) argb: Vec<u32>,
}

#[derive(Debug)]
pub(crate) struct BrowserPageBuildError {
    pub(crate) message: String,
    pub(crate) buffers: BrowserFrameBuffers,
}

#[derive(Clone)]
pub(crate) struct DecodedPageImage {
    pub(crate) url: String,
    pub(crate) surface: silksurf_render::ImageSurface,
}

#[derive(Clone)]
pub(crate) struct DecodedImageWeighter;

impl Weighter<String, DecodedPageImage> for DecodedImageWeighter {
    fn weight(&self, key: &String, value: &DecodedPageImage) -> u64 {
        (key.len() + value.surface.rgba.len()) as u64
    }
}

pub(crate) struct ImageResourceCache {
    pub(crate) decoded: Cache<String, DecodedPageImage, DecodedImageWeighter>,
}

impl ImageResourceCache {
    pub(crate) fn new() -> Self {
        Self::with_capacity(IMAGE_CACHE_ESTIMATED_ITEMS, IMAGE_CACHE_CAPACITY_BYTES)
    }

    pub(crate) fn with_capacity(estimated_items: usize, capacity_bytes: u64) -> Self {
        Self {
            decoded: Cache::with_weighter(estimated_items, capacity_bytes, DecodedImageWeighter),
        }
    }

    pub(crate) fn get(&self, url: &str) -> Option<DecodedPageImage> {
        self.decoded.get(url).cloned()
    }

    pub(crate) fn insert(&mut self, image: DecodedPageImage) {
        self.decoded.insert(image.url.clone(), image);
    }

    pub(crate) fn len(&self) -> usize {
        self.decoded.len()
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.decoded.weight()
    }
}

pub(crate) struct BrowserPage {
    pub(crate) frame: BrowserFrame,
    pub(crate) runtime: BrowserPageRuntime,
}

pub(crate) struct BrowserPageRuntime {
    pub(crate) dom: Arc<Mutex<silksurf_dom::Dom>>,
    pub(crate) document: silksurf_dom::NodeId,
    pub(crate) stylesheet: silksurf_css::Stylesheet,
    pub(crate) style_index: StyleIndex,
    pub(crate) viewport: Rect,
    pub(crate) js_ctx: SilkContext,
    pub(crate) fused: FusedResult,
    pub(crate) fused_workspace: FusedWorkspace,
    pub(crate) display_list: silksurf_render::DisplayList,
    pub(crate) images: Vec<DecodedPageImage>,
    pub(crate) rgba: Vec<u8>,
    pub(crate) damage_scratch: silksurf_render::DamageScratch,
    pub(crate) viewport_item_indices: Vec<usize>,
}

pub(crate) struct BrowserState {
    pub(crate) frame: BrowserFrame,
    pub(crate) runtime: Option<BrowserPageRuntime>,
    pub(crate) navigation_pending: bool,
    pub(crate) status_text: String,
    pub(crate) hover_status_text: Option<String>,
    pub(crate) history: Vec<String>,
    pub(crate) history_index: usize,
    pub(crate) pending_history: Option<PendingHistoryAction>,
    pub(crate) navigation_generation: u64,
    pub(crate) address_editing: bool,
    pub(crate) address_select_all: bool,
    pub(crate) address_text: String,
    pub(crate) address_cursor: usize,
    pub(crate) focused_input: Option<silksurf_dom::NodeId>,
    pub(crate) redraw_mode: BrowserRedrawMode,
    pub(crate) retained_present: Option<BrowserRetainedPresent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BrowserRetainedPresent {
    pub(crate) tag: silksurf_gui::WinitRetainedBufferTag,
    pub(crate) damage: silksurf_gui::WinitPresentDamage,
}

pub(crate) struct BrowserInputRuntime<'a> {
    pub(crate) state: &'a Rc<RefCell<BrowserState>>,
    pub(crate) navigation_rx: &'a Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    pub(crate) scroll: &'a Cell<f32>,
    pub(crate) chrome_height: u32,
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
    pub(crate) wake_handle: &'a silksurf_gui::WinitWakeHandle,
    pub(crate) render_config: &'a BrowserRenderConfig,
    pub(crate) image_cache: &'a Arc<Mutex<ImageResourceCache>>,
}

#[derive(Clone, Copy)]
pub(crate) struct TextItemPaint {
    pub(crate) rect: Rect,
    pub(crate) font_size: f32,
    pub(crate) color: silksurf_css::Color,
}

#[derive(Clone, Copy)]
pub(crate) struct PixelRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct AppOptions {
    pub(crate) speculative: bool,
    pub(crate) window_mode: bool,
    pub(crate) headless: bool,
    pub(crate) display_backend: silksurf_gui::WinitDisplayBackend,
    pub(crate) url: String,
    pub(crate) render_config: BrowserRenderConfig,
}

pub(crate) type NavigationResult = Result<BrowserPagePayload, String>;
pub(crate) type NavigationMessage = (u64, NavigationResult);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingHistoryAction {
    Push,
    MoveTo(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BrowserRedrawMode {
    Clean,
    Full,
    Scroll,
    Damage(Rect),
    PageInputFocus(Rect),
    DamageWithChrome(Rect),
    AddressFocusChrome,
    AddressFullTextChrome,
    AddressChrome,
    AddressTextChrome,
    NavigationStartChrome,
    StatusChrome,
    Chrome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AddressCaretMotion {
    Backward,
    Forward,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BrowserBitmapRefresh {
    Clean,
    Full,
    ScrollReuse(Rect),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowserChromeAction {
    Back,
    Forward,
    Home,
    Reload,
    Stop,
}
