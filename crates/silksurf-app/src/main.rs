//! `SilkSurf` Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render.
//!
//! Usage: silksurf-app \[URL\]
//! Default URL: `https://example.com`

#![allow(
    clippy::assigning_clones,
    clippy::cast_ptr_alignment,
    clippy::derivable_impls,
    clippy::float_cmp,
    clippy::if_not_else,
    clippy::manual_let_else,
    clippy::map_unwrap_or,
    clippy::needless_borrow,
    clippy::needless_option_as_deref,
    clippy::needless_pass_by_value,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::unnecessary_wraps,
    clippy::unnested_or_patterns,
    clippy::unreadable_literal
)]

/*
 * mimalloc global allocator.
 *
 * The CSS tokenizer and cascade produce many small heap allocations. mimalloc
 * uses thread-local free lists and page segregation, so the allocation-heavy
 * CSS path runs through a low-latency allocator without changing call sites.
 */
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use quick_cache::{Weighter, unsync::Cache};
use silksurf_css::StyleIndex;
use silksurf_dom::diff::{ChangeKind, DomDiff};
use silksurf_engine::fused_pipeline::{
    FusedResult, FusedWorkspace, ReplacedSize, fused_style_layout_paint,
    fused_style_layout_paint_with_replaced_sizes,
};
use silksurf_engine::parse_html;
use silksurf_engine::speculative::{FetchOrigin, SpeculativeRenderer};
use silksurf_js::SilkContext;
use silksurf_layout::Rect;
use silksurf_net::{HttpMethod, HttpRequest};

const FRAME_WIDTH: u32 = 1280;
const FRAME_HEIGHT: u32 = 800;
const MIN_INITIAL_WINDOW_HEIGHT: u32 = 320;
const BROWSER_CHROME_HEIGHT: f32 = 44.0;
const FOCUS_VIEWPORT_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(1);
const CURRENT_VIEW_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(2);
const NAVIGATION_START_RETAINED_TAG: silksurf_gui::WinitRetainedBufferTag =
    silksurf_gui::WinitRetainedBufferTag::new(3);
const SCROLL_VIEWPORT_RETAINED_TAG_BASE: u64 = 10_000;
const BROWSER_WHEEL_LINE_PX: f32 = 48.0;
const BROWSER_PAGE_SCROLL_FACTOR: f32 = 0.875;
const HOME_URL: &str = "https://example.com";
const NAV_BUTTON_Y: u32 = 8;
const NAV_BUTTON_WIDTH: u32 = 14;
const NAV_BUTTON_HEIGHT: u32 = 28;
const BACK_BUTTON_X: u32 = 8;
const FORWARD_BUTTON_X: u32 = 28;
const HOME_BUTTON_X: u32 = 48;
const RELOAD_BUTTON_X: u32 = 68;
const STOP_BUTTON_X: u32 = 88;
const ADDRESS_BAR_X: u32 = 108;
const ADDRESS_BAR_Y: u32 = 8;
const ADDRESS_BAR_WIDTH: u32 = 880;
const ADDRESS_BAR_HEIGHT: u32 = 28;
const ADDRESS_TEXT_MAX_CHARS: usize = 2048;
const PAGE_INPUT_TEXT_MAX_CHARS: usize = 4096;
const DOCUMENT_TILE_SIZE: u32 = 64;
const MAX_NAVIGATION_SCRIPT_BYTES: usize = 256 * 1024;
const MAX_NAVIGATION_MODULE_ROOTS: usize = 4;
const MAX_NAVIGATION_MODULE_GRAPH_BYTES: usize = 512 * 1024;
const MAX_DYNAMIC_SCRIPT_ROUNDS: usize = 8;
const MAX_MODULE_GRAPH_URLS: usize = 64;
const MAX_MODULE_GRAPH_ROUNDS: usize = 8;
const MAX_MODULE_GRAPH_SCAN_BYTES: usize = 2 * 1024 * 1024;
const MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS: usize = 8;
const IMAGE_CACHE_ESTIMATED_ITEMS: usize = 128;
const IMAGE_CACHE_CAPACITY_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_ROOT_ID: u64 = 1;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_ADDRESS_ID: u64 = 2;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_STATUS_ID: u64 = 3;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_BACK_ID: u64 = 10;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_FORWARD_ID: u64 = 11;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_HOME_ID: u64 = 12;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_RELOAD_ID: u64 = 13;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_STOP_ID: u64 = 14;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_LINK_BASE_ID: u64 = 10_000;
#[cfg(feature = "accessibility")]
const ACCESSIBILITY_INPUT_BASE_ID: u64 = 20_000;
const DEFAULT_USER_AGENT_STYLESHEET: &str = "
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
struct LinkTarget {
    rect: Rect,
    href: String,
}

#[derive(Debug, Clone, PartialEq)]
struct InputTarget {
    rect: Rect,
    node: silksurf_dom::NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FormSubmissionTarget {
    Get(String),
    Post(BrowserNavigationRequest),
    UnsupportedMethod(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserNavigationRequest {
    method: HttpMethod,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl BrowserNavigationRequest {
    fn get(url: String) -> Self {
        Self {
            method: HttpMethod::Get,
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn post_form(url: String, body: Vec<u8>) -> Self {
        Self {
            method: HttpMethod::Post,
            url,
            headers: vec![(
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            )],
            body,
        }
    }

    fn as_http_request(&self) -> HttpRequest {
        HttpRequest {
            method: self.method,
            url: self.url.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }
}

#[derive(Clone)]
struct BrowserRenderConfig {
    insecure: bool,
    platform_verifier: bool,
    tls_ca_file: Option<std::path::PathBuf>,
}

impl Default for BrowserRenderConfig {
    fn default() -> Self {
        Self {
            insecure: false,
            platform_verifier: false,
            tls_ca_file: None,
        }
    }
}

struct BrowserFrame {
    url: String,
    argb: Vec<u32>,
    raster_height: u32,
    bitmap_height: u32,
    bitmap_scroll_y: u32,
    focus_viewport_cache: Option<FocusViewportCache>,
    focus_viewport_retained_sent: bool,
    current_view_retained_sent: bool,
    navigation_start_retained_sent: bool,
    scroll_viewport_caches: Vec<ScrollViewportCache>,
    link_targets: Vec<LinkTarget>,
    input_targets: Vec<InputTarget>,
}

struct FocusViewportCache {
    scroll_y: u32,
    bitmap_height: u32,
    argb: Vec<u32>,
}

struct ScrollViewportCache {
    scroll_y: u32,
    bitmap_height: u32,
    tag: silksurf_gui::WinitRetainedBufferTag,
    argb: Vec<u32>,
    retained_sent: bool,
}

struct BrowserPagePayload {
    url: String,
    html: String,
    css_text: String,
    script_texts: Vec<String>,
    module_texts: Vec<(String, String)>,
    images: Vec<DecodedPageImage>,
    render_config: BrowserRenderConfig,
}

#[derive(Debug, Default)]
struct BrowserFrameBuffers {
    rgba: Vec<u8>,
    argb: Vec<u32>,
}

#[derive(Debug)]
struct BrowserPageBuildError {
    message: String,
    buffers: BrowserFrameBuffers,
}

#[derive(Clone)]
struct DecodedPageImage {
    url: String,
    surface: silksurf_render::ImageSurface,
}

#[derive(Clone)]
struct DecodedImageWeighter;

impl Weighter<String, DecodedPageImage> for DecodedImageWeighter {
    fn weight(&self, key: &String, value: &DecodedPageImage) -> u64 {
        (key.len() + value.surface.rgba.len()) as u64
    }
}

struct ImageResourceCache {
    decoded: Cache<String, DecodedPageImage, DecodedImageWeighter>,
}

impl ImageResourceCache {
    fn new() -> Self {
        Self::with_capacity(IMAGE_CACHE_ESTIMATED_ITEMS, IMAGE_CACHE_CAPACITY_BYTES)
    }

    fn with_capacity(estimated_items: usize, capacity_bytes: u64) -> Self {
        Self {
            decoded: Cache::with_weighter(estimated_items, capacity_bytes, DecodedImageWeighter),
        }
    }

    fn get(&self, url: &str) -> Option<DecodedPageImage> {
        self.decoded.get(url).cloned()
    }

    fn insert(&mut self, image: DecodedPageImage) {
        self.decoded.insert(image.url.clone(), image);
    }

    fn len(&self) -> usize {
        self.decoded.len()
    }

    fn bytes(&self) -> u64 {
        self.decoded.weight()
    }
}

struct BrowserPage {
    frame: BrowserFrame,
    runtime: BrowserPageRuntime,
}

struct BrowserPageRuntime {
    dom: Arc<Mutex<silksurf_dom::Dom>>,
    document: silksurf_dom::NodeId,
    stylesheet: silksurf_css::Stylesheet,
    style_index: StyleIndex,
    viewport: Rect,
    js_ctx: SilkContext,
    fused: FusedResult,
    fused_workspace: FusedWorkspace,
    display_list: silksurf_render::DisplayList,
    images: Vec<DecodedPageImage>,
    rgba: Vec<u8>,
    damage_scratch: silksurf_render::DamageScratch,
}

struct BrowserState {
    frame: BrowserFrame,
    runtime: Option<BrowserPageRuntime>,
    navigation_pending: bool,
    status_text: String,
    hover_status_text: Option<String>,
    history: Vec<String>,
    history_index: usize,
    pending_history: Option<PendingHistoryAction>,
    navigation_generation: u64,
    address_editing: bool,
    address_select_all: bool,
    address_text: String,
    address_cursor: usize,
    focused_input: Option<silksurf_dom::NodeId>,
    redraw_mode: BrowserRedrawMode,
    retained_present: Option<BrowserRetainedPresent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BrowserRetainedPresent {
    tag: silksurf_gui::WinitRetainedBufferTag,
    damage: silksurf_gui::WinitPresentDamage,
}

struct BrowserInputRuntime<'a> {
    state: &'a Rc<RefCell<BrowserState>>,
    navigation_rx: &'a Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    scroll: &'a Cell<f32>,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
    wake_handle: &'a silksurf_gui::WinitWakeHandle,
    render_config: &'a BrowserRenderConfig,
    image_cache: &'a Arc<Mutex<ImageResourceCache>>,
}

#[derive(Clone, Copy)]
struct TextItemPaint {
    rect: Rect,
    font_size: f32,
    color: silksurf_css::Color,
}

#[derive(Clone, Copy)]
struct PixelRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct AppOptions {
    speculative: bool,
    window_mode: bool,
    winit_mode: bool,
    display_backend: silksurf_gui::WinitDisplayBackend,
    url: String,
    render_config: BrowserRenderConfig,
}

type NavigationResult = Result<BrowserPagePayload, String>;
type NavigationMessage = (u64, NavigationResult);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingHistoryAction {
    Push,
    MoveTo(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BrowserRedrawMode {
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
enum AddressCaretMotion {
    Backward,
    Forward,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BrowserBitmapRefresh {
    Clean,
    Full,
    ScrollReuse(Rect),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserChromeAction {
    Back,
    Forward,
    Home,
    Reload,
    Stop,
}

fn parse_display_backend_arg(args: &[String]) -> Result<silksurf_gui::WinitDisplayBackend, String> {
    let value = args
        .windows(2)
        .find_map(|window| (window[0] == "--display-backend").then_some(window[1].as_str()))
        .or_else(|| {
            args.iter()
                .find_map(|arg| arg.strip_prefix("--display-backend="))
        });
    match value.unwrap_or("auto") {
        "auto" => Ok(silksurf_gui::WinitDisplayBackend::Auto),
        "wayland" => Ok(silksurf_gui::WinitDisplayBackend::Wayland),
        "x11" => Ok(silksurf_gui::WinitDisplayBackend::X11),
        other => Err(format!(
            "--display-backend must be auto, wayland, or x11; got {other}"
        )),
    }
}

fn positional_url_arg(args: &[String]) -> Option<String> {
    let mut skip_next = false;
    for arg in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            "--backend" | "--tls-ca-file" | "--display-backend" => {
                skip_next = true;
            }
            _ if arg.starts_with("--backend=")
                || arg.starts_with("--tls-ca-file=")
                || arg.starts_with("--display-backend=") => {}
            _ if arg.starts_with('-') => {}
            _ => return Some(arg.clone()),
        }
    }
    None
}

fn install_observability() {
    #[cfg(feature = "structured-tracing")]
    install_structured_tracing();
    install_panic_hook();
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("[SilkSurf] process panicking: {info}");
        default_hook(info);
    }));
}

#[cfg(feature = "structured-tracing")]
fn install_structured_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(
                "silksurf=info"
                    .parse()
                    // UNWRAP-OK: silksurf=info is a static tracing directive.
                    .expect("silksurf=info is a valid tracing directive"),
            ),
        )
        .with_writer(std::io::stderr)
        .init();
}

fn parse_app_options(args: &[String]) -> Result<AppOptions, String> {
    let insecure = args.iter().any(|arg| arg == "--insecure" || arg == "-k");
    let platform_verifier = args.iter().any(|arg| arg == "--platform-verifier");
    let speculative = args.iter().any(|arg| arg == "--speculative" || arg == "-s");
    let window_mode = args.iter().any(|arg| arg == "--window");
    let winit_mode = args.iter().any(|arg| arg == "--backend=winit")
        || args
            .windows(2)
            .any(|window| window[0] == "--backend" && window[1] == "winit");
    let display_backend = parse_display_backend_arg(args)?;
    let tls_ca_file = parse_tls_ca_file_arg(args);
    let url = positional_url_arg(args).unwrap_or_else(|| "https://example.com".to_string());
    log_startup_options(insecure, platform_verifier, tls_ca_file.as_ref());
    Ok(AppOptions {
        speculative,
        window_mode,
        winit_mode,
        display_backend,
        url,
        render_config: BrowserRenderConfig {
            insecure,
            platform_verifier,
            tls_ca_file,
        },
    })
}

fn parse_tls_ca_file_arg(args: &[String]) -> Option<std::path::PathBuf> {
    args.windows(2)
        .find_map(|window| {
            (window[0] == "--tls-ca-file").then(|| std::path::PathBuf::from(&window[1]))
        })
        .or_else(|| {
            args.iter().find_map(|arg| {
                arg.strip_prefix("--tls-ca-file=")
                    .map(std::path::PathBuf::from)
            })
        })
}

fn log_startup_options(
    insecure: bool,
    platform_verifier: bool,
    tls_ca_file: Option<&std::path::PathBuf>,
) {
    if insecure {
        eprintln!("[SilkSurf] WARNING: TLS certificate verification disabled (--insecure)");
    }
    if platform_verifier {
        eprintln!("[SilkSurf] TLS platform verifier requested");
    }
    if let Some(path) = tls_ca_file {
        eprintln!("[SilkSurf] Extra CA bundle: {}", path.display());
    }
}

fn run_legacy_window_mode() -> ! {
    #[cfg(not(feature = "xcb-backend"))]
    {
        eprintln!("[SilkSurf] Rebuild with `--features xcb-backend` to use --window");
        std::process::exit(1);
    }
    #[cfg(feature = "xcb-backend")]
    {
        match silksurf_gui::XcbWindow::new("silksurf", 1280, 720) {
            Ok(mut window) => run_legacy_xcb_window(&mut window),
            Err(err) => {
                eprintln!("[SilkSurf] --window: cannot open display: {err}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(feature = "xcb-backend")]
fn run_legacy_xcb_window(window: &mut silksurf_gui::XcbWindow) -> ! {
    let mut pixels: Vec<u32> = vec![0; 1280usize * 720usize];
    silksurf_render::fill_scalar(&mut pixels, 0xFF64_95ED);
    window.present(&pixels);
    let mut event_loop = silksurf_gui::EventLoop::new();
    let run_result = event_loop.run(window, |event| match event {
        silksurf_gui::Event::Close | silksurf_gui::Event::KeyPress { keysym: 0x09 } => {
            silksurf_gui::ControlFlow::Exit
        }
        _ => silksurf_gui::ControlFlow::Continue,
    });
    if let Err(err) = run_result {
        eprintln!("[SilkSurf] window event loop error: {err}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn run_winit_browser_page(
    display_backend: silksurf_gui::WinitDisplayBackend,
    render_config: BrowserRenderConfig,
    image_cache: Arc<Mutex<ImageResourceCache>>,
    page: BrowserPage,
) {
    let url = page.frame.url.clone();
    let initial_modulepreload_urls = runtime_module_warm_urls(&page.runtime, &url);
    let initial_window_height = initial_browser_window_height(page.frame.raster_height);
    let browser_state = Rc::new(RefCell::new(BrowserState {
        frame: page.frame,
        runtime: Some(page.runtime),
        navigation_pending: false,
        status_text: "ready".to_string(),
        hover_status_text: None,
        history: vec![url.clone()],
        history_index: 0,
        pending_history: None,
        navigation_generation: 0,
        address_editing: false,
        address_select_all: false,
        address_text: url,
        address_cursor: 0,
        focused_input: None,
        redraw_mode: BrowserRedrawMode::Full,
        retained_present: None,
    }));
    #[cfg(feature = "accessibility")]
    log_accessibility_snapshot(&browser_state.borrow());
    let navigation_rx: Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>> =
        Rc::new(RefCell::new(None));
    let scroll_y = Rc::new(Cell::new(0.0f32));
    let last_render_width = Rc::new(Cell::new(0u32));
    let last_render_height = Rc::new(Cell::new(0u32));
    let chrome_height = BROWSER_CHROME_HEIGHT as u32;
    let trace_app_frame = std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_some();
    let resolved_display_backend = display_backend.resolve_for_current_environment();
    let window =
        match silksurf_gui::WinitWindow::new("silksurf", FRAME_WIDTH, initial_window_height) {
            Ok(window) => window.with_display_backend(display_backend),
            Err(err) => {
                eprintln!("[SilkSurf] winit: cannot open display: {err}");
                std::process::exit(1);
            }
        };
    eprintln!(
        "[SilkSurf] Display backend: configured={display_backend:?} resolved={resolved_display_backend:?}"
    );

    let render_state = Rc::clone(&browser_state);
    let render_scroll = Rc::clone(&scroll_y);
    let render_last_width = Rc::clone(&last_render_width);
    let render_last_height = Rc::clone(&last_render_height);
    let render_modulepreload = Rc::new(RefCell::new(Some((
        initial_modulepreload_urls,
        render_config.clone(),
    ))));
    let render_modulepreload_state = Rc::clone(&render_modulepreload);
    let ready_state = Rc::clone(&browser_state);
    let ready_last_width = Rc::clone(&last_render_width);
    let ready_last_height = Rc::clone(&last_render_height);
    let action_state = Rc::clone(&browser_state);
    let action_last_width = Rc::clone(&last_render_width);
    let action_last_height = Rc::clone(&last_render_height);
    let retained_update_state = Rc::clone(&browser_state);
    let retained_prepared_state = Rc::clone(&browser_state);
    let presented_state = Rc::clone(&browser_state);
    let presented_last_width = Rc::clone(&last_render_width);
    let presented_last_height = Rc::clone(&last_render_height);
    let input_state = Rc::clone(&browser_state);
    let input_navigation_rx = Rc::clone(&navigation_rx);
    let input_scroll = Rc::clone(&scroll_y);
    let input_render_config = render_config.clone();
    let input_image_cache = Arc::clone(&image_cache);
    let wake_state = Rc::clone(&browser_state);
    let wake_navigation_rx = Rc::clone(&navigation_rx);
    let wake_scroll = Rc::clone(&scroll_y);
    let wake_last_height = Rc::clone(&last_render_height);

    window.run_with_input_wake_and_render_actions(
        move |width, height, buffer_age, pixels| {
            let damage = render_browser_window_frame(
                &render_state,
                &render_scroll,
                &render_last_width,
                &render_last_height,
                chrome_height,
                trace_app_frame,
                width,
                height,
                buffer_age,
                pixels,
            );
            if let Some((urls, config)) = render_modulepreload_state.borrow_mut().take() {
                preload_module_scripts(&urls, &config);
            }
            damage
        },
        move |width, height| {
            browser_render_ready(
                &ready_state,
                &ready_last_width,
                &ready_last_height,
                width,
                height,
            )
        },
        move |width, height| {
            browser_render_action(
                &action_state,
                &action_last_width,
                &action_last_height,
                width,
                height,
            )
        },
        move |width, height| browser_retained_buffer_update(&retained_update_state, width, height),
        move |tag| handle_browser_retained_buffer_prepared(&retained_prepared_state, tag),
        move |frame| {
            handle_browser_presented_frame(
                &presented_state,
                &presented_last_width,
                &presented_last_height,
                frame,
            )
        },
        move |input, window_width, window_height, wake_handle| {
            handle_browser_input(
                input,
                BrowserInputRuntime {
                    state: &input_state,
                    navigation_rx: &input_navigation_rx,
                    scroll: &input_scroll,
                    chrome_height,
                    window_width,
                    window_height,
                    wake_handle,
                    render_config: &input_render_config,
                    image_cache: &input_image_cache,
                },
            )
        },
        move || {
            handle_browser_wake(
                &wake_state,
                &wake_navigation_rx,
                &wake_scroll,
                wake_last_height.get(),
            )
        },
    );
}

fn main() {
    /*
     * The default browser runtime keeps startup observability small: status
     * lines go to stderr directly and the panic hook adds a [SilkSurf] prefix
     * before delegating to Rust's default hook. The structured tracing
     * subscriber is available through the structured-tracing feature.
     *
     * mimalloc aborts on OOM natively in release builds. The nightly-only
     * alloc_error_hook API is not part of the stable runtime surface.
     */
    install_observability();

    let args: Vec<String> = std::env::args().collect();
    let options = match parse_app_options(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("[SilkSurf] {message}");
            return;
        }
    };

    /*
     * --window opens the XCB backend, presents a placeholder frame, and pumps
     * events until Close or Escape. This legacy backend isolates XCB window
     * setup from the fetch, JS, layout, and raster paths.
     *
     * XcbWindow::new() reports headless display failures as SilkError. The app
     * converts that error into stderr plus exit code 1.
     */
    if options.window_mode {
        run_legacy_window_mode();
    }

    let image_cache = Arc::new(Mutex::new(ImageResourceCache::new()));
    let mut renderer = match renderer_from_config(&options.render_config) {
        Ok(renderer) => renderer,
        Err(message) => {
            eprintln!("[SilkSurf] {message}");
            return;
        }
    };

    if options.winit_mode {
        match load_navigation_payload(
            &BrowserNavigationRequest::get(options.url.clone()),
            &options.render_config,
            &image_cache,
        )
        .and_then(build_browser_page)
        {
            Ok(page) => {
                run_winit_browser_page(
                    options.display_backend,
                    options.render_config,
                    image_cache,
                    page,
                );
            }
            Err(message) => eprintln!("[SilkSurf] {message}"),
        }
        return;
    }

    run_static_browser_render(&options, &mut renderer, &image_cache);
}

fn run_static_browser_render(
    options: &AppOptions,
    renderer: &mut SpeculativeRenderer,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) {
    eprintln!("[SilkSurf] Fetching: {}", options.url);
    let (response, fetch_origin, fetch_elapsed) =
        match renderer.fetch_or_speculate(&options.url, &[]) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[SilkSurf] Fetch error: {}", e.message);
                return;
            }
        };

    match fetch_origin {
        FetchOrigin::Cache => eprintln!(
            "[SilkSurf] CACHE HIT: {} bytes in {:?}",
            response.body.len(),
            fetch_elapsed
        ),
        FetchOrigin::Fresh => eprintln!(
            "[SilkSurf] FETCHED: {} bytes in {:?} (now cached)",
            response.body.len(),
            fetch_elapsed
        ),
    }

    /*
     * Background revalidation sends conditional GET headers from a worker
     * thread on cache hits. Rendering proceeds against cached bytes and later
     * consumes the revalidation result.
     */
    let revalidation_handle = if fetch_origin == FetchOrigin::Cache && options.speculative {
        eprintln!(
            "[SilkSurf] Spawning background revalidation for {}",
            options.url
        );
        Some(renderer.spawn_revalidation(&options.url))
    } else {
        None
    };

    eprintln!(
        "[SilkSurf] Response: {} ({} bytes)",
        response.status,
        response.body.len()
    );

    let html = String::from_utf8_lossy(&response.body).to_string();

    // 2. Parse HTML into DOM
    let document = match parse_html(&html) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("[SilkSurf] Parse error: {e:?}");
            return;
        }
    };

    let doc_node = document.document;
    let dom = document.dom;
    eprintln!("[SilkSurf] DOM parsed successfully");

    // 3. Extract inline CSS from <style> tags + fetch external stylesheets
    let inline_css = extract_inline_css(&dom, doc_node);
    let mut css_text = stylesheet_text_with_user_agent_defaults(&inline_css);
    eprintln!(
        "[SilkSurf] Extracted {} bytes of inline CSS",
        inline_css.len()
    );

    /*
     * fetch_all_or_speculate loads external stylesheet links through the
     * cache-first resource path. Same-host HTTPS requests share HTTP/2
     * multiplexing when the server supports it; cached stylesheets return
     * without network delay.
     */
    append_static_external_stylesheets(renderer, &dom, doc_node, &options.url, &mut css_text);

    let image_urls = extract_image_urls(&dom, doc_node, &options.url);
    let decoded_images = {
        let mut image_cache = image_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fetch_decoded_images(renderer, &mut image_cache, &image_urls)
    };
    if !image_urls.is_empty() {
        eprintln!(
            "[SilkSurf] Images decoded: {}/{}",
            decoded_images.len(),
            image_urls.len()
        );
    }

    eprintln!("[SilkSurf] Total CSS to parse: {} bytes", css_text.len());

    // 4. Parse CSS -- cache-first via StylesheetCache in SpeculativeRenderer.
    // On first render: full tokenize+parse (~2.5ms for ChatGPT's 128KB CSS).
    // On subsequent renders with same CSS bytes: clone Arc + intern_rules (~200us).
    let css_start = std::time::Instant::now();
    let stylesheet = dom
        .with_interner_mut(|interner| renderer.get_or_parse_stylesheet(&css_text, interner))
        .unwrap_or_else(|| {
            // UNWRAP-OK: parse_stylesheet_with_interner on the empty string can only fail on
            // tokenizer errors; the empty input has none. This is the canonical empty-stylesheet
            // construction.
            silksurf_css::parse_stylesheet_with_interner(
                "",
                &mut silksurf_core::SilkInterner::new(),
            )
            .unwrap()
        });
    eprintln!("[SilkSurf] CSS parsed in {:?}", css_start.elapsed());

    // 5. Extract inline script text before wrapping Dom for the JS context.
    let scripts = extract_inline_scripts(&dom, doc_node);
    eprintln!("[SilkSurf] Found {} inline script(s)", scripts.len());

    // Viewport dimensions used by fused pipeline and rasterizer
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    // 6. Create JS context with live DOM bridge (boa_engine + silksurf_dom).
    //    Arc<Mutex<Dom>> lets the JS context read/write the same DOM that the
    //    HTML parser built, so getElementById and friends work on real content.
    let dom_arc = Arc::new(Mutex::new(dom));
    let mut js_ctx = SilkContext::with_dom(&dom_arc);

    // 7. Execute inline <script> tags.
    execute_static_inline_scripts(&mut js_ctx, &scripts);

    // 7. Drain pending microtasks and Promise reactions.
    js_ctx.run_pending_jobs();
    drain_initial_host_callbacks(&mut js_ctx);

    // 8. Fused style+layout+paint: single BFS pass over post-JS DOM.
    //    Replaces separate compute_styles + build_layout_tree + build_display_list calls.
    //    Running post-JS ensures DOM mutations from scripts are visible in the render.
    let fused_start = std::time::Instant::now();
    let dom_guard = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom_guard, doc_node, &options.url, &decoded_images);
    let mut fused = fused_style_layout_paint_with_replaced_sizes(
        &dom_guard,
        &stylesheet,
        doc_node,
        viewport,
        &replaced_sizes,
    );
    let fused_elapsed = fused_start.elapsed();
    let styled_count = fused.styles.iter().filter(|s| s.is_some()).count();
    eprintln!(
        "[SilkSurf] Fused style+layout+paint: {} items, {} styled nodes in {:?}",
        fused.display_items.len(),
        styled_count,
        fused_elapsed
    );
    if let Some(&bfs_idx) = fused.table.node_to_bfs_idx.get(&doc_node) {
        let root_rect = &fused.node_rects[bfs_idx as usize];
        eprintln!(
            "[SilkSurf] Root: {}x{} at ({}, {})",
            root_rect.width, root_rect.height, root_rect.x, root_rect.y
        );
    }

    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut fused.display_items),
        tiles: None,
    };
    append_image_display_items(
        &dom_guard,
        &fused,
        &options.url,
        &decoded_images,
        &mut display_list.items,
    );
    drop(dom_guard);
    let bitmap_height = FRAME_HEIGHT;

    /*
     * rasterize_skia_into provides anti-aliased paths, gradients,
     * rounded-corner arcs, box shadows, and cosmic-text glyph compositing.
     * The cosmic-text FontSystem uses shared state, so this path keeps text
     * rasterization single-threaded.
     */
    let raster_start = std::time::Instant::now();
    let mut raster_buf: Vec<u8> = Vec::new();
    silksurf_render::rasterize_skia_into(
        &display_list,
        FRAME_WIDTH,
        bitmap_height,
        &mut raster_buf,
    );
    let raster_elapsed = raster_start.elapsed();
    eprintln!(
        "[SilkSurf] Rasterized: {} bytes in {:?}",
        raster_buf.len(),
        raster_elapsed
    );

    eprintln!("\n=== PROCESSING BUDGET (excludes network) ===");
    eprintln!(
        "  CSS parse:      {:?}",
        css_start
            .elapsed()
            .saturating_sub(fused_elapsed)
            .saturating_sub(raster_elapsed)
    );
    eprintln!("  Fused pipeline: {fused_elapsed:?}");
    eprintln!("  Rasterize:      {raster_elapsed:?}");
    eprintln!("  TOTAL:          {:?}", css_start.elapsed());
    eprintln!("============================================\n");

    eprintln!("[SilkSurf] Pipeline complete for {}", options.url);

    /*
     * Revalidation completes after the initial render. A 304 response keeps
     * the cached render valid. A 200 response updates the cache and diffs the
     * cached DOM against the new DOM so the changed surface is observable.
     */
    if let Some(handle) = revalidation_handle
        && let Err(message) = handle_revalidation(
            handle,
            renderer,
            &options.url,
            &dom_arc,
            doc_node,
            &css_text,
            viewport,
            &fused,
            &mut raster_buf,
        )
    {
        eprintln!("[SilkSurf] {message}");
    }
}

fn append_static_external_stylesheets(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    doc_node: silksurf_dom::NodeId,
    url: &str,
    css_text: &mut String,
) {
    let stylesheet_urls = extract_stylesheet_urls(dom, doc_node, url);
    let css_accept_header = [("Accept".to_string(), "text/css,*/*".to_string())];
    let sheet_requests: Vec<(&str, &[(String, String)])> = stylesheet_urls
        .iter()
        .map(|u| (u.as_str(), css_accept_header.as_slice()))
        .collect();

    let sheet_results = renderer.fetch_all_or_speculate(&sheet_requests);
    for (result, sheet_url) in sheet_results.into_iter().zip(stylesheet_urls.iter()) {
        match result {
            Ok((resp, origin, elapsed)) if resp.status == 200 => {
                eprintln!(
                    "[SilkSurf] Stylesheet {sheet_url}: {} bytes ({:?} {:?})",
                    resp.body.len(),
                    origin,
                    elapsed
                );
                let sheet_css = String::from_utf8_lossy(&resp.body);
                css_text.push_str(&sheet_css);
                css_text.push('\n');
            }
            Ok((resp, _, _)) => {
                eprintln!("[SilkSurf] Stylesheet {sheet_url}: HTTP {}", resp.status);
            }
            Err(e) => eprintln!(
                "[SilkSurf] Stylesheet {sheet_url}: fetch error: {}",
                e.message
            ),
        }
    }
}

fn execute_static_inline_scripts(js_ctx: &mut SilkContext, scripts: &[String]) {
    for (i, script) in scripts.iter().enumerate() {
        const MAX_INLINE_SCRIPT: usize = 256 * 1024;
        if script.len() > MAX_INLINE_SCRIPT {
            eprintln!(
                "[SilkSurf] Script {i}: {} bytes (skipping -- bundle too large)",
                script.len()
            );
            continue;
        }
        log_static_script_start(i, script);
        let script_start = std::time::Instant::now();
        match js_ctx.eval(script) {
            Ok(()) => eprintln!(
                "[SilkSurf] Script {i} executed OK ({:?})",
                script_start.elapsed()
            ),
            Err(e) => eprintln!(
                "[SilkSurf] Script {i} error: {e} ({:?})",
                script_start.elapsed()
            ),
        }
    }
}

fn log_static_script_start(index: usize, script: &str) {
    if script.len() <= 1200 {
        eprintln!(
            "[SilkSurf] Script {index} FULL ({} bytes): {script}",
            script.len()
        );
        return;
    }
    let preview = &script[..script.len().min(80)];
    eprintln!(
        "[SilkSurf] Executing script {index} ({} bytes): {preview}...",
        script.len()
    );
}

fn load_navigation_payload(
    request: &BrowserNavigationRequest,
    config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> NavigationResult {
    let mut renderer = renderer_from_config(config)?;
    let url = request.url.as_str();
    let (response, fetch_origin, fetch_elapsed) =
        if request.method == HttpMethod::Get && request.body.is_empty() {
            renderer
                .fetch_or_speculate(url, &request.headers)
                .map_err(|err| format!("{url}: fetch error: {}", err.message))?
        } else {
            let http_request = request.as_http_request();
            renderer
                .fetch_uncached_request(&http_request)
                .map_err(|err| format!("{url}: fetch error: {}", err.message))?
        };
    match fetch_origin {
        FetchOrigin::Cache => eprintln!(
            "[SilkSurf] Navigation cache hit: {} bytes in {:?}",
            response.body.len(),
            fetch_elapsed
        ),
        FetchOrigin::Fresh => match request.method {
            HttpMethod::Get => eprintln!(
                "[SilkSurf] Navigation fetched: {} bytes in {:?}",
                response.body.len(),
                fetch_elapsed
            ),
            HttpMethod::Post => eprintln!(
                "[SilkSurf] Navigation posted: {} bytes in {:?}",
                response.body.len(),
                fetch_elapsed
            ),
            _ => eprintln!(
                "[SilkSurf] Navigation fetched via {}: {} bytes in {:?}",
                http_method_label(request.method),
                response.body.len(),
                fetch_elapsed
            ),
        },
    }

    let html = String::from_utf8_lossy(&response.body).to_string();
    let document = parse_html(&html).map_err(|err| format!("{url}: parse error: {err:?}"))?;
    let doc_node = document.document;
    let dom = &document.dom;

    let inline_css = extract_inline_css(dom, doc_node);
    let mut css_text = stylesheet_text_with_user_agent_defaults(&inline_css);
    let stylesheet_urls = extract_stylesheet_urls(dom, doc_node, url);
    let css_accept_header = [("Accept".to_string(), "text/css,*/*".to_string())];
    let sheet_requests: Vec<(&str, &[(String, String)])> = stylesheet_urls
        .iter()
        .map(|sheet_url| (sheet_url.as_str(), css_accept_header.as_slice()))
        .collect();
    for (result, sheet_url) in renderer
        .fetch_all_or_speculate(&sheet_requests)
        .into_iter()
        .zip(stylesheet_urls.iter())
    {
        match result {
            Ok((resp, _, _)) if resp.status == 200 => {
                let sheet_css = String::from_utf8_lossy(&resp.body);
                css_text.push_str(&sheet_css);
                css_text.push('\n');
            }
            Ok((resp, _, _)) => {
                eprintln!(
                    "[SilkSurf] Navigation stylesheet {sheet_url}: HTTP {}",
                    resp.status
                );
            }
            Err(err) => {
                eprintln!(
                    "[SilkSurf] Navigation stylesheet {sheet_url}: fetch error: {}",
                    err.message
                );
            }
        }
    }

    let image_urls = extract_image_urls(dom, doc_node, url);
    let images = {
        let mut image_cache = image_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fetch_decoded_images(&mut renderer, &mut image_cache, &image_urls)
    };
    let script_texts = load_document_script_texts(&mut renderer, dom, doc_node, url);
    let module_texts = load_document_module_texts(&mut renderer, dom, doc_node, url);

    Ok(BrowserPagePayload {
        url: url.to_string(),
        html,
        css_text,
        script_texts,
        module_texts,
        images,
        render_config: config.clone(),
    })
}

fn build_browser_page(payload: BrowserPagePayload) -> Result<BrowserPage, String> {
    build_browser_page_with_buffers(payload, BrowserFrameBuffers::default())
        .map_err(|err| err.message)
}

fn build_browser_page_with_buffers(
    payload: BrowserPagePayload,
    buffers: BrowserFrameBuffers,
) -> Result<BrowserPage, BrowserPageBuildError> {
    build_browser_page_with_buffers_for_height(payload, buffers, None)
}

fn build_browser_page_with_buffers_for_height(
    payload: BrowserPagePayload,
    buffers: BrowserFrameBuffers,
    live_window_height: Option<u32>,
) -> Result<BrowserPage, BrowserPageBuildError> {
    let trace_build = std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_some()
        || std::env::var_os("SILKSURF_TRACE_NAV_BUILD").is_some();
    let build_start = std::time::Instant::now();
    let phase_start = std::time::Instant::now();
    let document = match parse_html(&payload.html) {
        Ok(document) => document,
        Err(err) => {
            return Err(BrowserPageBuildError {
                message: format!("{}: parse error: {err:?}", payload.url),
                buffers,
            });
        }
    };
    trace_navigation_build_phase(trace_build, &payload.url, "html", phase_start.elapsed());
    let doc_node = document.document;
    let dom = document.dom;
    let phase_start = std::time::Instant::now();
    let stylesheet = match dom.with_interner_mut(|interner| {
        silksurf_css::parse_stylesheet_with_interner(&payload.css_text, interner).ok()
    }) {
        Some(stylesheet) => stylesheet,
        None => {
            return Err(BrowserPageBuildError {
                message: format!("{}: CSS parse failed", payload.url),
                buffers,
            });
        }
    };
    trace_navigation_build_phase(trace_build, &payload.url, "css", phase_start.elapsed());
    let scripts = if payload.script_texts.is_empty() {
        extract_inline_scripts(&dom, doc_node)
    } else {
        payload.script_texts
    };
    let mut executed_script_nodes = initial_executed_script_nodes(&dom, doc_node, &payload.url);
    let viewport = Rect {
        x: 0.0,
        y: BROWSER_CHROME_HEIGHT,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
    };
    let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
    let dom_arc = Arc::new(Mutex::new(dom));
    let mut js_ctx = SilkContext::with_dom(&dom_arc);
    {
        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = dom.take_dirty_nodes();
    }
    let phase_start = std::time::Instant::now();
    let script_phase_start = phase_start;
    let static_eval_start = std::time::Instant::now();
    for (idx, script) in scripts.iter().enumerate() {
        if script.len() > MAX_NAVIGATION_SCRIPT_BYTES {
            eprintln!(
                "[SilkSurf] Navigation script {idx}: {} bytes skipped",
                script.len()
            );
            continue;
        }
        trace_navigation_script(trace_build, idx, script.len(), "start", None);
        let script_start = std::time::Instant::now();
        if let Err(err) = js_ctx.eval(script) {
            eprintln!("[SilkSurf] Navigation script {idx} error: {err}");
        }
        trace_navigation_script(
            trace_build,
            idx,
            script.len(),
            "done",
            Some(script_start.elapsed()),
        );
    }
    trace_navigation_script_phase(trace_build, "static-eval", static_eval_start.elapsed());
    let jobs_start = std::time::Instant::now();
    js_ctx.run_pending_jobs();
    trace_navigation_script_phase(trace_build, "static-jobs", jobs_start.elapsed());
    let host_callbacks_start = std::time::Instant::now();
    drain_initial_host_callbacks(&mut js_ctx);
    trace_navigation_script_phase(
        trace_build,
        "static-host-callbacks",
        host_callbacks_start.elapsed(),
    );
    let dirty_drain_start = std::time::Instant::now();
    let dynamic_dirty_nodes = take_dom_dirty_nodes(&dom_arc);
    trace_navigation_script_phase(trace_build, "dirty-drain", dirty_drain_start.elapsed());
    let dynamic_start = std::time::Instant::now();
    execute_dynamic_classic_scripts(
        &payload.url,
        &payload.render_config,
        &dom_arc,
        &mut js_ctx,
        &mut executed_script_nodes,
        dynamic_dirty_nodes,
        trace_build,
    );
    trace_navigation_script_phase(trace_build, "dynamic-total", dynamic_start.elapsed());
    let module_start = std::time::Instant::now();
    execute_static_module_scripts(
        &payload.url,
        &dom_arc,
        doc_node,
        &mut js_ctx,
        &payload.module_texts,
        trace_build,
    );
    trace_navigation_script_phase(trace_build, "module-total", module_start.elapsed());
    let trace_body_start = std::time::Instant::now();
    trace_navigation_body_data_fixture(trace_build, &dom_arc);
    trace_navigation_script_phase(trace_build, "trace-body", trace_body_start.elapsed());
    let trace_scripts_start = std::time::Instant::now();
    trace_navigation_script_nodes(trace_build, &dom_arc, &executed_script_nodes);
    trace_navigation_script_phase(
        trace_build,
        "trace-script-nodes",
        trace_scripts_start.elapsed(),
    );
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "scripts",
        script_phase_start.elapsed(),
    );

    let phase_start = std::time::Instant::now();
    let dom_guard = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom_guard, doc_node, &payload.url, &payload.images);
    let mut fused_workspace = FusedWorkspace::new();
    fused_workspace.run_with_replaced_sizes(
        &dom_guard,
        &stylesheet,
        &style_index,
        doc_node,
        viewport,
        &replaced_sizes,
    );
    let mut fused = fused_workspace.snapshot_result();
    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut fused.display_items),
        tiles: None,
    };
    append_image_display_items(
        &dom_guard,
        &fused,
        &payload.url,
        &payload.images,
        &mut display_list.items,
    );
    let link_targets = collect_link_targets(&dom_guard, &display_list.items, &payload.url);
    let input_targets = collect_input_targets(&dom_guard, &fused);
    let focus_target = first_prepared_focus_target(&dom_guard, &input_targets);
    drop(dom_guard);
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "layout-paint",
        phase_start.elapsed(),
    );

    let phase_start = std::time::Instant::now();
    let document_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
    display_list = tile_browser_document_display_list(display_list, document_height);
    let bitmap_height = browser_page_bitmap_height(document_height, live_window_height);
    trace_navigation_build_phase(trace_build, &payload.url, "tiles", phase_start.elapsed());
    let BrowserFrameBuffers { mut rgba, mut argb } = buffers;
    let phase_start = std::time::Instant::now();
    if rasterize_browser_viewport_argb_direct(&display_list, 0, bitmap_height, &mut argb) {
        trace_navigation_build_phase(
            trace_build,
            &payload.url,
            "argb-direct",
            phase_start.elapsed(),
        );
        trace_navigation_build_buffer(trace_build, &payload.url, "rgba", rgba.len());
    } else {
        trace_navigation_build_phase(
            trace_build,
            &payload.url,
            "argb-direct-miss",
            phase_start.elapsed(),
        );
        let phase_start = std::time::Instant::now();
        rasterize_browser_viewport_into(&display_list, 0, bitmap_height, &mut rgba);
        trace_navigation_build_phase(trace_build, &payload.url, "raster", phase_start.elapsed());
        trace_navigation_build_buffer(trace_build, &payload.url, "rgba", rgba.len());
        let phase_start = std::time::Instant::now();
        let (resize_elapsed, pack_elapsed) = rgba_bytes_to_argb_words_into_timed(&rgba, &mut argb);
        trace_navigation_build_phase(trace_build, &payload.url, "argb-resize", resize_elapsed);
        trace_navigation_build_phase(trace_build, &payload.url, "argb-pack", pack_elapsed);
        trace_navigation_build_phase(trace_build, &payload.url, "argb", phase_start.elapsed());
    }
    trace_navigation_build_buffer(trace_build, &payload.url, "argb", argb.len() * 4);
    let phase_start = std::time::Instant::now();
    let focus_viewport_cache = build_focus_viewport_cache(
        &display_list,
        focus_target.as_ref(),
        document_height,
        bitmap_height,
        BROWSER_CHROME_HEIGHT as u32,
    );
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "focus-viewport-cache",
        phase_start.elapsed(),
    );
    {
        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = dom.take_dirty_nodes();
    }
    trace_navigation_build_phase(trace_build, &payload.url, "total", build_start.elapsed());
    Ok(BrowserPage {
        frame: BrowserFrame {
            url: payload.url,
            argb,
            raster_height: document_height,
            bitmap_height,
            bitmap_scroll_y: 0,
            focus_viewport_cache,
            focus_viewport_retained_sent: false,
            current_view_retained_sent: false,
            navigation_start_retained_sent: false,
            scroll_viewport_caches: Vec::new(),
            link_targets,
            input_targets,
        },
        runtime: BrowserPageRuntime {
            dom: dom_arc,
            document: doc_node,
            stylesheet,
            style_index,
            viewport,
            js_ctx,
            fused,
            fused_workspace,
            display_list,
            images: payload.images,
            rgba,
            damage_scratch: silksurf_render::DamageScratch::default(),
        },
    })
}

fn browser_page_bitmap_height(document_height: u32, live_window_height: Option<u32>) -> u32 {
    live_window_height.map_or_else(
        || initial_browser_window_height(document_height),
        |height| height.max(BROWSER_CHROME_HEIGHT as u32),
    )
}

fn trace_navigation_build_phase(
    enabled: bool,
    url: &str,
    phase: &str,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!("[SilkSurf] Navigation build {phase}: {elapsed:?} for {url}");
    }
}

fn trace_navigation_build_buffer(enabled: bool, url: &str, name: &str, bytes: usize) {
    if enabled {
        eprintln!("[SilkSurf] Navigation build {name} buffer: {bytes} bytes for {url}");
    }
}

fn trace_navigation_script(
    enabled: bool,
    index: usize,
    bytes: usize,
    state: &str,
    elapsed: Option<std::time::Duration>,
) {
    if !enabled {
        return;
    }
    if let Some(elapsed) = elapsed {
        eprintln!("[SilkSurf] Navigation script {index} {state}: {bytes} bytes in {elapsed:?}");
    } else {
        eprintln!("[SilkSurf] Navigation script {index} {state}: {bytes} bytes");
    }
}

fn trace_navigation_script_phase(enabled: bool, name: &str, elapsed: std::time::Duration) {
    if enabled {
        eprintln!("[SilkSurf] Navigation script phase {name}: {elapsed:?}");
    }
}

fn initial_executed_script_nodes(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> HashSet<silksurf_dom::NodeId> {
    let mut nodes = HashSet::new();
    collect_classic_script_nodes(dom, root, base_url, &mut nodes);
    nodes
}

fn execute_dynamic_classic_scripts(
    base_url: &str,
    config: &BrowserRenderConfig,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    js_ctx: &mut SilkContext,
    executed_nodes: &mut HashSet<silksurf_dom::NodeId>,
    mut dirty_nodes: Vec<silksurf_dom::NodeId>,
    trace_build: bool,
) {
    for round in 0..MAX_DYNAMIC_SCRIPT_ROUNDS {
        let round_start = std::time::Instant::now();
        let find_start = std::time::Instant::now();
        let scripts = dynamic_classic_script_refs(base_url, dom_arc, executed_nodes, &dirty_nodes);
        trace_navigation_dynamic_phase(trace_build, round, "find", find_start.elapsed());
        if scripts.is_empty() {
            return;
        }
        execute_dynamic_script_round(base_url, config, js_ctx, trace_build, round, &scripts);
        for script in scripts {
            executed_nodes.insert(script.node);
        }
        let jobs_start = std::time::Instant::now();
        js_ctx.run_pending_jobs();
        trace_navigation_dynamic_phase(trace_build, round, "jobs", jobs_start.elapsed());
        let callbacks_start = std::time::Instant::now();
        drain_initial_host_callbacks(js_ctx);
        trace_navigation_dynamic_phase(
            trace_build,
            round,
            "host-callbacks",
            callbacks_start.elapsed(),
        );
        let dirty_start = std::time::Instant::now();
        dirty_nodes = take_dom_dirty_nodes(dom_arc);
        trace_navigation_dynamic_phase(trace_build, round, "dirty-drain", dirty_start.elapsed());
        trace_navigation_dynamic_phase(trace_build, round, "total", round_start.elapsed());
    }
    eprintln!(
        "[SilkSurf] Navigation dynamic scripts stopped after {MAX_DYNAMIC_SCRIPT_ROUNDS} rounds"
    );
}

fn execute_static_module_scripts(
    base_url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    root: silksurf_dom::NodeId,
    js_ctx: &mut SilkContext,
    module_texts: &[(String, String)],
    trace_build: bool,
) {
    if module_texts.is_empty() {
        return;
    }
    let root_urls = {
        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        external_module_script_urls(&dom, root, base_url)
    };
    for (idx, root_url) in dedupe_resource_urls(&root_urls).iter().enumerate() {
        let root_path = module_path_for_url(root_url);
        let root_len = module_texts
            .iter()
            .find_map(|(path, text)| (path == &root_path).then_some(text.len()))
            .unwrap_or(0);
        let module_start = std::time::Instant::now();
        match js_ctx.eval_module_graph(&root_path, module_texts) {
            Ok(()) => trace_navigation_script(
                trace_build,
                idx,
                root_len,
                "module-done",
                Some(module_start.elapsed()),
            ),
            Err(err) => eprintln!("[SilkSurf] Module {root_url} error: {err}"),
        }
    }
    js_ctx.run_pending_jobs();
}

fn take_dom_dirty_nodes(dom_arc: &Arc<Mutex<silksurf_dom::Dom>>) -> Vec<silksurf_dom::NodeId> {
    let mut dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    dom.take_dirty_nodes()
}

fn dynamic_classic_script_refs(
    base_url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    executed_nodes: &HashSet<silksurf_dom::NodeId>,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Vec<DocumentScriptNode> {
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut scripts = Vec::new();
    let mut seen_nodes = HashSet::new();
    for &node in dirty_nodes {
        if !seen_nodes.insert(node) {
            continue;
        }
        if let Some(source) = script_ref_for_node(&dom, node, base_url) {
            scripts.push(DocumentScriptNode { node, source });
        }
    }
    scripts
        .into_iter()
        .filter(|script| !executed_nodes.contains(&script.node))
        .collect()
}

fn execute_dynamic_script_round(
    base_url: &str,
    config: &BrowserRenderConfig,
    js_ctx: &mut SilkContext,
    trace_build: bool,
    round: usize,
    scripts: &[DocumentScriptNode],
) {
    let urls_start = std::time::Instant::now();
    let external_urls = dynamic_external_script_urls(base_url, scripts);
    trace_navigation_dynamic_phase(trace_build, round, "urls", urls_start.elapsed());
    let fetch_start = std::time::Instant::now();
    let fetched = fetch_dynamic_external_script_texts(config, &external_urls, trace_build, round);
    trace_navigation_dynamic_phase(trace_build, round, "fetch-total", fetch_start.elapsed());
    let eval_start = std::time::Instant::now();
    for (idx, script) in scripts.iter().enumerate() {
        let Some(text) = dynamic_script_text(base_url, script, &fetched) else {
            continue;
        };
        execute_dynamic_script_text(js_ctx, trace_build, round, idx, text);
    }
    trace_navigation_dynamic_phase(trace_build, round, "eval-total", eval_start.elapsed());
}

fn dynamic_external_script_urls(base_url: &str, scripts: &[DocumentScriptNode]) -> Vec<String> {
    scripts
        .iter()
        .filter_map(|script| match &script.source {
            DocumentScriptRef::External(url) => Some(resolve_resource_url(base_url, url)),
            DocumentScriptRef::Inline(_) => None,
        })
        .filter(|url| !url.is_empty())
        .collect()
}

fn fetch_dynamic_external_script_texts(
    config: &BrowserRenderConfig,
    urls: &[String],
    trace_build: bool,
    round: usize,
) -> Vec<(String, String)> {
    if urls.is_empty() {
        return Vec::new();
    }
    let renderer_start = std::time::Instant::now();
    let mut renderer = match ephemeral_renderer_from_config(config) {
        Ok(renderer) => renderer,
        Err(message) => {
            eprintln!("[SilkSurf] Navigation dynamic script renderer: {message}");
            return Vec::new();
        }
    };
    trace_navigation_dynamic_phase(trace_build, round, "renderer", renderer_start.elapsed());
    let request_start = std::time::Instant::now();
    let texts = fetch_external_script_texts(&mut renderer, urls);
    trace_navigation_dynamic_phase(
        trace_build,
        round,
        "fetch-requests",
        request_start.elapsed(),
    );
    texts
}

fn dynamic_script_text<'a>(
    base_url: &str,
    script: &'a DocumentScriptNode,
    fetched: &'a [(String, String)],
) -> Option<&'a str> {
    match &script.source {
        DocumentScriptRef::Inline(text) => Some(text.as_str()),
        DocumentScriptRef::External(url) => {
            let resolved = resolve_resource_url(base_url, url);
            fetched
                .iter()
                .find_map(|(fetched_url, text)| (fetched_url == &resolved).then_some(text.as_str()))
        }
    }
}

fn execute_dynamic_script_text(
    js_ctx: &mut SilkContext,
    trace_build: bool,
    round: usize,
    index: usize,
    script: &str,
) {
    if script.len() > MAX_NAVIGATION_SCRIPT_BYTES {
        eprintln!(
            "[SilkSurf] Navigation dynamic script {round}.{index}: {} bytes skipped",
            script.len()
        );
        return;
    }
    trace_navigation_dynamic_script(trace_build, round, index, script.len(), "start", None);
    let script_start = std::time::Instant::now();
    if let Err(err) = js_ctx.eval(script) {
        eprintln!("[SilkSurf] Navigation dynamic script {round}.{index} error: {err}");
    }
    trace_navigation_dynamic_script(
        trace_build,
        round,
        index,
        script.len(),
        "done",
        Some(script_start.elapsed()),
    );
}

fn trace_navigation_dynamic_script(
    enabled: bool,
    round: usize,
    index: usize,
    bytes: usize,
    state: &str,
    elapsed: Option<std::time::Duration>,
) {
    if !enabled {
        return;
    }
    if let Some(elapsed) = elapsed {
        eprintln!(
            "[SilkSurf] Navigation dynamic script {round}.{index} {state}: {bytes} bytes in {elapsed:?}"
        );
    } else {
        eprintln!("[SilkSurf] Navigation dynamic script {round}.{index} {state}: {bytes} bytes");
    }
}

fn trace_navigation_dynamic_phase(
    enabled: bool,
    round: usize,
    name: &str,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!("[SilkSurf] Navigation dynamic phase {round} {name}: {elapsed:?}");
    }
}

fn trace_navigation_body_data_fixture(enabled: bool, dom_arc: &Arc<Mutex<silksurf_dom::Dom>>) {
    if !enabled {
        return;
    }
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(body) = first_element_by_name(&dom, silksurf_dom::NodeId::from_raw(0), "body") else {
        return;
    };
    if let Some(value) = element_attribute(&dom, body, "data-fixture") {
        eprintln!("[SilkSurf] Navigation DOM body data-fixture={value}");
    }
    if let Some(value) = element_attribute(&dom, body, "data-dynamic-script") {
        eprintln!("[SilkSurf] Navigation DOM body data-dynamic-script={value}");
    }
    if let Some(value) = element_attribute(&dom, body, "data-module-graph") {
        eprintln!("[SilkSurf] Navigation DOM body data-module-graph={value}");
    }
}

fn trace_navigation_script_nodes(
    enabled: bool,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    script_nodes: &HashSet<silksurf_dom::NodeId>,
) {
    if !enabled {
        return;
    }
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for &node in script_nodes {
        if let Some(src) = element_attribute(&dom, node, "src") {
            let text_bytes = script_text_content(&dom, node).len();
            eprintln!("[SilkSurf] Navigation DOM script src={src} text_bytes={text_bytes}");
        }
    }
}

fn first_element_by_name(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
) -> Option<silksurf_dom::NodeId> {
    if dom
        .element_name(node)
        .ok()
        .flatten()
        .is_some_and(|element| element.eq_ignore_ascii_case(name))
    {
        return Some(node);
    }
    for &child in dom.children(node).ok()? {
        if let Some(found) = first_element_by_name(dom, child, name) {
            return Some(found);
        }
    }
    None
}

fn handle_revalidation(
    handle: silksurf_engine::speculative::RevalidationHandle,
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    let result = handle
        .wait()
        .map_err(|err| format!("Revalidation error: {}", err.message))?;
    if !result.changed {
        eprintln!(
            "[SilkSurf] Revalidation: 304 NOT MODIFIED in {:?} -- cached render is current, no re-render",
            result.rtt
        );
        return Ok(());
    }
    apply_changed_revalidation(
        result, renderer, url, dom_arc, doc_node, css_text, viewport, fused, raster_buf,
    )
}

fn apply_changed_revalidation(
    result: silksurf_engine::speculative::RevalidationResult,
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    eprintln!(
        "[SilkSurf] Revalidation: CONTENT CHANGED (200) in {:?}",
        result.rtt
    );
    let Some(response) = result.response else {
        return Ok(());
    };
    renderer.update_cache(url, &response);
    eprintln!(
        "[SilkSurf] Cache updated ({} bytes)",
        renderer.cache_bytes()
    );
    rerender_revalidated_cache(
        renderer, url, dom_arc, doc_node, css_text, viewport, fused, raster_buf,
    )
}

fn rerender_revalidated_cache(
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    let new_html = String::from_utf8_lossy(
        &renderer
            .cache
            .get(url)
            .map(|entry| entry.body.clone())
            .unwrap_or_default(),
    )
    .to_string();
    let new_doc = silksurf_engine::parse_html(&new_html)
        .map_err(|err| format!("Revalidation parse error: {err:?}"))?;
    let diff = revalidation_dom_diff(dom_arc, doc_node, &new_doc);
    if diff.is_empty() {
        eprintln!("[SilkSurf] DOM diff: no structural changes (cached render valid)");
        return Ok(());
    }
    rerender_revalidation_diff(
        renderer, css_text, viewport, fused, raster_buf, new_doc, diff,
    );
    Ok(())
}

fn revalidation_dom_diff(
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    new_doc: &silksurf_engine::ParsedDocument,
) -> silksurf_dom::diff::DomDiff {
    let orig_dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    silksurf_dom::diff::diff_doms(&orig_dom, doc_node, &new_doc.dom, new_doc.document)
}

fn rerender_revalidation_diff(
    renderer: &mut SpeculativeRenderer,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
    new_doc: silksurf_engine::ParsedDocument,
    diff: silksurf_dom::diff::DomDiff,
) {
    eprintln!(
        "[SilkSurf] DOM diff: {} changed, {} added, {} removed nodes -- re-rendering new DOM",
        diff.changed.len(),
        diff.added.len(),
        diff.removed.len(),
    );
    let rerender_t0 = std::time::Instant::now();
    let (css_elapsed, new_stylesheet) = parse_revalidation_stylesheet(renderer, css_text, &new_doc);
    let fused_t0 = std::time::Instant::now();
    let mut new_fused =
        fused_style_layout_paint(&new_doc.dom, &new_stylesheet, new_doc.document, viewport);
    let fused_elapsed = fused_t0.elapsed();
    let raster_elapsed = raster_revalidation_diff(&diff, fused, &mut new_fused, raster_buf);
    let total = rerender_t0.elapsed();
    let new_styled = new_fused
        .styles
        .iter()
        .filter(|style| style.is_some())
        .count();
    eprintln!(
        "[SilkSurf] Re-render ({new_styled} styled nodes): CSS {css_elapsed:?} + fused {fused_elapsed:?} + raster {raster_elapsed:?} = {total:?}",
    );
}

fn parse_revalidation_stylesheet(
    renderer: &mut SpeculativeRenderer,
    css_text: &str,
    new_doc: &silksurf_engine::ParsedDocument,
) -> (std::time::Duration, silksurf_css::Stylesheet) {
    let css_t0 = std::time::Instant::now();
    let stylesheet = new_doc
        .dom
        .with_interner_mut(|interner| renderer.get_or_parse_stylesheet(css_text, interner))
        .unwrap_or_else(|| {
            silksurf_css::parse_stylesheet_with_interner(
                "",
                &mut silksurf_core::SilkInterner::new(),
            )
            // UNWRAP-OK: empty CSS is always a valid stylesheet.
            .unwrap()
        });
    (css_t0.elapsed(), stylesheet)
}

fn raster_revalidation_diff(
    diff: &silksurf_dom::diff::DomDiff,
    fused: &FusedResult,
    new_fused: &mut FusedResult,
    raster_buf: &mut Vec<u8>,
) -> std::time::Duration {
    let raster_t0 = std::time::Instant::now();
    let damage = text_only_diff_damage_rect(diff, fused, new_fused);
    let new_display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut new_fused.display_items),
        tiles: None,
    };
    if let Some(damage) = damage {
        let mut damage_scratch = silksurf_render::DamageScratch::default();
        silksurf_render::rasterize_skia_damage_into(
            &new_display_list,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            damage,
            raster_buf,
            &mut damage_scratch,
        );
        eprintln!(
            "[SilkSurf] Re-render damage rect: {}x{} at ({}, {})",
            damage.width, damage.height, damage.x, damage.y
        );
    } else {
        silksurf_render::rasterize_skia_into(
            &new_display_list,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            raster_buf,
        );
    }
    raster_t0.elapsed()
}

fn renderer_from_config(config: &BrowserRenderConfig) -> Result<SpeculativeRenderer, String> {
    if config.insecure {
        return Ok(SpeculativeRenderer::with_insecure());
    }
    if let Some(ref ca_path) = config.tls_ca_file {
        return SpeculativeRenderer::with_extra_ca_file(ca_path)
            .map_err(|err| format!("--tls-ca-file: {}", err.message));
    }
    if config.platform_verifier {
        #[cfg(feature = "platform-verifier")]
        {
            return SpeculativeRenderer::with_platform_verifier()
                .map_err(|err| format!("TLS platform verifier: {}", err.message));
        }
        #[cfg(not(feature = "platform-verifier"))]
        {
            return Err("rebuild with --features platform-verifier".to_string());
        }
    }
    Ok(SpeculativeRenderer::new())
}

fn ephemeral_renderer_from_config(
    config: &BrowserRenderConfig,
) -> Result<SpeculativeRenderer, String> {
    if config.insecure {
        return Ok(SpeculativeRenderer::with_insecure_ephemeral());
    }
    if let Some(ref ca_path) = config.tls_ca_file {
        return SpeculativeRenderer::with_extra_ca_file_ephemeral(ca_path)
            .map_err(|err| format!("--tls-ca-file: {}", err.message));
    }
    if config.platform_verifier {
        #[cfg(feature = "platform-verifier")]
        {
            return SpeculativeRenderer::with_platform_verifier_ephemeral()
                .map_err(|err| format!("TLS platform verifier: {}", err.message));
        }
        #[cfg(not(feature = "platform-verifier"))]
        {
            return Err("rebuild with --features platform-verifier".to_string());
        }
    }
    Ok(SpeculativeRenderer::new_ephemeral())
}

fn drain_initial_host_callbacks(js_ctx: &mut SilkContext) {
    if !js_ctx.has_pending_host_callbacks() {
        return;
    }

    match js_ctx.run_host_callbacks(64) {
        Ok(count) if count > 0 => {
            eprintln!("[SilkSurf] Initial host callbacks: {count}");
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!("[SilkSurf] Initial host callback error: {err}");
        }
    }
}

fn tick_browser_runtime(state: &mut BrowserState) -> bool {
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };

    let redraw_mode = match repaint_runtime_host_callbacks(&mut runtime, &mut state.frame) {
        Ok(redraw_mode) => redraw_mode,
        Err(err) => {
            eprintln!("[SilkSurf] Runtime callback error: {err}");
            set_browser_status(state, "error");
            mark_redraw(state, BrowserRedrawMode::Chrome);
            state.runtime = Some(runtime);
            return true;
        }
    };
    state.runtime = Some(runtime);
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        return true;
    }
    false
}

fn repaint_runtime_host_callbacks(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
) -> Result<Option<BrowserRedrawMode>, String> {
    if !runtime.js_ctx.has_pending_host_callbacks() {
        return Ok(None);
    }

    let callback_count = runtime.js_ctx.run_host_callbacks(64)?;
    if callback_count == 0 {
        return Ok(None);
    }

    let dirty_nodes = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dom.take_dirty_nodes()
    };
    if dirty_nodes.is_empty() {
        return Ok(None);
    }

    let redraw_mode = repaint_runtime_dirty_nodes(runtime, frame, &dirty_nodes);
    eprintln!("[SilkSurf] Runtime host callbacks: {callback_count}");
    Ok(redraw_mode)
}

fn repaint_runtime_dirty_nodes(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Option<BrowserRedrawMode> {
    if dirty_nodes.is_empty() {
        return None;
    }

    if let Some(redraw_mode) = repaint_runtime_text_only_dirty_nodes(runtime, frame, dirty_nodes) {
        return Some(redraw_mode);
    }

    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom, runtime.document, &frame.url, &runtime.images);
    runtime.fused_workspace.run_with_replaced_sizes(
        &dom,
        &runtime.stylesheet,
        &runtime.style_index,
        runtime.document,
        runtime.viewport,
        &replaced_sizes,
    );
    let mut new_fused = runtime.fused_workspace.snapshot_result();
    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut new_fused.display_items),
        tiles: None,
    };
    append_image_display_items(
        &dom,
        &new_fused,
        &frame.url,
        &runtime.images,
        &mut display_list.items,
    );
    frame.link_targets = collect_link_targets(&dom, &display_list.items, &frame.url);
    frame.input_targets = collect_input_targets(&dom, &new_fused);
    let damage = dirty_nodes_damage_rect(&dom, dirty_nodes, &runtime.fused, &new_fused);
    drop(dom);

    let next_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
    display_list = tile_browser_document_display_list(display_list, next_height);
    frame.raster_height = next_height;
    let redraw_mode = if let Some(damage) = damage {
        rasterize_browser_document_damage_into(
            &display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
            sync_argb_damage_from_rgba(
                &runtime.rgba,
                &mut frame.argb,
                FRAME_WIDTH,
                frame.bitmap_height,
                viewport_damage_rect(damage, frame.bitmap_scroll_y),
            );
        }
        BrowserRedrawMode::Damage(damage)
    } else {
        rasterize_browser_viewport_argb_preferred(
            &display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            &mut runtime.rgba,
            &mut frame.argb,
        );
        BrowserRedrawMode::Full
    };
    runtime.display_list = display_list;

    runtime.fused = new_fused;
    Some(redraw_mode)
}

fn repaint_runtime_text_only_dirty_nodes(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Option<BrowserRedrawMode> {
    if let [node] = dirty_nodes {
        return repaint_single_runtime_text_node(runtime, frame, *node);
    }

    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut updates = Vec::with_capacity(dirty_nodes.len());
    let mut damage = None;
    for &node in dirty_nodes {
        let text = dirty_text_node_content(&dom, node)?;
        let item_index = dirty_text_display_item_index(&runtime.display_list.items, node)?;
        let item_damage =
            text_item_in_place_damage_rect(&runtime.display_list.items[item_index], text)?;
        let _ = fused_node_rect(&runtime.fused, node)?;
        damage = Some(match damage {
            Some(current) => union_rect(current, item_damage)?,
            None => item_damage,
        });
        updates.push((item_index, text.to_string()));
    }
    drop(dom);

    let damage = damage?;
    trace_runtime_text_repaint(dirty_nodes.len(), damage);
    let direct_item = (updates.len() == 1).then_some(updates[0].0);
    let mut direct_text = None;
    for (item_index, text) in updates {
        let text_paint =
            update_text_display_item_content(&mut runtime.display_list.items[item_index], &text)?;
        if Some(item_index) == direct_item {
            direct_text = Some((item_index, text, text_paint));
        }
    }
    if let Some((item_index, text, text_paint)) = direct_text
        && paint_text_damage_argb(
            &runtime.display_list.items,
            item_index,
            frame,
            damage,
            text_paint,
            &text,
        )
    {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

fn repaint_single_runtime_text_node(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    node: silksurf_dom::NodeId,
) -> Option<BrowserRedrawMode> {
    let (item_index, text, damage) = {
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = dirty_text_node_content(&dom, node)?.to_string();
        let item_index = dirty_text_display_item_index(&runtime.display_list.items, node)?;
        let damage =
            text_item_in_place_damage_rect(&runtime.display_list.items[item_index], &text)?;
        let _ = fused_node_rect(&runtime.fused, node)?;
        (item_index, text, damage)
    };

    trace_runtime_text_repaint(1, damage);
    let text_paint =
        update_text_display_item_content(&mut runtime.display_list.items[item_index], &text)?;
    if paint_text_damage_argb(
        &runtime.display_list.items,
        item_index,
        frame,
        damage,
        text_paint,
        &text,
    ) {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

fn trace_runtime_text_repaint(dirty_count: usize, damage: Rect) {
    if std::env::var_os("SILKSURF_TRACE_RUNTIME_TEXT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] Runtime text repaint: dirty_nodes={} damage=({}, {}, {}, {})",
        dirty_count, damage.x, damage.y, damage.width, damage.height
    );
}

fn dirty_text_node_content(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> Option<&str> {
    match dom.node(node).ok()?.kind() {
        silksurf_dom::NodeKind::Text { text } => Some(text.as_str()),
        _ => None,
    }
}

fn dirty_text_display_item_index(
    items: &[silksurf_render::DisplayItem],
    node: silksurf_dom::NodeId,
) -> Option<usize> {
    items.iter().position(|item| {
        matches!(
            item,
            silksurf_render::DisplayItem::Text {
                node: item_node,
                ..
            } if *item_node == node
        )
    })
}

fn text_item_in_place_damage_rect(
    item: &silksurf_render::DisplayItem,
    value: &str,
) -> Option<Rect> {
    let silksurf_render::DisplayItem::Text {
        rect, font_size, ..
    } = item
    else {
        return None;
    };
    if rect.width <= 0.0 || rect.height <= 0.0 || *font_size <= 0.0 || !font_size.is_finite() {
        return None;
    }
    let (width, height) = page_bitmap_text_bounds(value, *font_size)?;
    if width <= rect.width + 0.5 && height <= rect.height + 0.5 {
        Some(focused_input_text_damage_rect(item, value).unwrap_or(*rect))
    } else {
        None
    }
}

fn update_text_display_item_content(
    item: &mut silksurf_render::DisplayItem,
    value: &str,
) -> Option<TextItemPaint> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        text_len,
        font_size,
        color,
        ..
    } = item
    else {
        return None;
    };
    text.clear();
    text.push_str(value);
    *text_len = value.len() as u32;
    Some(TextItemPaint {
        rect: *rect,
        font_size: *font_size,
        color: *color,
    })
}

fn repaint_focused_input_value(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    node: silksurf_dom::NodeId,
    value: &str,
) -> Option<BrowserRedrawMode> {
    let control_damage = frame
        .input_targets
        .iter()
        .find(|target| target.node == node)?
        .rect;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text_index = runtime.display_list.items.iter().position(|item| {
        display_text_item_matches_input_node(&dom, item, node)
            || display_text_item_intersects_rect(item, control_damage)
    })?;
    drop(dom);
    let text_item = &runtime.display_list.items[text_index];
    let damage = focused_input_text_damage_rect(text_item, value).unwrap_or(control_damage);
    trace_focused_input_damage(node, damage);
    let text_paint =
        update_focused_input_text_item(&mut runtime.display_list.items[text_index], value)?;
    if paint_text_damage_argb(
        &runtime.display_list.items,
        text_index,
        frame,
        damage,
        text_paint,
        value,
    ) {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

fn update_focused_input_text_item(
    item: &mut silksurf_render::DisplayItem,
    value: &str,
) -> Option<TextItemPaint> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        text_len,
        font_size,
        color,
        ..
    } = item
    else {
        return None;
    };
    text.clear();
    text.push_str(value);
    *text_len = value.len() as u32;
    Some(TextItemPaint {
        rect: *rect,
        font_size: *font_size,
        color: *color,
    })
}

fn paint_text_damage_argb(
    items: &[silksurf_render::DisplayItem],
    text_index: usize,
    frame: &mut BrowserFrame,
    damage: Rect,
    text_paint: TextItemPaint,
    value: &str,
) -> bool {
    if text_paint.color.a != 255 || !page_bitmap_text_supported(value, text_paint.font_size) {
        return false;
    }
    let viewport_damage = viewport_damage_rect(damage, frame.bitmap_scroll_y);
    let Some(pixel_rect) = pixel_rect_from_rect(viewport_damage, FRAME_WIDTH, frame.bitmap_height)
    else {
        return false;
    };
    let Some(background) = text_damage_background_argb(items, text_index, damage) else {
        return false;
    };
    let required = FRAME_WIDTH as usize * frame.bitmap_height as usize;
    if frame.argb.len() < required {
        return false;
    }
    fill_argb_rect(
        &mut frame.argb,
        FRAME_WIDTH,
        frame.bitmap_height,
        pixel_rect.x,
        pixel_rect.y,
        pixel_rect.width,
        pixel_rect.height,
        background,
    );
    draw_page_bitmap_text_clipped(
        &mut frame.argb,
        FRAME_WIDTH,
        frame.bitmap_height,
        text_paint.rect.x,
        text_paint.rect.y - frame.bitmap_scroll_y as f32,
        value,
        text_paint.font_size,
        css_color_to_argb(text_paint.color),
        pixel_rect,
    )
}

fn page_bitmap_text_supported(text: &str, font_size: f32) -> bool {
    page_bitmap_text_bounds(text, font_size).is_some()
}

fn page_bitmap_text_bounds(text: &str, font_size: f32) -> Option<(f32, f32)> {
    let (_, advance, line_height, space_advance) = page_bitmap_text_metrics(font_size)?;
    if text.is_empty() {
        return Some((0.0, 0.0));
    }
    let mut current_width = 0_i32;
    let mut widest_width = 0_i32;
    let mut line_count = 1_i32;
    for ch in text.chars() {
        match ch {
            '\n' => {
                widest_width = widest_width.max(current_width);
                current_width = 0;
                line_count = line_count.saturating_add(1);
            }
            '\r' => {}
            '\t' => current_width = current_width.saturating_add(space_advance.saturating_mul(4)),
            ' ' => current_width = current_width.saturating_add(space_advance),
            _ => {
                if !ch.is_ascii() || chrome_glyph_byte(ch as u8).is_none() {
                    return None;
                }
                current_width = current_width.saturating_add(advance);
            }
        }
    }
    widest_width = widest_width.max(current_width);
    Some((
        widest_width.max(0) as f32,
        line_count.max(1).saturating_mul(line_height).max(0) as f32,
    ))
}

fn text_damage_background_argb(
    items: &[silksurf_render::DisplayItem],
    text_index: usize,
    damage: Rect,
) -> Option<u32> {
    for item in items.iter().take(text_index).rev() {
        if !display_item_intersects_viewport(item, damage) {
            continue;
        }
        match item {
            silksurf_render::DisplayItem::SolidColor { color, .. }
            | silksurf_render::DisplayItem::RoundedRect { color, .. } => {
                if color.a != 255 {
                    return None;
                }
                return Some(css_color_to_argb(*color));
            }
            silksurf_render::DisplayItem::LinearGradient { .. }
            | silksurf_render::DisplayItem::Image { .. } => return None,
            silksurf_render::DisplayItem::Text { .. }
            | silksurf_render::DisplayItem::BoxShadow { .. } => {}
        }
    }
    Some(argb(255, 255, 255, 255))
}

fn pixel_rect_from_rect(rect: Rect, width: u32, height: u32) -> Option<PixelRect> {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let x0 = rect.x.floor().max(0.0).min(width as f32) as u32;
    let y0 = rect.y.floor().max(0.0).min(height as f32) as u32;
    let x1 = (rect.x + rect.width).ceil().max(0.0).min(width as f32) as u32;
    let y1 = (rect.y + rect.height).ceil().max(0.0).min(height as f32) as u32;
    (x1 > x0 && y1 > y0).then_some(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn pixel_rect_intersection(a: PixelRect, b: PixelRect) -> Option<PixelRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let y1 =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    (x1 > x0 && y1 > y0).then_some(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn trace_focused_input_damage(node: silksurf_dom::NodeId, damage: Rect) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input repaint: node={} damage=({}, {}, {}, {})",
        node.raw(),
        damage.x,
        damage.y,
        damage.width,
        damage.height
    );
}

fn display_text_item_matches_input_node(
    dom: &silksurf_dom::Dom,
    item: &silksurf_render::DisplayItem,
    node: silksurf_dom::NodeId,
) -> bool {
    let silksurf_render::DisplayItem::Text {
        node: item_node, ..
    } = item
    else {
        return false;
    };
    let mut current = Some(*item_node);
    while let Some(current_node) = current {
        if current_node == node {
            return true;
        }
        current = dom.parent(current_node).ok().flatten();
    }
    false
}

fn display_text_item_intersects_rect(item: &silksurf_render::DisplayItem, rect: Rect) -> bool {
    let silksurf_render::DisplayItem::Text {
        rect: item_rect, ..
    } = item
    else {
        return false;
    };
    rects_intersect(*item_rect, rect)
}

fn focused_input_text_damage_rect(
    item: &silksurf_render::DisplayItem,
    new_value: &str,
) -> Option<Rect> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        font_size,
        ..
    } = item
    else {
        return None;
    };
    let common_prefix_bytes = common_prefix_byte_len(text, new_value);
    let char_width = (*font_size * 0.65).max(1.0);
    let line_height = (*font_size * 1.35 + 4.0).max(1.0);
    let (start_line, start_column) = text_position(&text[..common_prefix_bytes]);
    let old_suffix = &text[common_prefix_bytes..];
    let new_suffix = &new_value[common_prefix_bytes..];
    let old_columns = trailing_line_column_count(old_suffix);
    let new_columns = trailing_line_column_count(new_suffix);
    let dirty_lines = suffix_line_span(old_suffix).max(suffix_line_span(new_suffix));
    let y = rect.y + start_line as f32 * line_height;
    let x = if dirty_lines == 1 {
        (rect.x + start_column as f32 * char_width - 2.0).max(rect.x)
    } else {
        rect.x
    };
    let dirty_columns = old_columns.max(new_columns).max(1) as f32;
    let width = if dirty_lines == 1 {
        (dirty_columns * char_width + 4.0).min(rect.x + rect.width - x)
    } else {
        rect.x + rect.width - x
    };
    if y >= rect.y + rect.height {
        return None;
    }
    let height = (dirty_lines as f32 * line_height).min(rect.y + rect.height - y);
    (width > 0.0).then_some(Rect {
        x,
        y,
        width,
        height: height.max(1.0),
    })
}

fn focused_empty_insert_damage(
    frame: &BrowserFrame,
    node: silksurf_dom::NodeId,
    old_value: &str,
    new_value: &str,
) -> Option<Rect> {
    if !old_value.is_empty() || new_value.is_empty() {
        return None;
    }
    let rect = frame
        .input_targets
        .iter()
        .find(|target| target.node == node)?
        .rect;
    let columns = new_value
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count()
        .max(1) as f32;
    let width = (columns * 10.0 + 8.0).min(rect.width);
    (width > 0.0 && rect.height > 0.0).then_some(Rect {
        x: rect.x,
        y: rect.y,
        width,
        height: rect.height,
    })
}

fn common_prefix_byte_len(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.char_indices())
        .take_while(|((_, a_ch), (_, b_ch))| a_ch == b_ch)
        .last()
        .map_or(0, |((idx, ch), _)| idx + ch.len_utf8())
}

fn text_position(text: &str) -> (usize, usize) {
    let mut line = 0usize;
    let mut column = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    (line, column)
}

fn trailing_line_column_count(text: &str) -> usize {
    text.rsplit('\n')
        .next()
        .map_or(0, |line| line.chars().count())
}

fn suffix_line_span(text: &str) -> usize {
    text.chars().filter(|ch| *ch == '\n').count() + 1
}

fn start_navigation_worker(
    state: &mut BrowserState,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    request: BrowserNavigationRequest,
    history_action: PendingHistoryAction,
    wake_handle: &silksurf_gui::WinitWakeHandle,
    render_config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> bool {
    if state.navigation_pending {
        return false;
    }
    state.navigation_generation = state.navigation_generation.saturating_add(1);
    let generation = state.navigation_generation;
    state.navigation_pending = true;
    state.pending_history = Some(history_action);
    let navigation_start_retained_ready = state.frame.navigation_start_retained_sent;
    set_browser_status(state, "loading");
    mark_redraw(state, BrowserRedrawMode::NavigationStartChrome);
    if navigation_start_retained_ready {
        let damage =
            browser_navigation_start_present_damage(FRAME_WIDTH, state.frame.bitmap_height);
        if damage != silksurf_gui::WinitPresentDamage::Clean {
            state.retained_present = Some(BrowserRetainedPresent {
                tag: NAVIGATION_START_RETAINED_TAG,
                damage,
            });
        }
    }
    let (tx, rx) = mpsc::channel();
    *navigation_rx.borrow_mut() = Some(rx);
    let wake_handle = wake_handle.clone();
    let render_config = render_config.clone();
    let image_cache = Arc::clone(image_cache);
    thread::spawn(move || {
        let result = load_navigation_payload(&request, &render_config, &image_cache);
        let _ = tx.send((generation, result));
        wake_handle.wake();
    });
    true
}

fn render_browser_window_frame(
    state_ref: &Rc<RefCell<BrowserState>>,
    scroll_ref: &Cell<f32>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    chrome_height: u32,
    trace_app_frame: bool,
    window_width: u32,
    window_height: u32,
    buffer_age: u8,
    pixels: &mut [u32],
) -> silksurf_gui::WinitPresentDamage {
    let mut state = state_ref.borrow_mut();
    let max_scroll =
        max_browser_scroll_offset(state.frame.raster_height, window_height, chrome_height);
    let scroll = clamp_scroll_offset(scroll_ref.get(), max_scroll);
    scroll_ref.set(scroll);
    prepare_browser_bitmap_for_window(
        &mut state,
        last_width,
        last_height,
        chrome_height,
        window_width,
        window_height,
        scroll,
        trace_app_frame,
    );
    let render_mode = state.redraw_mode;
    let seed_full_buffer = browser_render_seeds_full_buffer(render_mode, buffer_age);
    let blit_start = std::time::Instant::now();
    blit_browser_window_frame(
        &state,
        seed_full_buffer,
        render_mode,
        chrome_height,
        window_width,
        window_height,
        pixels,
    );
    let blit_elapsed = blit_start.elapsed();
    let chrome_start = std::time::Instant::now();
    draw_browser_window_chrome(
        &state,
        seed_full_buffer,
        render_mode,
        window_width,
        window_height,
        pixels,
    );
    let chrome_elapsed = chrome_start.elapsed();
    trace_browser_window_frame(
        trace_app_frame,
        window_width,
        window_height,
        buffer_age,
        render_mode,
        seed_full_buffer,
        blit_elapsed,
        chrome_elapsed,
    );
    if render_mode != BrowserRedrawMode::Clean {
        last_width.set(window_width);
        last_height.set(window_height);
    }
    state.retained_present = None;
    state.redraw_mode = BrowserRedrawMode::Clean;
    browser_present_damage(
        render_mode,
        state.frame.raster_height,
        chrome_height,
        scroll.round() as u32,
        window_width,
        window_height,
    )
}

fn prepare_browser_bitmap_for_window(
    state: &mut BrowserState,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
    scroll: f32,
    trace_app_frame: bool,
) {
    let exposes_unpainted_area = window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    );
    let refresh_start = std::time::Instant::now();
    let bitmap_refresh = refresh_browser_frame_bitmap(
        state,
        scroll.round() as u32,
        window_height.max(chrome_height),
    );
    trace_browser_bitmap_refresh(trace_app_frame, bitmap_refresh, refresh_start.elapsed());
    if exposes_unpainted_area || bitmap_refresh == BrowserBitmapRefresh::Full {
        last_width.set(window_width);
        last_height.set(window_height);
        state.redraw_mode = BrowserRedrawMode::Full;
    } else if let BrowserBitmapRefresh::ScrollReuse(damage) = bitmap_refresh {
        mark_redraw(state, BrowserRedrawMode::Damage(damage));
    }
}

fn blit_browser_window_frame(
    state: &BrowserState,
    seed_full_buffer: bool,
    render_mode: BrowserRedrawMode,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    if seed_full_buffer || render_mode == BrowserRedrawMode::Full {
        blit_browser_frame(
            &state.frame.argb,
            FRAME_WIDTH,
            state.frame.bitmap_height,
            chrome_height,
            0,
            window_width,
            window_height,
            pixels,
        );
        return;
    }
    if let BrowserRedrawMode::Damage(damage) | BrowserRedrawMode::DamageWithChrome(damage) =
        render_mode
    {
        blit_browser_frame_damage(
            &state.frame.argb,
            FRAME_WIDTH,
            state.frame.bitmap_height,
            chrome_height,
            state.frame.bitmap_scroll_y,
            window_width,
            window_height,
            damage,
            pixels,
        );
    }
}

fn draw_browser_window_chrome(
    state: &BrowserState,
    seed_full_buffer: bool,
    render_mode: BrowserRedrawMode,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    match (seed_full_buffer, render_mode) {
        (_, BrowserRedrawMode::Clean | BrowserRedrawMode::Scroll) => {}
        (false, BrowserRedrawMode::AddressFocusChrome) => {
            draw_browser_address_focus_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressFullTextChrome) => {
            draw_browser_address_full_text_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressChrome) => {
            draw_browser_address_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressTextChrome) => {
            draw_browser_address_text_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::NavigationStartChrome) => {
            draw_browser_navigation_start_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::StatusChrome) => {
            draw_browser_status_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::Damage(_) | BrowserRedrawMode::PageInputFocus(_)) => {}
        (true, _)
        | (false, BrowserRedrawMode::Full)
        | (false, BrowserRedrawMode::DamageWithChrome(_))
        | (false, BrowserRedrawMode::Chrome) => {
            draw_browser_chrome_overlays(state, pixels, window_width, window_height);
        }
    }
}

fn trace_browser_window_frame(
    enabled: bool,
    window_width: u32,
    window_height: u32,
    buffer_age: u8,
    render_mode: BrowserRedrawMode,
    seed_full_buffer: bool,
    blit_elapsed: std::time::Duration,
    chrome_elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] app frame: {window_width}x{window_height} age {buffer_age} mode {render_mode:?}, seed_full {seed_full_buffer}, blit {blit_elapsed:?}, chrome {chrome_elapsed:?}"
        );
    }
}

fn browser_render_ready(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    window_width: u32,
    window_height: u32,
) -> bool {
    window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    ) || state.borrow().redraw_mode != BrowserRedrawMode::Clean
}

fn browser_render_action(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitRenderAction {
    if window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    ) {
        return silksurf_gui::WinitRenderAction::Render;
    }
    let Some(retained) = state.borrow().retained_present else {
        return silksurf_gui::WinitRenderAction::Render;
    };
    silksurf_gui::WinitRenderAction::Retained {
        tag: retained.tag,
        damage: retained.damage,
    }
}

fn browser_retained_buffer_update(
    state: &Rc<RefCell<BrowserState>>,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    let mut state = state.borrow_mut();
    if let Some(update) = take_focus_retained_buffer_update(&mut state, window_width, window_height)
    {
        return Some(update);
    }
    if state.focused_input.is_some() {
        if let Some(update) =
            take_navigation_start_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
        if let Some(update) =
            take_current_view_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
    } else {
        if let Some(update) =
            take_current_view_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
        if let Some(update) =
            take_navigation_start_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
    }
    if state.frame.focus_viewport_cache.is_some()
        || state.focused_input.is_some()
        || state.address_editing
    {
        state.frame.scroll_viewport_caches.clear();
        return None;
    }
    prepare_scroll_viewport_caches(&mut state, window_width, window_height);
    take_scroll_retained_buffer_update(&mut state, window_width, window_height)
}

fn take_focus_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.focus_viewport_retained_sent || window_width != FRAME_WIDTH {
        return None;
    }
    let cache = state.frame.focus_viewport_cache.as_ref()?;
    if cache.bitmap_height != window_height {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if cache.argb.len() < pixel_count {
        return None;
    }
    let pixels = cache.argb.clone();
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: FOCUS_VIEWPORT_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels,
    })
}

fn take_current_view_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.current_view_retained_sent
        || window_width != FRAME_WIDTH
        || state.frame.bitmap_height != window_height
    {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if state.frame.argb.len() < pixel_count {
        return None;
    }
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: CURRENT_VIEW_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels: state.frame.argb[..pixel_count].to_vec(),
    })
}

fn take_navigation_start_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.navigation_start_retained_sent
        || window_width != FRAME_WIDTH
        || state.frame.bitmap_height != window_height
    {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if state.frame.argb.len() < pixel_count {
        return None;
    }
    let mut pixels = state.frame.argb[..pixel_count].to_vec();
    draw_navigation_start_retained_chrome(&mut pixels, window_width, window_height);
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: NAVIGATION_START_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels,
    })
}

fn prepare_scroll_viewport_caches(state: &mut BrowserState, window_width: u32, window_height: u32) {
    if window_width != FRAME_WIDTH || state.frame.bitmap_height != window_height {
        state.frame.scroll_viewport_caches.clear();
        return;
    }
    let Some(runtime) = state.runtime.as_ref() else {
        state.frame.scroll_viewport_caches.clear();
        return;
    };
    let max_scroll = max_browser_scroll_offset(
        state.frame.raster_height,
        window_height,
        BROWSER_CHROME_HEIGHT as u32,
    );
    let targets = scroll_retained_targets(state.frame.bitmap_scroll_y, max_scroll);
    if targets.is_empty() {
        state.frame.scroll_viewport_caches.clear();
        return;
    }
    if scroll_viewport_caches_cover_targets(
        &state.frame.scroll_viewport_caches,
        &targets,
        window_height,
    ) {
        return;
    }

    let mut caches = Vec::with_capacity(targets.len());
    for scroll_y in targets {
        let mut rgba = Vec::new();
        let mut argb = Vec::new();
        rasterize_browser_viewport_argb_preferred(
            &runtime.display_list,
            scroll_y,
            window_height,
            &mut rgba,
            &mut argb,
        );
        caches.push(ScrollViewportCache {
            scroll_y,
            bitmap_height: window_height,
            tag: scroll_retained_tag_for_scroll_y(scroll_y),
            argb,
            retained_sent: false,
        });
    }
    state.frame.scroll_viewport_caches = caches;
}

fn take_scroll_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    let cache_index = state
        .frame
        .scroll_viewport_caches
        .iter()
        .position(|cache| {
            !cache.retained_sent
                && cache.bitmap_height == window_height
                && cache.argb.len() >= pixel_count
        })?;
    let cache = &mut state.frame.scroll_viewport_caches[cache_index];
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: cache.tag,
        width: window_width,
        height: window_height,
        pixels: cache.argb.clone(),
    })
}

fn scroll_retained_targets(current_scroll_y: u32, max_scroll: f32) -> Vec<u32> {
    let mut targets = Vec::with_capacity(2);
    for delta in [
        (BROWSER_WHEEL_LINE_PX * 2.0) as i32,
        -(BROWSER_WHEEL_LINE_PX as i32),
    ] {
        let target = clamp_scroll_offset(current_scroll_y as f32 + delta as f32, max_scroll);
        let target_scroll_y = target.round() as u32;
        if target_scroll_y != current_scroll_y && !targets.contains(&target_scroll_y) {
            targets.push(target_scroll_y);
        }
    }
    targets
}

fn scroll_viewport_caches_cover_targets(
    caches: &[ScrollViewportCache],
    targets: &[u32],
    bitmap_height: u32,
) -> bool {
    targets.iter().all(|target| {
        caches
            .iter()
            .any(|cache| cache.scroll_y == *target && cache.bitmap_height == bitmap_height)
    })
}

fn scroll_retained_tag_for_scroll_y(scroll_y: u32) -> silksurf_gui::WinitRetainedBufferTag {
    silksurf_gui::WinitRetainedBufferTag::new(
        SCROLL_VIEWPORT_RETAINED_TAG_BASE + u64::from(scroll_y),
    )
}

fn handle_browser_retained_buffer_prepared(
    state: &Rc<RefCell<BrowserState>>,
    tag: silksurf_gui::WinitRetainedBufferTag,
) {
    let mut state = state.borrow_mut();
    if tag == FOCUS_VIEWPORT_RETAINED_TAG {
        state.frame.focus_viewport_retained_sent = true;
        return;
    }
    if tag == CURRENT_VIEW_RETAINED_TAG {
        state.frame.current_view_retained_sent = true;
        return;
    }
    if tag == NAVIGATION_START_RETAINED_TAG {
        state.frame.navigation_start_retained_sent = true;
        return;
    }
    if let Some(cache) = state
        .frame
        .scroll_viewport_caches
        .iter_mut()
        .find(|cache| cache.tag == tag)
    {
        cache.retained_sent = true;
    }
}

fn surface_pixel_count(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)
}

fn handle_browser_presented_frame(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    frame: silksurf_gui::WinitPresentedFrame,
) {
    let Some(retained_tag) = frame.retained_tag else {
        return;
    };
    let mut state = state.borrow_mut();
    let retained_matches = state
        .retained_present
        .is_some_and(|retained| retained.tag == retained_tag);
    if !retained_matches {
        return;
    }
    if retained_tag == CURRENT_VIEW_RETAINED_TAG {
        state.frame.current_view_retained_sent = false;
    }
    state.retained_present = None;
    state.redraw_mode = BrowserRedrawMode::Clean;
    last_width.set(frame.width);
    last_height.set(frame.height);
}

fn handle_browser_wake(
    state_ref: &Rc<RefCell<BrowserState>>,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let result = navigation_rx
        .borrow_mut()
        .as_ref()
        .and_then(|rx| rx.try_recv().ok());
    let mut state = state_ref.borrow_mut();
    if let Some(result) = result {
        *navigation_rx.borrow_mut() = None;
        return apply_navigation_result(&mut state, result, scroll, live_window_height);
    }
    tick_browser_runtime(&mut state)
}

fn apply_navigation_result(
    state: &mut BrowserState,
    result: NavigationMessage,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let (generation, result) = result;
    if generation != state.navigation_generation {
        return false;
    }
    state.navigation_pending = false;
    match result {
        Ok(payload) => apply_navigation_payload(state, payload, scroll, live_window_height),
        Err(message) => {
            eprintln!("[SilkSurf] Navigation error: {message}");
            mark_navigation_error(state);
            true
        }
    }
}

fn apply_navigation_payload(
    state: &mut BrowserState,
    payload: BrowserPagePayload,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let render_config = payload.render_config.clone();
    let buffers = take_browser_frame_buffers(state);
    let live_window_height = (live_window_height > 0).then_some(live_window_height);
    match build_browser_page_with_buffers_for_height(payload, buffers, live_window_height) {
        Ok(page) => {
            eprintln!("[SilkSurf] Navigation complete: {}", page.frame.url);
            let modulepreload_urls = runtime_module_warm_urls(&page.runtime, &page.frame.url);
            let loaded_url = page.frame.url.clone();
            apply_history_success(state, loaded_url.as_str());
            state.frame = page.frame;
            state.runtime = Some(page.runtime);
            state.address_text = loaded_url;
            state.address_editing = false;
            state.address_select_all = false;
            clear_page_input_focus(state);
            set_browser_status(state, "ready");
            mark_redraw(state, BrowserRedrawMode::Full);
            scroll.set(0.0);
            preload_module_scripts(&modulepreload_urls, &render_config);
        }
        Err(err) => {
            let message = err.message;
            restore_browser_frame_buffers(state, err.buffers);
            eprintln!("[SilkSurf] Navigation render error: {message}");
            mark_navigation_error(state);
        }
    }
    true
}

fn runtime_module_warm_urls(runtime: &BrowserPageRuntime, base_url: &str) -> Vec<String> {
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    extract_module_warm_urls(&dom, runtime.document, base_url)
}

fn take_browser_frame_buffers(state: &mut BrowserState) -> BrowserFrameBuffers {
    BrowserFrameBuffers {
        rgba: state
            .runtime
            .as_mut()
            .map(|runtime| std::mem::take(&mut runtime.rgba))
            .unwrap_or_default(),
        argb: std::mem::take(&mut state.frame.argb),
    }
}

fn restore_browser_frame_buffers(state: &mut BrowserState, buffers: BrowserFrameBuffers) {
    state.frame.argb = buffers.argb;
    if let Some(runtime) = state.runtime.as_mut() {
        runtime.rgba = buffers.rgba;
    }
}

fn mark_navigation_error(state: &mut BrowserState) {
    state.pending_history = None;
    set_browser_status(state, "error");
    mark_redraw(state, BrowserRedrawMode::Chrome);
}

fn handle_browser_input(
    input: silksurf_gui::WinitInput,
    runtime: BrowserInputRuntime<'_>,
) -> silksurf_gui::WinitInputResult {
    let frame_height = runtime.state.borrow().frame.raster_height;
    let max_scroll =
        max_browser_scroll_offset(frame_height, runtime.window_height, runtime.chrome_height);
    let current = runtime.scroll.get();
    if let silksurf_gui::WinitInput::CursorMoved { x, y } = input {
        let mut state = runtime.state.borrow_mut();
        let cursor = browser_cursor_shape_for_state(&state, runtime.chrome_height, x, y, current);
        let redraw = update_hover_status(&mut state, runtime.chrome_height, x, y, current);
        return silksurf_gui::WinitInputResult {
            redraw,
            cursor: Some(cursor),
        };
    }
    if let Some(changed) = handle_address_caret_input(input, &runtime) {
        return changed.into();
    }
    if let Some(next) = browser_scroll_target(
        input,
        current,
        runtime.window_height,
        runtime.chrome_height,
        max_scroll,
    ) {
        return apply_browser_scroll(&runtime, current, next, max_scroll).into();
    }
    handle_browser_command_input(input, &runtime, current).into()
}

fn browser_scroll_target(
    input: silksurf_gui::WinitInput,
    current: f32,
    window_height: u32,
    chrome_height: u32,
    max_scroll: f32,
) -> Option<f32> {
    let page_delta = ((window_height.saturating_sub(chrome_height)) as f32
        * BROWSER_PAGE_SCROLL_FACTOR)
        .max(BROWSER_WHEEL_LINE_PX);
    match input {
        silksurf_gui::WinitInput::ScrollPixels(delta) => Some(current + delta),
        silksurf_gui::WinitInput::PageDown => Some(current + page_delta),
        silksurf_gui::WinitInput::PageUp => Some(current - page_delta),
        silksurf_gui::WinitInput::Home => Some(0.0),
        silksurf_gui::WinitInput::End => Some(max_scroll),
        _ => None,
    }
}

fn apply_browser_scroll(
    runtime: &BrowserInputRuntime<'_>,
    current: f32,
    next: f32,
    max_scroll: f32,
) -> bool {
    let scroll = clamp_scroll_offset(next, max_scroll);
    if (scroll - current).abs() < 0.5 {
        return false;
    }
    let bitmap_height = runtime.window_height.max(runtime.chrome_height);
    let scroll_y = scroll.round() as u32;
    let mut state = runtime.state.borrow_mut();
    runtime.scroll.set(scroll);
    if let Some(retained) = apply_scroll_viewport_cache(
        &mut state,
        scroll_y,
        bitmap_height,
        runtime.chrome_height,
        runtime.window_width,
        runtime.window_height,
    ) {
        mark_redraw(
            &mut state,
            scroll_viewport_cache_redraw_mode(scroll_y, bitmap_height),
        );
        state.retained_present = Some(retained);
    } else {
        mark_redraw(&mut state, BrowserRedrawMode::Scroll);
    }
    true
}

fn handle_address_caret_input(
    input: silksurf_gui::WinitInput,
    runtime: &BrowserInputRuntime<'_>,
) -> Option<bool> {
    let motion = match input {
        silksurf_gui::WinitInput::MoveCaretLeft => AddressCaretMotion::Backward,
        silksurf_gui::WinitInput::MoveCaretRight => AddressCaretMotion::Forward,
        silksurf_gui::WinitInput::Home => AddressCaretMotion::Start,
        silksurf_gui::WinitInput::End => AddressCaretMotion::End,
        _ => return None,
    };
    let mut state = runtime.state.borrow_mut();
    if !state.address_editing {
        return None;
    }
    if move_address_caret(&mut state, motion) {
        mark_redraw(&mut state, BrowserRedrawMode::AddressFullTextChrome);
        return Some(true);
    }
    Some(false)
}

fn handle_browser_command_input(
    input: silksurf_gui::WinitInput,
    runtime: &BrowserInputRuntime<'_>,
    current_scroll: f32,
) -> bool {
    match input {
        silksurf_gui::WinitInput::PrimaryClick { x, y } => {
            handle_browser_primary_click(runtime, x, y, current_scroll)
        }
        silksurf_gui::WinitInput::FocusAddress => focus_address_input(runtime),
        silksurf_gui::WinitInput::TextInput(ch) => handle_text_input(runtime, ch),
        silksurf_gui::WinitInput::SubmitAddress => submit_address_input(runtime),
        silksurf_gui::WinitInput::Backspace => handle_backspace_input(runtime),
        silksurf_gui::WinitInput::Copy => copy_address_input(runtime),
        silksurf_gui::WinitInput::Paste => paste_clipboard_into_address(runtime),
        silksurf_gui::WinitInput::Cut => cut_address_input(runtime),
        silksurf_gui::WinitInput::FocusNextPageInput => focus_next_page_input_from_runtime(runtime),
        silksurf_gui::WinitInput::MoveCaretLeft
        | silksurf_gui::WinitInput::MoveCaretRight
        | silksurf_gui::WinitInput::Home
        | silksurf_gui::WinitInput::End => false,
        silksurf_gui::WinitInput::Back => navigate_history_back(runtime),
        silksurf_gui::WinitInput::Forward => navigate_history_forward(runtime),
        silksurf_gui::WinitInput::Reload => reload_current_page(runtime),
        silksurf_gui::WinitInput::Stop => stop_navigation(&mut runtime.state.borrow_mut()),
        _ => false,
    }
}

fn browser_cursor_shape_for_state(
    state: &BrowserState,
    chrome_height: u32,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> silksurf_gui::WinitCursorShape {
    if browser_address_bar_contains(x, y)
        || hit_test_input(
            &state.frame.input_targets,
            x,
            y,
            current_scroll,
            chrome_height,
        )
        .is_some()
    {
        return silksurf_gui::WinitCursorShape::Text;
    }
    if hit_test_chrome_action(x, y).is_some_and(|action| chrome_action_enabled(state, action)) {
        return silksurf_gui::WinitCursorShape::Pointer;
    }
    if hit_test_link(
        &state.frame.link_targets,
        x,
        y,
        current_scroll,
        chrome_height,
    )
    .is_some()
    {
        return silksurf_gui::WinitCursorShape::Pointer;
    }
    silksurf_gui::WinitCursorShape::Default
}

fn update_hover_status(
    state: &mut BrowserState,
    chrome_height: u32,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> bool {
    trace_link_hit_test(state, x, y, current_scroll);
    let next = (!state.navigation_pending)
        .then(|| {
            hit_test_link(
                &state.frame.link_targets,
                x,
                y,
                current_scroll,
                chrome_height,
            )
            .map(str::to_string)
        })
        .flatten();
    if state.hover_status_text == next {
        return false;
    }
    state.hover_status_text = next;
    mark_redraw(state, BrowserRedrawMode::StatusChrome);
    true
}

fn trace_link_hit_test(state: &BrowserState, x: f32, y: f32, scroll_y: f32) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] link hit-test: cursor=({x:.1},{y:.1}) scroll={scroll_y:.1} links={}",
        state.frame.link_targets.len()
    );
    for target in &state.frame.link_targets {
        eprintln!(
            "[SilkSurf] link target: href={} rect=({}, {}, {}, {})",
            target.href, target.rect.x, target.rect.y, target.rect.width, target.rect.height
        );
    }
}

fn handle_browser_primary_click(
    runtime: &BrowserInputRuntime<'_>,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> bool {
    if let Some(action) = hit_test_chrome_action(x, y) {
        return handle_chrome_click(runtime, action);
    }
    if browser_address_bar_contains(x, y) {
        return focus_address_input(runtime);
    }
    let mut redraw_requested = blur_address_input(runtime);
    if let Some(input_node) = hit_test_page_input(runtime, x, y, current_scroll) {
        let mut state = runtime.state.borrow_mut();
        if activate_page_input_control(&mut state, input_node) {
            return true;
        }
        redraw_requested |= focus_page_input(&mut state, input_node);
        return redraw_requested;
    }
    redraw_requested | follow_hit_link(runtime, x, y, current_scroll)
}

fn handle_chrome_click(runtime: &BrowserInputRuntime<'_>, action: BrowserChromeAction) -> bool {
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    handle_chrome_action(
        &mut state,
        runtime.navigation_rx,
        action,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn blur_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    if !state.address_editing {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);
    true
}

fn hit_test_page_input(
    runtime: &BrowserInputRuntime<'_>,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> Option<silksurf_dom::NodeId> {
    let state = runtime.state.borrow();
    let input_node = hit_test_input(
        &state.frame.input_targets,
        x,
        y,
        current_scroll,
        runtime.chrome_height,
    );
    trace_input_hit_test(&state, x, y, current_scroll);
    input_node
}

fn follow_hit_link(runtime: &BrowserInputRuntime<'_>, x: f32, y: f32, current_scroll: f32) -> bool {
    {
        let mut state = runtime.state.borrow_mut();
        clear_page_input_focus(&mut state);
    }
    let href = {
        let state = runtime.state.borrow();
        hit_test_link(
            &state.frame.link_targets,
            x,
            y,
            current_scroll,
            runtime.chrome_height,
        )
        .map(str::to_string)
    };
    let Some(href) = href else {
        return false;
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    if state.navigation_pending {
        return false;
    }
    eprintln!("[SilkSurf] Navigating: {href}");
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(href),
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn focus_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    let mode = address_focus_redraw_mode(&state);
    let changed = focus_address_bar(&mut state);
    if changed {
        mark_redraw(&mut state, mode);
    }
    changed
}

fn mark_address_edit_redraw(state: &mut BrowserState, full_address_damage: bool) {
    let mode = if full_address_damage {
        BrowserRedrawMode::AddressFullTextChrome
    } else {
        BrowserRedrawMode::AddressTextChrome
    };
    mark_redraw(state, mode);
}

fn handle_text_input(runtime: &BrowserInputRuntime<'_>, ch: char) -> bool {
    let mut state = runtime.state.borrow_mut();
    let full_address_damage = state.address_select_all;
    if push_address_char(&mut state, ch) {
        mark_address_edit_redraw(&mut state, full_address_damage);
        return true;
    }
    push_focused_input_char(&mut state, ch)
}

fn submit_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target_url = {
        let state = runtime.state.borrow();
        state
            .address_editing
            .then(|| normalize_address_input(&state.address_text))
            .flatten()
    };
    match target_url {
        Some(target_url) => navigate_address_target(runtime, target_url),
        None => {
            let mut state = runtime.state.borrow_mut();
            if state.address_editing {
                set_browser_status(&mut state, "error");
                mark_redraw(&mut state, BrowserRedrawMode::Chrome);
                return true;
            }
            if push_focused_textarea_newline(&mut state) {
                return true;
            }
            drop(state);
            submit_focused_form(runtime)
        }
    }
}

fn submit_focused_form(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target = focused_form_submission_target(&runtime.state.borrow());
    match target {
        Some(FormSubmissionTarget::Get(target_url)) => navigate_address_target(runtime, target_url),
        Some(FormSubmissionTarget::Post(request)) => navigate_form_request(runtime, request),
        Some(FormSubmissionTarget::UnsupportedMethod(method)) => {
            let mut state = runtime.state.borrow_mut();
            set_browser_status(&mut state, format!("unsupported form method {method}"));
            mark_redraw(&mut state, BrowserRedrawMode::Chrome);
            true
        }
        None => false,
    }
}

fn navigate_address_target(runtime: &BrowserInputRuntime<'_>, target_url: String) -> bool {
    let mut state = runtime.state.borrow_mut();
    if state.navigation_pending {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    clear_page_input_focus(&mut state);
    state.address_text = target_url.clone();
    eprintln!("[SilkSurf] Navigating: {target_url}");
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(target_url),
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn navigate_form_request(
    runtime: &BrowserInputRuntime<'_>,
    request: BrowserNavigationRequest,
) -> bool {
    let mut state = runtime.state.borrow_mut();
    if state.navigation_pending {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    clear_page_input_focus(&mut state);
    state.address_text = request.url.clone();
    eprintln!("[SilkSurf] Navigating: {}", request.url);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        request,
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn handle_backspace_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    if edit_address_backspace(&mut state) {
        mark_address_edit_redraw(&mut state, true);
        return true;
    }
    if edit_focused_input_backspace(&mut state) {
        return true;
    }
    drop(state);
    navigate_history_back(runtime)
}

fn copy_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let state = runtime.state.borrow();
    if let Some(text) = address_clipboard_text(&state)
        && let Err(err) = write_clipboard_text(text)
    {
        eprintln!("[SilkSurf] Clipboard copy failed: {err}");
    }
    false
}

fn paste_clipboard_into_address(runtime: &BrowserInputRuntime<'_>) -> bool {
    let text = match read_clipboard_text() {
        Ok(text) => text,
        Err(err) => {
            eprintln!("[SilkSurf] Clipboard paste failed: {err}");
            return false;
        }
    };
    let mut state = runtime.state.borrow_mut();
    let full_address_damage = state.address_select_all;
    if !paste_address_text(&mut state, text.as_str()) {
        return false;
    }
    mark_address_edit_redraw(&mut state, full_address_damage);
    true
}

fn cut_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    let copied = address_clipboard_text(&state)
        .map(write_clipboard_text)
        .transpose();
    if let Err(err) = copied {
        eprintln!("[SilkSurf] Clipboard cut failed: {err}");
    }
    if !cut_address_text(&mut state) {
        return false;
    }
    mark_address_edit_redraw(&mut state, true);
    true
}

fn focus_next_page_input_from_runtime(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    let changed = focus_next_visible_page_input(
        &mut state,
        runtime.scroll.get(),
        runtime.chrome_height,
        runtime.window_height,
    );
    if !changed {
        return false;
    }
    let Some(focused) = state.focused_input else {
        return true;
    };
    let Some(target_rect) = state
        .frame
        .input_targets
        .iter()
        .find(|target| target.node == focused)
        .map(|target| target.rect)
    else {
        return true;
    };
    let max_scroll = max_browser_scroll_offset(
        state.frame.raster_height,
        runtime.window_height,
        runtime.chrome_height,
    );
    let next_scroll = scroll_to_show_input_target(
        runtime.scroll.get(),
        target_rect,
        max_scroll,
        runtime.chrome_height,
        runtime.window_height,
    );
    if (next_scroll - runtime.scroll.get()).abs() >= 0.5 {
        runtime.scroll.set(next_scroll);
        let bitmap_height = runtime.window_height.max(runtime.chrome_height);
        let scroll_y = next_scroll.round() as u32;
        if apply_focus_viewport_cache(&mut state, scroll_y, bitmap_height) {
            let redraw_mode = focus_viewport_cache_redraw_mode(scroll_y, bitmap_height);
            mark_redraw(&mut state, redraw_mode);
            state.retained_present = focus_viewport_retained_present(
                &state,
                redraw_mode,
                runtime.chrome_height,
                scroll_y,
                runtime.window_width,
                runtime.window_height,
            );
        } else {
            mark_redraw(&mut state, BrowserRedrawMode::Scroll);
        }
    }
    true
}

fn focus_viewport_cache_redraw_mode(scroll_y: u32, bitmap_height: u32) -> BrowserRedrawMode {
    BrowserRedrawMode::Damage(scroll_visible_document_rect(scroll_y, bitmap_height))
}

fn focus_viewport_retained_present(
    state: &BrowserState,
    redraw_mode: BrowserRedrawMode,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let damage = browser_present_damage(
        redraw_mode,
        state.frame.raster_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    );
    (damage != silksurf_gui::WinitPresentDamage::Clean).then_some(BrowserRetainedPresent {
        tag: FOCUS_VIEWPORT_RETAINED_TAG,
        damage,
    })
}

fn apply_focus_viewport_cache(state: &mut BrowserState, scroll_y: u32, bitmap_height: u32) -> bool {
    let cache_matches = state
        .frame
        .focus_viewport_cache
        .as_ref()
        .is_some_and(|cache| cache.scroll_y == scroll_y && cache.bitmap_height == bitmap_height);
    if !cache_matches {
        return false;
    }
    let Some(cache) = state.frame.focus_viewport_cache.take() else {
        return false;
    };
    state.frame.argb = cache.argb;
    state.frame.bitmap_scroll_y = cache.scroll_y;
    state.frame.bitmap_height = cache.bitmap_height;
    true
}

fn scroll_viewport_cache_redraw_mode(scroll_y: u32, bitmap_height: u32) -> BrowserRedrawMode {
    BrowserRedrawMode::Damage(scroll_visible_document_rect(scroll_y, bitmap_height))
}

fn apply_scroll_viewport_cache(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let cache_index = state
        .frame
        .scroll_viewport_caches
        .iter()
        .position(|cache| {
            cache.retained_sent
                && cache.scroll_y == scroll_y
                && cache.bitmap_height == bitmap_height
        })?;
    let cache = state.frame.scroll_viewport_caches.swap_remove(cache_index);
    state.frame.argb = cache.argb;
    state.frame.bitmap_scroll_y = cache.scroll_y;
    state.frame.bitmap_height = cache.bitmap_height;
    scroll_viewport_retained_present(
        state,
        cache.tag,
        chrome_height,
        scroll_y,
        bitmap_height,
        window_width,
        window_height,
    )
}

fn scroll_viewport_retained_present(
    state: &BrowserState,
    tag: silksurf_gui::WinitRetainedBufferTag,
    chrome_height: u32,
    scroll_y: u32,
    bitmap_height: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let redraw_mode = scroll_viewport_cache_redraw_mode(scroll_y, bitmap_height);
    let damage = browser_present_damage(
        redraw_mode,
        state.frame.raster_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    );
    (damage != silksurf_gui::WinitPresentDamage::Clean)
        .then_some(BrowserRetainedPresent { tag, damage })
}

fn navigate_history_back(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target = {
        let state = runtime.state.borrow();
        history_back_target(&state)
    };
    navigate_history_target(runtime, target)
}

fn navigate_history_forward(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target = {
        let state = runtime.state.borrow();
        history_forward_target(&state)
    };
    navigate_history_target(runtime, target)
}

fn navigate_history_target(
    runtime: &BrowserInputRuntime<'_>,
    target: Option<(usize, String)>,
) -> bool {
    let Some((target_index, target_url)) = target else {
        return false;
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(target_url),
        PendingHistoryAction::MoveTo(target_index),
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn reload_current_page(runtime: &BrowserInputRuntime<'_>) -> bool {
    let (target_url, history_index) = {
        let state = runtime.state.borrow();
        (state.frame.url.clone(), state.history_index)
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(target_url),
        PendingHistoryAction::MoveTo(history_index),
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

fn handle_chrome_action(
    state: &mut BrowserState,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    action: BrowserChromeAction,
    wake_handle: &silksurf_gui::WinitWakeHandle,
    render_config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> bool {
    if !chrome_action_enabled(state, action) {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    match action {
        BrowserChromeAction::Back => history_back_target(state).is_some_and(|(index, url)| {
            start_navigation_worker(
                state,
                navigation_rx,
                BrowserNavigationRequest::get(url),
                PendingHistoryAction::MoveTo(index),
                wake_handle,
                render_config,
                image_cache,
            )
        }),
        BrowserChromeAction::Forward => {
            history_forward_target(state).is_some_and(|(index, url)| {
                start_navigation_worker(
                    state,
                    navigation_rx,
                    BrowserNavigationRequest::get(url),
                    PendingHistoryAction::MoveTo(index),
                    wake_handle,
                    render_config,
                    image_cache,
                )
            })
        }
        BrowserChromeAction::Home => start_navigation_worker(
            state,
            navigation_rx,
            BrowserNavigationRequest::get(browser_home_url()),
            PendingHistoryAction::Push,
            wake_handle,
            render_config,
            image_cache,
        ),
        BrowserChromeAction::Reload => start_navigation_worker(
            state,
            navigation_rx,
            BrowserNavigationRequest::get(state.frame.url.clone()),
            PendingHistoryAction::MoveTo(state.history_index),
            wake_handle,
            render_config,
            image_cache,
        ),
        BrowserChromeAction::Stop => stop_navigation(state),
    }
}

fn chrome_action_enabled(state: &BrowserState, action: BrowserChromeAction) -> bool {
    match action {
        BrowserChromeAction::Back => {
            !state.navigation_pending && history_back_target(state).is_some()
        }
        BrowserChromeAction::Forward => {
            !state.navigation_pending && history_forward_target(state).is_some()
        }
        BrowserChromeAction::Home | BrowserChromeAction::Reload => !state.navigation_pending,
        BrowserChromeAction::Stop => state.navigation_pending,
    }
}

fn stop_navigation(state: &mut BrowserState) -> bool {
    if !state.navigation_pending {
        return false;
    }
    state.navigation_generation = state.navigation_generation.saturating_add(1);
    state.navigation_pending = false;
    state.pending_history = None;
    set_browser_status(state, "ready");
    mark_redraw(state, BrowserRedrawMode::Chrome);
    eprintln!("[SilkSurf] Navigation stopped");
    true
}

fn history_back_target(state: &BrowserState) -> Option<(usize, String)> {
    let target_index = state.history_index.checked_sub(1)?;
    let target_url = state.history.get(target_index)?.clone();
    Some((target_index, target_url))
}

fn history_forward_target(state: &BrowserState) -> Option<(usize, String)> {
    let target_index = state.history_index.checked_add(1)?;
    let target_url = state.history.get(target_index)?.clone();
    Some((target_index, target_url))
}

fn apply_history_success(state: &mut BrowserState, loaded_url: &str) {
    match state.pending_history.take() {
        Some(PendingHistoryAction::Push) => {
            let keep = state.history_index.saturating_add(1);
            state.history.truncate(keep);
            if state
                .history
                .last()
                .is_some_and(|current_url| current_url == loaded_url)
            {
                state.history_index = state.history.len().saturating_sub(1);
            } else {
                state.history.push(loaded_url.to_string());
                state.history_index = state.history.len().saturating_sub(1);
            }
        }
        Some(PendingHistoryAction::MoveTo(index)) => {
            if index < state.history.len() {
                state.history_index = index;
            }
        }
        None => {}
    }
}

fn mark_redraw(state: &mut BrowserState, mode: BrowserRedrawMode) {
    state.retained_present = None;
    if mode != BrowserRedrawMode::Clean {
        state.frame.navigation_start_retained_sent = false;
        if !matches!(mode, BrowserRedrawMode::PageInputFocus(_)) {
            state.frame.current_view_retained_sent = false;
        }
        state.frame.scroll_viewport_caches.clear();
    }
    state.redraw_mode = combine_redraw_mode(state.redraw_mode, mode);
}

fn combine_redraw_mode(current: BrowserRedrawMode, next: BrowserRedrawMode) -> BrowserRedrawMode {
    match (current, next) {
        (BrowserRedrawMode::Clean, mode) | (mode, BrowserRedrawMode::Clean) => mode,
        (BrowserRedrawMode::Scroll, mode) | (mode, BrowserRedrawMode::Scroll) => mode,
        (BrowserRedrawMode::Full, _) | (_, BrowserRedrawMode::Full) => BrowserRedrawMode::Full,
        (BrowserRedrawMode::Damage(a), BrowserRedrawMode::Damage(b)) => {
            union_rect(a, b).map_or(BrowserRedrawMode::Chrome, BrowserRedrawMode::Damage)
        }
        (BrowserRedrawMode::PageInputFocus(a), BrowserRedrawMode::PageInputFocus(b))
        | (BrowserRedrawMode::PageInputFocus(a), BrowserRedrawMode::Damage(b))
        | (BrowserRedrawMode::Damage(a), BrowserRedrawMode::PageInputFocus(b)) => {
            union_rect(a, b).map_or(BrowserRedrawMode::Chrome, BrowserRedrawMode::Damage)
        }
        (BrowserRedrawMode::DamageWithChrome(a), BrowserRedrawMode::Damage(b))
        | (BrowserRedrawMode::Damage(a), BrowserRedrawMode::DamageWithChrome(b))
        | (BrowserRedrawMode::DamageWithChrome(a), BrowserRedrawMode::PageInputFocus(b))
        | (BrowserRedrawMode::PageInputFocus(a), BrowserRedrawMode::DamageWithChrome(b))
        | (BrowserRedrawMode::DamageWithChrome(a), BrowserRedrawMode::DamageWithChrome(b)) => {
            union_rect(a, b).map_or(
                BrowserRedrawMode::Chrome,
                BrowserRedrawMode::DamageWithChrome,
            )
        }
        (BrowserRedrawMode::Chrome, BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::NavigationStartChrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::StatusChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::Chrome) => BrowserRedrawMode::Chrome,
        (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::AddressChrome) => {
            BrowserRedrawMode::AddressChrome
        }
        (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::AddressFocusChrome) => {
            BrowserRedrawMode::AddressFullTextChrome
        }
        (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::StatusChrome)
        | (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::Damage(damage), BrowserRedrawMode::NavigationStartChrome)
        | (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::Damage(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::StatusChrome)
        | (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::PageInputFocus(damage))
        | (BrowserRedrawMode::PageInputFocus(damage), BrowserRedrawMode::NavigationStartChrome)
        | (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::PageInputFocus(damage)) => {
            BrowserRedrawMode::DamageWithChrome(damage)
        }
        (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::Chrome)
        | (BrowserRedrawMode::Chrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::AddressChrome)
        | (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::AddressFocusChrome)
        | (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::StatusChrome)
        | (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::DamageWithChrome(damage))
        | (BrowserRedrawMode::DamageWithChrome(damage), BrowserRedrawMode::NavigationStartChrome)
        | (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::DamageWithChrome(damage)) => {
            BrowserRedrawMode::DamageWithChrome(damage)
        }
        (BrowserRedrawMode::Chrome, BrowserRedrawMode::Chrome) => BrowserRedrawMode::Chrome,
        (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::NavigationStartChrome) => {
            BrowserRedrawMode::NavigationStartChrome
        }
        (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::StatusChrome) => {
            BrowserRedrawMode::StatusChrome
        }
        (BrowserRedrawMode::StatusChrome, _) | (_, BrowserRedrawMode::StatusChrome) => {
            BrowserRedrawMode::Chrome
        }
        (BrowserRedrawMode::NavigationStartChrome, _)
        | (_, BrowserRedrawMode::NavigationStartChrome) => BrowserRedrawMode::Chrome,
        (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::AddressChrome) => {
            BrowserRedrawMode::AddressChrome
        }
        (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::AddressFocusChrome) => {
            BrowserRedrawMode::AddressFocusChrome
        }
        (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::AddressFullTextChrome)
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::AddressTextChrome)
        | (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::AddressFullTextChrome) => {
            BrowserRedrawMode::AddressFullTextChrome
        }
        (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::AddressTextChrome) => {
            BrowserRedrawMode::AddressTextChrome
        }
    }
}

fn text_only_diff_damage_rect(
    diff: &DomDiff,
    old_fused: &FusedResult,
    new_fused: &FusedResult,
) -> Option<Rect> {
    if !diff.added.is_empty()
        || !diff.removed.is_empty()
        || diff.changed.is_empty()
        || diff
            .changed
            .iter()
            .any(|(_, kind)| *kind != ChangeKind::TextContent)
    {
        return None;
    }

    let mut damage = None;
    for &(node, _) in &diff.changed {
        let old_rect = fused_node_rect(old_fused, node)?;
        let new_rect = fused_node_rect(new_fused, silksurf_dom::NodeId::from_raw(node.raw()))?;
        damage = Some(match damage {
            Some(current) => union_rect(current, old_rect)
                .map_or(new_rect, |rect| union_rect(rect, new_rect).unwrap_or(rect)),
            None => union_rect(old_rect, new_rect)?,
        });
    }
    damage
}

fn dirty_nodes_damage_rect(
    dom: &silksurf_dom::Dom,
    dirty_nodes: &[silksurf_dom::NodeId],
    old_fused: &FusedResult,
    new_fused: &FusedResult,
) -> Option<Rect> {
    if dirty_nodes.is_empty() {
        return None;
    }

    let mut damage = None;
    for &node in dirty_nodes {
        let is_text_node = matches!(
            dom.node(node).ok().map(silksurf_dom::Node::kind),
            Some(silksurf_dom::NodeKind::Text { .. })
        );
        if !is_text_node && !is_editable_input_node(dom, node) {
            return None;
        }
        let old_rect = fused_node_rect(old_fused, node)?;
        let new_rect = fused_node_rect(new_fused, node)?;
        damage = Some(match damage {
            Some(current) => union_rect(current, old_rect)
                .map_or(new_rect, |rect| union_rect(rect, new_rect).unwrap_or(rect)),
            None => union_rect(old_rect, new_rect)?,
        });
    }
    damage
}

fn fused_node_rect(fused: &FusedResult, node: silksurf_dom::NodeId) -> Option<Rect> {
    let bfs_idx = *fused.table.node_to_bfs_idx.get(&node)? as usize;
    fused.node_rects.get(bfs_idx).copied()
}

fn union_rect(a: Rect, b: Rect) -> Option<Rect> {
    if a.width <= 0.0 || a.height <= 0.0 {
        return Some(b);
    }
    if b.width <= 0.0 || b.height <= 0.0 {
        return Some(a);
    }
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn focus_address_bar(state: &mut BrowserState) -> bool {
    let next_text = state.frame.url.clone();
    let changed =
        !state.address_editing || !state.address_select_all || state.address_text != next_text;
    state.address_editing = true;
    state.address_select_all = true;
    state.address_text = next_text;
    state.address_cursor = state.address_text.len();
    changed
}

fn address_focus_redraw_mode(state: &BrowserState) -> BrowserRedrawMode {
    if !state.address_editing || state.address_text == state.frame.url {
        BrowserRedrawMode::AddressFocusChrome
    } else {
        BrowserRedrawMode::AddressChrome
    }
}

fn push_address_char(state: &mut BrowserState, ch: char) -> bool {
    if !state.address_editing || !(ch.is_ascii_graphic() || ch == ' ') {
        return false;
    }
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
    }
    if state.address_text.len() >= ADDRESS_TEXT_MAX_CHARS {
        return false;
    }
    let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
    state.address_text.insert(cursor, ch);
    state.address_cursor = cursor + ch.len_utf8();
    true
}

fn edit_address_backspace(state: &mut BrowserState) -> bool {
    if !state.address_editing {
        return false;
    }
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
        return true;
    }
    let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
    let previous = previous_address_cursor(&state.address_text, cursor);
    if previous == cursor {
        state.address_cursor = cursor;
        return false;
    }
    state.address_text.replace_range(previous..cursor, "");
    state.address_cursor = previous;
    true
}

fn address_clipboard_text(state: &BrowserState) -> Option<&str> {
    state
        .address_editing
        .then_some(state.address_text.as_str())
        .filter(|text| !text.is_empty())
}

fn paste_address_text(state: &mut BrowserState, text: &str) -> bool {
    if !state.address_editing {
        return false;
    }
    let mut changed = false;
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
        changed = true;
    }
    for ch in text.chars().filter(address_paste_char_allowed) {
        if state.address_text.len() >= ADDRESS_TEXT_MAX_CHARS {
            break;
        }
        let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
        state.address_text.insert(cursor, ch);
        state.address_cursor = cursor + ch.len_utf8();
        changed = true;
    }
    changed
}

fn cut_address_text(state: &mut BrowserState) -> bool {
    if !state.address_editing || !state.address_select_all || state.address_text.is_empty() {
        return false;
    }
    state.address_text.clear();
    state.address_select_all = false;
    state.address_cursor = 0;
    true
}

fn address_paste_char_allowed(ch: &char) -> bool {
    ch.is_ascii_graphic() || *ch == ' '
}

fn move_address_caret(state: &mut BrowserState, motion: AddressCaretMotion) -> bool {
    if !state.address_editing {
        return false;
    }
    let current = if state.address_select_all {
        selected_address_caret(&state.address_text, motion)
    } else {
        let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
        match motion {
            AddressCaretMotion::Backward => previous_address_cursor(&state.address_text, cursor),
            AddressCaretMotion::Forward => next_address_cursor(&state.address_text, cursor),
            AddressCaretMotion::Start => 0,
            AddressCaretMotion::End => state.address_text.len(),
        }
    };
    let changed = state.address_select_all || state.address_cursor != current;
    state.address_select_all = false;
    state.address_cursor = current;
    changed
}

fn selected_address_caret(text: &str, motion: AddressCaretMotion) -> usize {
    match motion {
        AddressCaretMotion::Backward | AddressCaretMotion::Start => 0,
        AddressCaretMotion::Forward | AddressCaretMotion::End => text.len(),
    }
}

fn clamp_address_cursor(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(cursor) {
        return cursor;
    }
    previous_address_cursor(text, cursor)
}

fn previous_address_cursor(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index < cursor)
        .last()
        .unwrap_or(0)
}

fn next_address_cursor(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .find(|index| *index > cursor)
        .unwrap_or(text.len())
}

fn read_clipboard_text() -> Result<String, arboard::Error> {
    arboard::Clipboard::new()?.get_text()
}

fn write_clipboard_text(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_text(text.to_owned())
}

fn focus_page_input(state: &mut BrowserState, node: silksurf_dom::NodeId) -> bool {
    let changed = state.focused_input != Some(node) || state.address_editing;
    let target_rect = state
        .frame
        .input_targets
        .iter()
        .find(|target| target.node == node)
        .map(|target| target.rect);
    let redraw_mode = if state.address_editing {
        Some(BrowserRedrawMode::AddressChrome)
    } else {
        target_rect.map(BrowserRedrawMode::PageInputFocus)
    };
    state.address_editing = false;
    state.address_select_all = false;
    state.focused_input = Some(node);
    if changed {
        if let Some(redraw_mode) = redraw_mode {
            mark_redraw(state, redraw_mode);
            if let BrowserRedrawMode::PageInputFocus(_) = redraw_mode
                && state.frame.current_view_retained_sent
            {
                let damage = browser_present_damage(
                    redraw_mode,
                    state.frame.raster_height,
                    BROWSER_CHROME_HEIGHT as u32,
                    state.frame.bitmap_scroll_y,
                    FRAME_WIDTH,
                    state.frame.bitmap_height,
                );
                if damage != silksurf_gui::WinitPresentDamage::Clean {
                    state.retained_present = Some(BrowserRetainedPresent {
                        tag: CURRENT_VIEW_RETAINED_TAG,
                        damage,
                    });
                }
            }
        }
        eprintln!("[SilkSurf] Page input focused: node={}", node.raw());
        trace_page_input_focus(node, target_rect);
    }
    changed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputControlKind {
    Checkbox,
    Radio,
    Select,
}

fn activate_page_input_control(state: &mut BrowserState, node: silksurf_dom::NodeId) -> bool {
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };
    let edit_result = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        input_control_kind(&dom, node).and_then(|kind| {
            let changed = dom.with_mutation_batch(|dom| match kind {
                InputControlKind::Checkbox => toggle_checkbox_control(dom, node),
                InputControlKind::Radio => check_radio_control(dom, runtime.document, node),
                InputControlKind::Select => cycle_select_control(dom, node),
            });
            changed.ok().filter(|changed| *changed).map(|_| {
                if kind == InputControlKind::Select {
                    let _ = dom.take_dirty_nodes();
                    return vec![node];
                }
                dom.take_dirty_nodes()
            })
        })
    };
    let Some(dirty_nodes) = edit_result else {
        state.runtime = Some(runtime);
        return false;
    };
    let redraw_mode = repaint_runtime_dirty_nodes(&mut runtime, &mut state.frame, &dirty_nodes);
    state.runtime = Some(runtime);
    state.focused_input = Some(node);
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        eprintln!(
            "[SilkSurf] Page input toggled: node={} dirty_nodes={}",
            node.raw(),
            dirty_nodes.len()
        );
        return true;
    }
    false
}

fn input_control_kind(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<InputControlKind> {
    match input_node_kind(dom, node)? {
        silksurf_dom::TagName::Input => match input_type(dom, node).as_str() {
            "checkbox" => Some(InputControlKind::Checkbox),
            "radio" => Some(InputControlKind::Radio),
            _ => None,
        },
        silksurf_dom::TagName::Select => Some(InputControlKind::Select),
        _ => None,
    }
}

fn cycle_select_control(
    dom: &mut silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let options = enabled_select_options(dom, select);
    let Some(next_option) = next_select_option(dom, &options) else {
        return Ok(false);
    };
    set_single_selected_option(dom, select, next_option)
}

fn enabled_select_options(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Vec<silksurf_dom::NodeId> {
    let mut options = Vec::new();
    collect_enabled_option_nodes(dom, select, &mut options);
    options
}

fn collect_enabled_option_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    options: &mut Vec<silksurf_dom::NodeId>,
) {
    if node_tag_name(dom, node) == Some(silksurf_dom::TagName::Option)
        && element_attribute(dom, node, "disabled").is_none()
    {
        options.push(node);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_enabled_option_nodes(dom, child, options);
    }
}

fn next_select_option(
    dom: &silksurf_dom::Dom,
    options: &[silksurf_dom::NodeId],
) -> Option<silksurf_dom::NodeId> {
    if options.is_empty() {
        return None;
    }
    let selected_index = options
        .iter()
        .position(|&option| option_selected(dom, option));
    Some(options[(selected_index.map_or(0, |index| index + 1)) % options.len()])
}

fn set_single_selected_option(
    dom: &mut silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
    selected: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let mut changed = false;
    for option in enabled_select_options(dom, select) {
        if option == selected {
            if !option_selected(dom, option) {
                dom.set_attribute(option, "selected", "")?;
                changed = true;
            }
        } else if dom.remove_attribute(option, "selected")? {
            changed = true;
        }
    }
    Ok(changed)
}

fn toggle_checkbox_control(
    dom: &mut silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    if input_checked(dom, node) {
        dom.remove_attribute(node, "checked")
    } else {
        dom.set_attribute(node, "checked", "")?;
        Ok(true)
    }
}

fn check_radio_control(
    dom: &mut silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    node: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let mut changed = false;
    if !input_checked(dom, node) {
        dom.set_attribute(node, "checked", "")?;
        changed = true;
    }
    let Some(name) = element_attribute(dom, node, "name").map(str::to_string) else {
        return Ok(changed);
    };
    if name.is_empty() {
        return Ok(changed);
    }
    let group_root = nearest_form_node(dom, node).unwrap_or(root);
    let mut radios = Vec::new();
    collect_radio_group_nodes(dom, group_root, name.as_str(), &mut radios);
    for radio in radios {
        if radio != node && dom.remove_attribute(radio, "checked")? {
            changed = true;
        }
    }
    Ok(changed)
}

fn collect_radio_group_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
    radios: &mut Vec<silksurf_dom::NodeId>,
) {
    if input_control_kind(dom, node) == Some(InputControlKind::Radio)
        && element_attribute(dom, node, "name").is_some_and(|value| value == name)
    {
        radios.push(node);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_radio_group_nodes(dom, child, name, radios);
    }
}

fn focus_next_page_input(state: &mut BrowserState) -> bool {
    let Some(next_node) =
        next_text_editable_input_target(state).or_else(|| next_input_target(state))
    else {
        return false;
    };
    focus_page_input(state, next_node)
}

fn next_text_editable_input_target(state: &BrowserState) -> Option<silksurf_dom::NodeId> {
    let runtime = state.runtime.as_ref()?;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut first = None;
    let mut take_next = state.focused_input.is_none();
    for target in &state.frame.input_targets {
        if !is_text_editable_input_node(&dom, target.node) {
            continue;
        }
        first.get_or_insert(target.node);
        if take_next {
            return Some(target.node);
        }
        if state.focused_input == Some(target.node) {
            take_next = true;
        }
    }
    first
}

fn next_input_target(state: &BrowserState) -> Option<silksurf_dom::NodeId> {
    let next_index = state
        .focused_input
        .and_then(|focused| {
            state
                .frame
                .input_targets
                .iter()
                .position(|target| target.node == focused)
        })
        .map_or(0, |index| (index + 1) % state.frame.input_targets.len());
    state
        .frame
        .input_targets
        .get(next_index)
        .map(|target| target.node)
}

fn focus_next_visible_page_input(
    state: &mut BrowserState,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) -> bool {
    if state.frame.input_targets.is_empty() {
        return false;
    }
    let visible_count = state
        .frame
        .input_targets
        .iter()
        .filter(|target| {
            input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
        })
        .count();
    if visible_count == 0 {
        return focus_next_page_input(state);
    }
    let next_visible_index = state
        .focused_input
        .and_then(|focused| {
            state
                .frame
                .input_targets
                .iter()
                .filter(|target| {
                    input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
                })
                .position(|target| target.node == focused)
        })
        .map_or(0, |index| (index + 1) % visible_count);
    let Some(next_node) = state
        .frame
        .input_targets
        .iter()
        .filter(|target| {
            input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
        })
        .nth(next_visible_index)
        .map(|target| target.node)
    else {
        return false;
    };
    trace_visible_page_input_focus(next_node, scroll_y, chrome_height, window_height);
    focus_page_input(state, next_node)
}

fn input_target_intersects_viewport(
    target: &InputTarget,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) -> bool {
    let viewport_top = scroll_y + chrome_height as f32;
    let viewport_bottom = scroll_y + window_height as f32;
    let target_top = target.rect.y;
    let target_bottom = target.rect.y + target.rect.height;
    target.rect.width > 0.0
        && target.rect.height > 0.0
        && target_bottom > viewport_top
        && target_top < viewport_bottom
}

fn trace_visible_page_input_focus(
    node: silksurf_dom::NodeId,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input focus visible: node={} scroll={} chrome={} height={}",
        node.raw(),
        scroll_y,
        chrome_height,
        window_height
    );
}

fn trace_page_input_focus(node: silksurf_dom::NodeId, rect: Option<Rect>) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    if let Some(rect) = rect {
        eprintln!(
            "[SilkSurf] input focus target: node={} rect=({}, {}, {}, {})",
            node.raw(),
            rect.x,
            rect.y,
            rect.width,
            rect.height
        );
    }
}

fn clear_page_input_focus(state: &mut BrowserState) {
    state.focused_input = None;
}

fn push_focused_input_char(state: &mut BrowserState, ch: char) -> bool {
    if !(ch.is_ascii_graphic() || ch == ' ') {
        return false;
    }
    edit_focused_input_value(state, |value| {
        if value.len() >= PAGE_INPUT_TEXT_MAX_CHARS {
            return false;
        }
        value.push(ch);
        true
    })
}

fn push_focused_textarea_newline(state: &mut BrowserState) -> bool {
    let Some(node) = state.focused_input else {
        return false;
    };
    let is_textarea = {
        let Some(runtime) = state.runtime.as_ref() else {
            return false;
        };
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        is_text_content_editable_input_node(&dom, node)
    };
    if !is_textarea {
        return false;
    }
    edit_focused_input_value(state, |value| {
        if value.len() >= PAGE_INPUT_TEXT_MAX_CHARS {
            return false;
        }
        value.push('\n');
        true
    })
}

fn edit_focused_input_backspace(state: &mut BrowserState) -> bool {
    edit_focused_input_value(state, |value| value.pop().is_some())
}

fn edit_focused_input_value(
    state: &mut BrowserState,
    edit: impl FnOnce(&mut String) -> bool,
) -> bool {
    let Some(node) = state.focused_input else {
        return false;
    };
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };

    let edit_result = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !is_text_editable_input_node(&dom, node) {
            None
        } else {
            let mut value = input_value(&dom, node);
            if !edit(&mut value) {
                None
            } else {
                let value_before_edit = input_value(&dom, node);
                let value_after_edit = value.clone();
                let result =
                    dom.with_mutation_batch(|dom| set_editable_input_value(dom, node, value));
                if result.is_err() {
                    None
                } else {
                    Some((dom.take_dirty_nodes(), value_before_edit, value_after_edit))
                }
            }
        }
    };
    let Some((dirty_nodes, value_before_edit, value_after_edit)) = edit_result else {
        state.runtime = Some(runtime);
        return false;
    };

    let empty_insert_damage =
        focused_empty_insert_damage(&state.frame, node, &value_before_edit, &value_after_edit);
    let redraw_mode =
        repaint_focused_input_value(&mut runtime, &mut state.frame, node, &value_after_edit)
            .or_else(|| {
                repaint_runtime_dirty_nodes(&mut runtime, &mut state.frame, &dirty_nodes)
                    .map(|mode| empty_insert_damage.map_or(mode, BrowserRedrawMode::Damage))
            });
    state.runtime = Some(runtime);
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        eprintln!(
            "[SilkSurf] Page input updated: node={} dirty_nodes={}",
            node.raw(),
            dirty_nodes.len()
        );
        return true;
    }
    false
}

fn input_value(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    if is_text_content_editable_input_node(dom, node) {
        return textarea_text(dom, node);
    }
    dom.attributes(node)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "value")
                .map(|attr| attr.value.to_string())
        })
        .unwrap_or_default()
}

fn set_editable_input_value(
    dom: &mut silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    value: String,
) -> Result<(), silksurf_dom::DomError> {
    if is_text_content_editable_input_node(dom, node) {
        dom.set_text_content(node, value)
    } else {
        dom.set_attribute(node, "value", value)
    }
}

fn textarea_text(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    let mut text = String::new();
    append_text_descendants(dom, node, &mut text);
    text
}

fn append_text_descendants(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId, text: &mut String) {
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        if let Ok(dom_node) = dom.node(child)
            && let silksurf_dom::NodeKind::Text { text: child_text } = dom_node.kind()
        {
            text.push_str(child_text);
            continue;
        }
        append_text_descendants(dom, child, text);
    }
}

fn focused_form_submission_target(state: &BrowserState) -> Option<FormSubmissionTarget> {
    let focused = state.focused_input?;
    let runtime = state.runtime.as_ref()?;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if is_textarea_node(&dom, focused) {
        return None;
    }
    let form = nearest_form_node(&dom, focused)?;
    form_submission_target(&dom, form, &state.frame.url)
}

fn form_submission_target(
    dom: &silksurf_dom::Dom,
    form: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<FormSubmissionTarget> {
    let method = element_attribute(dom, form, "method")
        .unwrap_or("get")
        .to_ascii_lowercase();
    let action = element_attribute(dom, form, "action").unwrap_or("");
    let mut target = url::Url::parse(base_url).ok()?.join(action).ok()?;
    let pairs = form_submission_pairs(dom, form);
    match method.as_str() {
        "get" => {
            for (name, value) in pairs {
                target.query_pairs_mut().append_pair(&name, &value);
            }
            browser_supported_url(&target).map(FormSubmissionTarget::Get)
        }
        "post" => {
            let body = encode_form_submission_body(&pairs);
            browser_supported_url(&target).map(|url| {
                FormSubmissionTarget::Post(BrowserNavigationRequest::post_form(
                    url,
                    body.into_bytes(),
                ))
            })
        }
        _ => Some(FormSubmissionTarget::UnsupportedMethod(method)),
    }
}

fn nearest_form_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::NodeId> {
    let mut current = Some(node);
    while let Some(id) = current {
        if dom.element_name(id).ok().flatten().is_some_and(|name| {
            silksurf_dom::TagName::from_str(name) == silksurf_dom::TagName::Form
        }) {
            return Some(id);
        }
        current = dom.parent(id).ok().flatten();
    }
    None
}

fn form_submission_pairs(
    dom: &silksurf_dom::Dom,
    form: silksurf_dom::NodeId,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    collect_form_submission_pairs(dom, form, &mut pairs);
    pairs
}

fn encode_form_submission_body(pairs: &[(String, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

fn collect_form_submission_pairs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    pairs: &mut Vec<(String, String)>,
) {
    if let Some(pair) = form_control_submission_pair(dom, node) {
        pairs.push(pair);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_form_submission_pairs(dom, child, pairs);
    }
}

fn form_control_submission_pair(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<(String, String)> {
    if element_attribute(dom, node, "disabled").is_some() {
        return None;
    }
    let name = element_attribute(dom, node, "name")?.to_string();
    if name.is_empty() {
        return None;
    }
    let element_name = dom.element_name(node).ok().flatten()?;
    match silksurf_dom::TagName::from_str(element_name) {
        silksurf_dom::TagName::Input => {
            input_submission_value(dom, node).map(|value| (name, value))
        }
        silksurf_dom::TagName::Select => {
            select_submission_value(dom, node).map(|value| (name, value))
        }
        silksurf_dom::TagName::Textarea => Some((name, textarea_text(dom, node))),
        _ => None,
    }
}

fn select_submission_value(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Option<String> {
    selected_select_option(dom, select).map(|option| option_value(dom, option))
}

fn selected_select_option(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Option<silksurf_dom::NodeId> {
    let options = enabled_select_options(dom, select);
    options
        .iter()
        .copied()
        .find(|&option| option_selected(dom, option))
        .or_else(|| options.first().copied())
}

fn option_value(dom: &silksurf_dom::Dom, option: silksurf_dom::NodeId) -> String {
    element_attribute(dom, option, "value")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| textarea_text(dom, option))
}

fn input_submission_value(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> Option<String> {
    let input_type = element_attribute(dom, node, "type")
        .unwrap_or("text")
        .to_ascii_lowercase();
    match input_type.as_str() {
        "button" | "file" | "image" | "reset" | "submit" => None,
        "checkbox" | "radio" => input_checked(dom, node).then(|| checkbox_radio_value(dom, node)),
        _ => Some(input_value(dom, node)),
    }
}

fn checkbox_radio_value(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    element_attribute(dom, node, "value")
        .filter(|value| !value.is_empty())
        .unwrap_or("on")
        .to_string()
}

fn input_checked(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    element_attribute(dom, node, "checked").is_some()
}

fn option_selected(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    element_attribute(dom, node, "selected").is_some()
}

fn input_type(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    element_attribute(dom, node, "type")
        .unwrap_or("text")
        .to_ascii_lowercase()
}

fn element_attribute<'a>(
    dom: &'a silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
) -> Option<&'a str> {
    dom.attributes(node).ok()?.iter().find_map(|attr| {
        attr.name
            .as_str()
            .eq_ignore_ascii_case(name)
            .then_some(attr.value.as_str())
    })
}

fn http_method_label(method: HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
    }
}

fn normalize_address_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        return browser_supported_url(&url);
    }
    let with_scheme = format!("https://{trimmed}");
    let url = url::Url::parse(&with_scheme).ok()?;
    browser_supported_url(&url)
}

fn browser_home_url() -> String {
    std::env::var("SILKSURF_HOME_URL")
        .ok()
        .and_then(|value| normalize_address_input(&value))
        .unwrap_or_else(|| HOME_URL.to_string())
}

fn browser_address_bar_contains(x: f32, y: f32) -> bool {
    x >= ADDRESS_BAR_X as f32
        && x < (ADDRESS_BAR_X + ADDRESS_BAR_WIDTH) as f32
        && y >= ADDRESS_BAR_Y as f32
        && y < (ADDRESS_BAR_Y + ADDRESS_BAR_HEIGHT) as f32
}

fn hit_test_chrome_action(x: f32, y: f32) -> Option<BrowserChromeAction> {
    if nav_button_contains(BACK_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Back);
    }
    if nav_button_contains(FORWARD_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Forward);
    }
    if nav_button_contains(HOME_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Home);
    }
    if nav_button_contains(RELOAD_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Reload);
    }
    if nav_button_contains(STOP_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Stop);
    }
    None
}

fn nav_button_contains(button_x: u32, x: f32, y: f32) -> bool {
    x >= button_x as f32
        && x < (button_x + NAV_BUTTON_WIDTH) as f32
        && y >= NAV_BUTTON_Y as f32
        && y < (NAV_BUTTON_Y + NAV_BUTTON_HEIGHT) as f32
}

#[cfg(test)]
fn rgba(r: u8, g: u8, b: u8, a: u8) -> silksurf_css::Color {
    silksurf_css::Color { r, g, b, a }
}

fn rgba_bytes_to_argb_words_into(rgba: &[u8], argb: &mut Vec<u32>) {
    let _ = rgba_bytes_to_argb_words_into_timed(rgba, argb);
}

fn rgba_bytes_to_argb_words_into_timed(
    rgba: &[u8],
    argb: &mut Vec<u32>,
) -> (std::time::Duration, std::time::Duration) {
    let resize_start = std::time::Instant::now();
    resize_argb_words_uninit(argb, rgba.len() / 4);
    let resize_elapsed = resize_start.elapsed();

    let pack_start = std::time::Instant::now();
    pack_rgba_bytes_to_argb_words(rgba, argb);
    (resize_elapsed, pack_start.elapsed())
}

fn resize_argb_words_uninit(argb: &mut Vec<u32>, target_len: usize) {
    if target_len <= argb.len() {
        argb.truncate(target_len);
        return;
    }
    if argb.capacity() < target_len {
        argb.reserve_exact(target_len - argb.len());
    }
    /*
     * SAFETY: each caller overwrites every exposed word before any framebuffer
     * read. u32 has no destructor, so an early panic only releases the
     * allocation.
     */
    unsafe {
        argb.set_len(target_len);
    }
}

fn pack_rgba_bytes_to_argb_words(rgba: &[u8], argb: &mut [u32]) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: the runtime feature gate above proves AVX2 support.
            let packed = unsafe { pack_rgba_bytes_to_argb_words_avx2(rgba, argb) };
            pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
            return;
        }
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            if std::is_x86_feature_detected!("ssse3") {
                // SAFETY: the runtime feature gate above proves SSSE3 support.
                let packed = unsafe { pack_rgba_bytes_to_argb_words_ssse3(rgba, argb) };
                pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
                return;
            }
            // SAFETY: the runtime feature gate above proves SSE2 support.
            let packed = unsafe { pack_rgba_bytes_to_argb_words_sse2(rgba, argb) };
            pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
            return;
        }
    }

    pack_rgba_bytes_to_argb_words_scalar(rgba, argb);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn pack_rgba_bytes_to_argb_words_avx2(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m256i, _mm256_loadu_si256, _mm256_setr_epi8, _mm256_shuffle_epi8, _mm256_storeu_si256,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m256i, _mm256_loadu_si256, _mm256_setr_epi8, _mm256_shuffle_epi8, _mm256_storeu_si256,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 8;
    let shuffle_mask = _mm256_setr_epi8(
        2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15, 2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11,
        14, 13, 12, 15,
    );

    for lane in 0..lanes {
        let rgba_offset = lane * 32;
        let argb_offset = lane * 8;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m256i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m256i>() };
        // SAFETY: AVX2 unaligned load reads one complete 32-byte lane.
        let raw = unsafe { _mm256_loadu_si256(source) };
        let argb_words = _mm256_shuffle_epi8(raw, shuffle_mask);
        // SAFETY: AVX2 unaligned store writes one complete 8-word lane.
        unsafe {
            _mm256_storeu_si256(dest, argb_words);
        }
    }

    lanes * 8
}

fn pack_rgba_bytes_to_argb_words_scalar(rgba: &[u8], argb: &mut [u32]) {
    for (dst, px) in argb.iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = argb_word_from_rgba(px);
    }
}

fn argb_word_from_rgba(px: &[u8]) -> u32 {
    (u32::from(px[3]) << 24) | (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2])
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn pack_rgba_bytes_to_argb_words_ssse3(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m128i, _mm_loadu_si128, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m128i, _mm_loadu_si128, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 4;
    let shuffle_mask = _mm_setr_epi8(2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15);

    for lane in 0..lanes {
        let rgba_offset = lane * 16;
        let argb_offset = lane * 4;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m128i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m128i>() };
        // SAFETY: SSSE3 unaligned load reads one complete 16-byte lane.
        let raw = unsafe { _mm_loadu_si128(source) };
        let argb_words = _mm_shuffle_epi8(raw, shuffle_mask);
        // SAFETY: SSSE3 unaligned store writes one complete 4-word lane.
        unsafe {
            _mm_storeu_si128(dest, argb_words);
        }
    }

    lanes * 4
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn pack_rgba_bytes_to_argb_words_sse2(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi32, _mm_slli_epi32,
        _mm_srli_epi32, _mm_storeu_si128,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi32, _mm_slli_epi32,
        _mm_srli_epi32, _mm_storeu_si128,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 4;
    let red_blue_mask = _mm_set1_epi32(0x00ff_00ff);
    let green_alpha_mask = _mm_set1_epi32(0xff00_ff00u32 as i32);

    for lane in 0..lanes {
        let rgba_offset = lane * 16;
        let argb_offset = lane * 4;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m128i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m128i>() };
        // SAFETY: SSE2 unaligned load reads one complete 16-byte lane.
        let raw = unsafe { _mm_loadu_si128(source) };
        let red_blue = _mm_and_si128(raw, red_blue_mask);
        let green_alpha = _mm_and_si128(raw, green_alpha_mask);
        let red = _mm_slli_epi32(red_blue, 16);
        let blue = _mm_srli_epi32(red_blue, 16);
        let argb_words = _mm_or_si128(green_alpha, _mm_or_si128(red, blue));
        // SAFETY: SSE2 unaligned store writes one complete 4-word lane.
        unsafe {
            _mm_storeu_si128(dest, argb_words);
        }
    }

    lanes * 4
}

fn sync_argb_damage_from_rgba(
    rgba: &[u8],
    argb: &mut [u32],
    width: u32,
    height: u32,
    damage: Rect,
) {
    let x0 = damage.x.floor().max(0.0).min(width as f32) as u32;
    let x1 = (damage.x + damage.width).ceil().max(0.0).min(width as f32) as u32;
    let y0 = damage.y.floor().max(0.0).min(height as f32) as u32;
    let y1 = (damage.y + damage.height)
        .ceil()
        .max(0.0)
        .min(height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let width_usize = width as usize;
    for y in y0 as usize..y1 as usize {
        let row_start = y * width_usize;
        let argb_start = row_start + x0 as usize;
        let argb_end = row_start + x1 as usize;
        let rgba_start = argb_start * 4;
        let rgba_end = argb_end * 4;
        if argb_end > argb.len() || rgba_end > rgba.len() {
            return;
        }
        let argb_row = &mut argb[argb_start..argb_end];
        let rgba_row = &rgba[rgba_start..rgba_end];
        pack_rgba_bytes_to_argb_words(rgba_row, argb_row);
    }
}

fn sync_argb_damage_from_scratch(
    scratch: &silksurf_render::DamageScratch,
    argb: &mut [u32],
    frame_width: u32,
) -> bool {
    let Some(damage) = scratch.last_damage() else {
        return false;
    };
    let frame_width = frame_width as usize;
    let damage_x = damage.x as usize;
    let damage_y = damage.y as usize;
    let damage_width = damage.width as usize;
    let damage_height = damage.height as usize;
    if frame_width == 0 || damage_width == 0 || damage_height == 0 {
        return false;
    }
    let scratch_stride = damage_width * 4;
    let scratch_pixels = scratch.pixels();
    if scratch_pixels.len() < scratch_stride * damage_height {
        return false;
    }

    for row in 0..damage_height {
        let argb_start = (damage_y + row) * frame_width + damage_x;
        let argb_end = argb_start + damage_width;
        let scratch_start = row * scratch_stride;
        let scratch_end = scratch_start + scratch_stride;
        if argb_end > argb.len() || scratch_end > scratch_pixels.len() {
            return false;
        }
        let argb_row = &mut argb[argb_start..argb_end];
        let scratch_row = &scratch_pixels[scratch_start..scratch_end];
        pack_rgba_bytes_to_argb_words(scratch_row, argb_row);
    }
    true
}

fn stylesheet_text_with_user_agent_defaults(document_css: &str) -> String {
    let mut css_text =
        String::with_capacity(DEFAULT_USER_AGENT_STYLESHEET.len() + document_css.len() + 1);
    css_text.push_str(DEFAULT_USER_AGENT_STYLESHEET);
    css_text.push('\n');
    css_text.push_str(document_css);
    css_text
}

fn collect_link_targets(
    dom: &silksurf_dom::Dom,
    items: &[silksurf_render::DisplayItem],
    base_url: &str,
) -> Vec<LinkTarget> {
    let mut targets = Vec::new();
    for item in items {
        let silksurf_render::DisplayItem::Text { rect, node, .. } = item else {
            continue;
        };
        if let Some(href) = href_for_node_anchor(dom, *node, base_url) {
            targets.push(LinkTarget { rect: *rect, href });
        }
    }
    targets
}

fn collect_input_targets(dom: &silksurf_dom::Dom, fused: &FusedResult) -> Vec<InputTarget> {
    let mut targets = Vec::new();
    for &node in &fused.table.bfs_order {
        if !is_editable_input_node(dom, node) {
            continue;
        }
        let Some(rect) = fused_node_rect(fused, node) else {
            continue;
        };
        if rect.width > 0.0 && rect.height > 0.0 {
            targets.push(InputTarget { rect, node });
        }
    }
    targets
}

#[cfg(feature = "accessibility")]
fn log_accessibility_snapshot(state: &BrowserState) {
    let update = build_browser_accessibility_update(state);
    eprintln!(
        "[SilkSurf] Accessibility snapshot: nodes={}",
        update.nodes.len()
    );
}

#[cfg(feature = "accessibility")]
fn build_browser_accessibility_update(state: &BrowserState) -> accesskit::TreeUpdate {
    let mut nodes =
        Vec::with_capacity(8 + state.frame.link_targets.len() + state.frame.input_targets.len());
    let mut root = accesskit::Node::new(accesskit::Role::RootWebArea);
    root.set_label("SilkSurf");
    root.set_url(state.frame.url.as_str());
    root.set_bounds(accessibility_rect(
        0.0,
        0.0,
        FRAME_WIDTH as f32,
        state.frame.bitmap_height as f32,
    ));

    push_chrome_accessibility_nodes(state, &mut root, &mut nodes);
    push_link_accessibility_nodes(&state.frame.link_targets, &mut root, &mut nodes);
    push_input_accessibility_nodes(state, &mut root, &mut nodes);
    nodes.push((accesskit::NodeId(ACCESSIBILITY_ROOT_ID), root));

    accesskit::TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree::new(accesskit::NodeId(
            ACCESSIBILITY_ROOT_ID,
        ))),
        tree_id: accesskit::TreeId::ROOT,
        focus: accessibility_focus_id(state),
    }
}

#[cfg(feature = "accessibility")]
fn push_chrome_accessibility_nodes(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    push_accessibility_button(root, nodes, ACCESSIBILITY_BACK_ID, "Back", BACK_BUTTON_X);
    push_accessibility_button(
        root,
        nodes,
        ACCESSIBILITY_FORWARD_ID,
        "Forward",
        FORWARD_BUTTON_X,
    );
    push_accessibility_button(root, nodes, ACCESSIBILITY_HOME_ID, "Home", HOME_BUTTON_X);
    push_accessibility_button(
        root,
        nodes,
        ACCESSIBILITY_RELOAD_ID,
        "Reload",
        RELOAD_BUTTON_X,
    );
    push_accessibility_button(root, nodes, ACCESSIBILITY_STOP_ID, "Stop", STOP_BUTTON_X);
    push_address_accessibility_node(state, root, nodes);
    push_status_accessibility_node(state, root, nodes);
}

#[cfg(feature = "accessibility")]
fn push_accessibility_button(
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
    id: u64,
    label: &str,
    x: u32,
) {
    let mut button = accesskit::Node::new(accesskit::Role::Button);
    button.set_label(label);
    button.set_bounds(accessibility_rect(
        x as f32,
        NAV_BUTTON_Y as f32,
        NAV_BUTTON_WIDTH as f32,
        NAV_BUTTON_HEIGHT as f32,
    ));
    button.add_action(accesskit::Action::Click);
    nodes.push((accesskit::NodeId(id), button));
    root.push_child(accesskit::NodeId(id));
}

#[cfg(feature = "accessibility")]
fn push_address_accessibility_node(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    let mut address = accesskit::Node::new(accesskit::Role::UrlInput);
    address.set_label("Address");
    let address_value = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    address.set_value(address_value);
    address.set_bounds(accessibility_rect(
        ADDRESS_BAR_X as f32,
        ADDRESS_BAR_Y as f32,
        ADDRESS_BAR_WIDTH as f32,
        ADDRESS_BAR_HEIGHT as f32,
    ));
    address.add_action(accesskit::Action::Focus);
    nodes.push((accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID), address));
    root.push_child(accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID));
}

#[cfg(feature = "accessibility")]
fn push_status_accessibility_node(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    let mut status = accesskit::Node::new(accesskit::Role::Label);
    status.set_value(browser_status_text(state));
    status.set_bounds(accessibility_rect(
        (ADDRESS_BAR_X + ADDRESS_BAR_WIDTH + 12) as f32,
        17.0,
        96.0,
        14.0,
    ));
    nodes.push((accesskit::NodeId(ACCESSIBILITY_STATUS_ID), status));
    root.push_child(accesskit::NodeId(ACCESSIBILITY_STATUS_ID));
}

#[cfg(feature = "accessibility")]
fn push_link_accessibility_nodes(
    links: &[LinkTarget],
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    for (index, target) in links.iter().enumerate() {
        let id = accesskit::NodeId(ACCESSIBILITY_LINK_BASE_ID + index as u64);
        let mut link = accesskit::Node::new(accesskit::Role::Link);
        link.set_label(target.href.as_str());
        link.set_url(target.href.as_str());
        link.set_bounds(accessibility_rect_from_layout(target.rect));
        link.add_action(accesskit::Action::Click);
        nodes.push((id, link));
        root.push_child(id);
    }
}

#[cfg(feature = "accessibility")]
fn push_input_accessibility_nodes(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    for target in &state.frame.input_targets {
        let id = accessibility_input_id(target.node);
        let mut input = accesskit::Node::new(accesskit::Role::TextInput);
        input.set_label("Page input");
        let value = accessibility_input_value(state, target.node);
        input.set_value(value.as_str());
        input.set_bounds(accessibility_rect_from_layout(target.rect));
        input.add_action(accesskit::Action::Focus);
        nodes.push((id, input));
        root.push_child(id);
    }
}

#[cfg(feature = "accessibility")]
fn accessibility_focus_id(state: &BrowserState) -> accesskit::NodeId {
    if state.address_editing {
        return accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID);
    }
    state
        .focused_input
        .map(accessibility_input_id)
        .unwrap_or(accesskit::NodeId(ACCESSIBILITY_ROOT_ID))
}

#[cfg(feature = "accessibility")]
fn accessibility_input_id(node: silksurf_dom::NodeId) -> accesskit::NodeId {
    accesskit::NodeId(ACCESSIBILITY_INPUT_BASE_ID + node.raw() as u64)
}

#[cfg(feature = "accessibility")]
fn accessibility_input_value(state: &BrowserState, node: silksurf_dom::NodeId) -> String {
    let Some(runtime) = &state.runtime else {
        return String::new();
    };
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    input_value(&dom, node)
}

#[cfg(feature = "accessibility")]
fn accessibility_rect_from_layout(rect: Rect) -> accesskit::Rect {
    accessibility_rect(rect.x, rect.y, rect.width, rect.height)
}

#[cfg(feature = "accessibility")]
fn accessibility_rect(x: f32, y: f32, width: f32, height: f32) -> accesskit::Rect {
    accesskit::Rect::new(x as f64, y as f64, (x + width) as f64, (y + height) as f64)
}

fn is_editable_input_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    input_node_kind(dom, node).is_some() || is_text_content_editable_node(dom, node)
}

fn is_text_editable_input_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    if is_textarea_node(dom, node) || is_text_content_editable_node(dom, node) {
        return true;
    }
    if input_node_kind(dom, node) != Some(silksurf_dom::TagName::Input) {
        return false;
    }
    !matches!(
        input_type(dom, node).as_str(),
        "button"
            | "checkbox"
            | "color"
            | "file"
            | "hidden"
            | "image"
            | "radio"
            | "range"
            | "reset"
            | "submit"
    )
}

fn is_textarea_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    input_node_kind(dom, node) == Some(silksurf_dom::TagName::Textarea)
}

fn is_text_content_editable_input_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> bool {
    is_textarea_node(dom, node) || is_text_content_editable_node(dom, node)
}

fn is_text_content_editable_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    dom.attributes(node)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "contenteditable")
        })
        .is_some_and(|attr| contenteditable_value_is_editable(attr.value.as_str()))
}

fn contenteditable_value_is_editable(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("plaintext-only")
}

fn input_node_kind(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::TagName> {
    let tag = node_tag_name(dom, node)?;
    matches!(
        tag,
        silksurf_dom::TagName::Input
            | silksurf_dom::TagName::Textarea
            | silksurf_dom::TagName::Select
    )
    .then_some(tag)
}

fn node_tag_name(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::TagName> {
    dom.element_name(node)
        .ok()
        .flatten()
        .map(|name| silksurf_dom::TagName::from_str(&name))
}

fn href_for_node_anchor(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<String> {
    let mut current = Some(node);
    while let Some(node_id) = current {
        if dom
            .element_name(node_id)
            .ok()
            .flatten()
            .is_some_and(|name| name.eq_ignore_ascii_case("a"))
            && let Ok(attrs) = dom.attributes(node_id)
            && let Some(href) = attrs
                .iter()
                .find(|attr| attr.name == silksurf_dom::AttributeName::Href)
        {
            return resolve_page_url(href.value.as_str(), base_url);
        }
        current = dom.parent(node_id).ok().flatten();
    }
    None
}

fn resolve_page_url(href: &str, base_url: &str) -> Option<String> {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        return browser_supported_url(&url);
    }
    let base = url::Url::parse(base_url).ok()?;
    let joined = base.join(trimmed).ok()?;
    browser_supported_url(&joined)
}

fn browser_supported_url(url: &url::Url) -> Option<String> {
    match url.scheme() {
        "http" | "https" => Some(url.to_string()),
        _ => None,
    }
}

fn hit_test_link(
    targets: &[LinkTarget],
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> Option<&str> {
    if window_y < chrome_height as f32 {
        return None;
    }
    let document_y = window_y + scroll_y;
    targets
        .iter()
        .rev()
        .find(|target| rect_contains(target.rect, window_x, document_y))
        .map(|target| target.href.as_str())
}

fn hit_test_input(
    targets: &[InputTarget],
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> Option<silksurf_dom::NodeId> {
    if window_y < chrome_height as f32 {
        return None;
    }
    let document_y = window_y + scroll_y;
    targets
        .iter()
        .rev()
        .find(|target| rect_contains(target.rect, window_x, document_y))
        .map(|target| target.node)
}

fn trace_input_hit_test(state: &BrowserState, x: f32, y: f32, scroll_y: f32) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input hit-test: click=({x:.1},{y:.1}) scroll={scroll_y:.1} inputs={} links={}",
        state.frame.input_targets.len(),
        state.frame.link_targets.len()
    );
    for target in &state.frame.link_targets {
        eprintln!(
            "[SilkSurf] link target: href={} rect=({}, {}, {}, {})",
            target.href, target.rect.x, target.rect.y, target.rect.width, target.rect.height
        );
    }
    eprintln!(
        "[SilkSurf] input target count: {}",
        state.frame.input_targets.len()
    );
    for target in &state.frame.input_targets {
        eprintln!(
            "[SilkSurf] input target: node={} rect=({}, {}, {}, {})",
            target.node.raw(),
            target.rect.x,
            target.rect.y,
            target.rect.width,
            target.rect.height
        );
    }
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn browser_frame_height(items: &[silksurf_render::DisplayItem], minimum_height: u32) -> u32 {
    let content_bottom = items
        .iter()
        .map(display_item_bottom)
        .fold(minimum_height as f32, f32::max);
    content_bottom.ceil().max(minimum_height as f32) as u32
}

fn tile_browser_document_display_list(
    display_list: silksurf_render::DisplayList,
    document_height: u32,
) -> silksurf_render::DisplayList {
    display_list.with_tiles(FRAME_WIDTH, document_height, DOCUMENT_TILE_SIZE)
}

fn rasterize_browser_viewport_into(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    rgba: &mut Vec<u8>,
) {
    let viewport_list = browser_viewport_display_list(display_list, scroll_y, bitmap_height);
    silksurf_render::rasterize_skia_into(&viewport_list, FRAME_WIDTH, bitmap_height, rgba);
    fill_browser_toolbar_background_rgba(rgba, FRAME_WIDTH, bitmap_height);
}

fn rasterize_browser_viewport_argb_preferred(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    rgba: &mut Vec<u8>,
    argb: &mut Vec<u32>,
) -> bool {
    if rasterize_browser_viewport_argb_direct(display_list, scroll_y, bitmap_height, argb) {
        return true;
    }
    rasterize_browser_viewport_into(display_list, scroll_y, bitmap_height, rgba);
    rgba_bytes_to_argb_words_into(rgba, argb);
    false
}

fn first_prepared_focus_target(
    dom: &silksurf_dom::Dom,
    input_targets: &[InputTarget],
) -> Option<InputTarget> {
    input_targets
        .iter()
        .find(|target| is_text_editable_input_node(dom, target.node))
        .or_else(|| input_targets.first())
        .cloned()
}

fn build_focus_viewport_cache(
    display_list: &silksurf_render::DisplayList,
    focus_target: Option<&InputTarget>,
    document_height: u32,
    bitmap_height: u32,
    chrome_height: u32,
) -> Option<FocusViewportCache> {
    let target = focus_target?;
    let target_scroll = focus_target_scroll(target, document_height, bitmap_height, chrome_height)?;
    let mut rgba = Vec::new();
    let mut argb = Vec::new();
    rasterize_browser_viewport_argb_preferred(
        display_list,
        target_scroll,
        bitmap_height,
        &mut rgba,
        &mut argb,
    );
    Some(FocusViewportCache {
        scroll_y: target_scroll,
        bitmap_height,
        argb,
    })
}

#[cfg(test)]
fn first_focus_target_scroll(
    input_targets: &[InputTarget],
    document_height: u32,
    bitmap_height: u32,
    chrome_height: u32,
) -> Option<u32> {
    let target = input_targets.first()?;
    focus_target_scroll(target, document_height, bitmap_height, chrome_height)
}

fn focus_target_scroll(
    target: &InputTarget,
    document_height: u32,
    bitmap_height: u32,
    chrome_height: u32,
) -> Option<u32> {
    let max_scroll = max_browser_scroll_offset(document_height, bitmap_height, chrome_height);
    let scroll =
        scroll_to_show_input_target(0.0, target.rect, max_scroll, chrome_height, bitmap_height);
    (scroll >= 0.5).then_some(scroll.round() as u32)
}

fn rasterize_browser_document_damage_into(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    rgba: &mut Vec<u8>,
    scratch: &mut silksurf_render::DamageScratch,
) {
    silksurf_render::rasterize_skia_translated_damage_into(
        display_list,
        FRAME_WIDTH,
        bitmap_height,
        viewport_damage_rect(damage, scroll_y),
        damage,
        (0.0, -(scroll_y as f32)),
        rgba,
        scratch,
    );
}

fn rasterize_browser_document_damage_scratch(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    scratch: &mut silksurf_render::DamageScratch,
) {
    silksurf_render::rasterize_skia_translated_damage_scratch(
        display_list,
        FRAME_WIDTH,
        bitmap_height,
        viewport_damage_rect(damage, scroll_y),
        damage,
        (0.0, -(scroll_y as f32)),
        scratch,
    );
}

fn trace_visible_document_raster(display_list: &silksurf_render::DisplayList, damage: Rect) {
    if std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_none() {
        return;
    }
    let mut item_count = 0usize;
    let mut text_count = 0usize;
    let mut text_bytes = 0usize;
    for item in browser_viewport_source_items(display_list, damage) {
        if !display_item_intersects_viewport(item, damage) {
            continue;
        }
        item_count += 1;
        if let silksurf_render::DisplayItem::Text { text, .. } = item {
            text_count += 1;
            text_bytes += text.len();
        }
    }
    eprintln!(
        "[SilkSurf] visible document raster: total_items={} items={item_count} text_items={text_count} text_bytes={text_bytes} rect=({}, {}, {}, {})",
        display_list.items.len(),
        damage.x,
        damage.y,
        damage.width,
        damage.height
    );
}

fn fill_browser_toolbar_background_rgba(rgba: &mut [u8], width: u32, height: u32) {
    let toolbar_rows = (BROWSER_CHROME_HEIGHT as u32).min(height);
    if width == 0 || toolbar_rows == 0 {
        return;
    }
    let row_bytes = width as usize * 4;
    let toolbar_bytes = toolbar_rows as usize * row_bytes;
    if rgba.len() < toolbar_bytes {
        return;
    }
    for pixel in rgba[..toolbar_bytes].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[243, 244, 246, 255]);
    }
    let separator_start = (toolbar_rows as usize - 1) * row_bytes;
    let separator_end = separator_start + row_bytes;
    for pixel in rgba[separator_start..separator_end].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[209, 213, 219, 255]);
    }
}

fn fill_browser_toolbar_background_argb(pixels: &mut [u32], width: u32, height: u32) {
    let toolbar_rows = (BROWSER_CHROME_HEIGHT as u32).min(height);
    if width == 0 || toolbar_rows == 0 {
        return;
    }
    fill_argb_rect(
        pixels,
        width,
        height,
        0,
        0,
        width,
        toolbar_rows,
        argb(243, 244, 246, 255),
    );
    fill_argb_rect(
        pixels,
        width,
        height,
        0,
        toolbar_rows - 1,
        width,
        1,
        argb(209, 213, 219, 255),
    );
}

fn rasterize_browser_viewport_argb_direct(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    pixels: &mut Vec<u32>,
) -> bool {
    let viewport = scroll_visible_document_rect(scroll_y, bitmap_height);
    let items = browser_viewport_source_items(display_list, viewport);
    if !viewport_argb_direct_items_supported(&items, viewport) {
        trace_viewport_argb_direct_miss(&items, viewport);
        return false;
    }
    resize_argb_words_uninit(pixels, FRAME_WIDTH as usize * bitmap_height as usize);
    if viewport_argb_direct_needs_default_fill(&items, viewport) {
        pixels.fill(argb(255, 255, 255, 255));
    }
    fill_browser_toolbar_background_argb(pixels, FRAME_WIDTH, bitmap_height);
    for item in items {
        if display_item_intersects_viewport(item, viewport) {
            paint_viewport_argb_direct_item(pixels, bitmap_height, item, scroll_y);
        }
    }
    true
}

fn rasterize_browser_document_damage_argb_direct(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    pixels: &mut [u32],
) -> bool {
    let viewport_damage = viewport_damage_rect(damage, scroll_y);
    let Some(clip) = pixel_rect_from_rect(viewport_damage, FRAME_WIDTH, bitmap_height) else {
        return true;
    };
    let items = browser_viewport_source_items(display_list, damage);
    if !viewport_argb_direct_items_supported(&items, damage) {
        trace_viewport_argb_direct_miss(&items, damage);
        return false;
    }
    if viewport_argb_direct_needs_default_fill(&items, damage) {
        fill_argb_rect(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            clip.x,
            clip.y,
            clip.width,
            clip.height,
            argb(255, 255, 255, 255),
        );
    }
    for item in items {
        if display_item_intersects_viewport(item, damage) {
            paint_viewport_argb_direct_item_clipped(pixels, bitmap_height, item, scroll_y, clip);
        }
    }
    true
}

fn viewport_argb_direct_needs_default_fill(
    items: &[&silksurf_render::DisplayItem],
    viewport: Rect,
) -> bool {
    for item in items {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        return !opaque_fill_covers_rect(item, viewport);
    }
    true
}

fn opaque_fill_covers_rect(item: &silksurf_render::DisplayItem, rect: Rect) -> bool {
    match item {
        silksurf_render::DisplayItem::SolidColor {
            rect: item_rect,
            color,
        } => color.a == 255 && rect_contains_rect(*item_rect, rect),
        silksurf_render::DisplayItem::RoundedRect {
            rect: item_rect,
            radii,
            color,
        } => {
            color.a == 255
                && radii.iter().all(|radius| *radius <= 0.0)
                && rect_contains_rect(*item_rect, rect)
        }
        _ => false,
    }
}

fn viewport_argb_direct_items_supported(
    items: &[&silksurf_render::DisplayItem],
    viewport: Rect,
) -> bool {
    items
        .iter()
        .filter(|item| display_item_intersects_viewport(item, viewport))
        .all(|item| viewport_argb_direct_item_supported(item))
}

fn viewport_argb_direct_item_supported(item: &silksurf_render::DisplayItem) -> bool {
    match item {
        silksurf_render::DisplayItem::SolidColor { color, .. } => color.a == 255,
        silksurf_render::DisplayItem::Text {
            text,
            font_size,
            color,
            ..
        } => color.a == 255 && page_bitmap_text_supported(text, *font_size),
        silksurf_render::DisplayItem::RoundedRect { radii, color, .. } => {
            color.a == 255 && radii.iter().all(|radius| *radius <= 0.0)
        }
        silksurf_render::DisplayItem::Image { image, .. } => image_has_full_rgba_argb(image),
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => false,
    }
}

fn trace_viewport_argb_direct_miss(items: &[&silksurf_render::DisplayItem], viewport: Rect) {
    if std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_none()
        && std::env::var_os("SILKSURF_TRACE_NAV_BUILD").is_none()
    {
        return;
    }
    let mut unsupported_text = 0usize;
    let mut unsupported_rounding = 0usize;
    let mut unsupported_alpha = 0usize;
    let mut unsupported_shadow = 0usize;
    let mut unsupported_gradient = 0usize;
    let mut unsupported_image = 0usize;
    let mut visible = 0usize;
    for item in items {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        visible += 1;
        match *item {
            silksurf_render::DisplayItem::SolidColor { color, .. } if color.a != 255 => {
                unsupported_alpha += 1;
            }
            silksurf_render::DisplayItem::Text {
                text,
                font_size,
                color,
                ..
            } if color.a != 255 || !page_bitmap_text_supported(text, *font_size) => {
                unsupported_text += 1;
            }
            silksurf_render::DisplayItem::RoundedRect { radii, color, .. } => {
                if color.a != 255 {
                    unsupported_alpha += 1;
                } else if radii.iter().any(|radius| *radius > 0.0) {
                    unsupported_rounding += 1;
                }
            }
            silksurf_render::DisplayItem::Image { image, .. }
                if !image_has_full_rgba_argb(image) =>
            {
                unsupported_image += 1;
            }
            silksurf_render::DisplayItem::BoxShadow { .. } => {
                unsupported_shadow += 1;
            }
            silksurf_render::DisplayItem::LinearGradient { .. } => {
                unsupported_gradient += 1;
            }
            _ => {}
        }
    }
    eprintln!(
        "[SilkSurf] argb direct miss: visible_items={visible} text={unsupported_text} rounding={unsupported_rounding} alpha={unsupported_alpha} shadow={unsupported_shadow} gradient={unsupported_gradient} image={unsupported_image}"
    );
}

fn paint_viewport_argb_direct_item(
    pixels: &mut [u32],
    bitmap_height: u32,
    item: &silksurf_render::DisplayItem,
    scroll_y: u32,
) {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, color }
        | silksurf_render::DisplayItem::RoundedRect { rect, color, .. } => {
            fill_shifted_argb_rect(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                css_color_to_argb(*color),
            );
        }
        silksurf_render::DisplayItem::Text {
            rect,
            text,
            font_size,
            color,
            ..
        } => {
            draw_shifted_argb_text(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                text,
                *font_size,
                *color,
            );
        }
        silksurf_render::DisplayItem::Image { rect, image } => {
            blit_shifted_argb_image(pixels, bitmap_height, *rect, scroll_y, image);
        }
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => {}
    }
}

fn paint_viewport_argb_direct_item_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    item: &silksurf_render::DisplayItem,
    scroll_y: u32,
    clip: PixelRect,
) {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, color }
        | silksurf_render::DisplayItem::RoundedRect { rect, color, .. } => {
            fill_shifted_argb_rect_clipped(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                css_color_to_argb(*color),
                clip,
            );
        }
        silksurf_render::DisplayItem::Text {
            rect,
            text,
            font_size,
            color,
            ..
        } => {
            draw_shifted_argb_text_clipped(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                text,
                *font_size,
                *color,
                clip,
            );
        }
        silksurf_render::DisplayItem::Image { rect, image } => {
            blit_shifted_argb_image_clipped(pixels, bitmap_height, *rect, scroll_y, image, clip);
        }
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => {}
    }
}

fn fill_shifted_argb_rect(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    color: u32,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    if let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) {
        fill_argb_rect(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            pixel_rect.x,
            pixel_rect.y,
            pixel_rect.width,
            pixel_rect.height,
            color,
        );
    }
}

fn fill_shifted_argb_rect_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    color: u32,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    let Some(pixel_rect) = pixel_rect_intersection(pixel_rect, clip) else {
        return;
    };
    fill_argb_rect(
        pixels,
        FRAME_WIDTH,
        bitmap_height,
        pixel_rect.x,
        pixel_rect.y,
        pixel_rect.width,
        pixel_rect.height,
        color,
    );
}

fn draw_shifted_argb_text(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    text: &str,
    font_size: f32,
    color: silksurf_css::Color,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    if let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) {
        let _ = draw_page_bitmap_text_clipped(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            shifted.x,
            shifted.y,
            text,
            font_size,
            css_color_to_argb(color),
            pixel_rect,
        );
    }
}

fn draw_shifted_argb_text_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    text: &str,
    font_size: f32,
    color: silksurf_css::Color,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let _ = draw_page_bitmap_text_clipped(
        pixels,
        FRAME_WIDTH,
        bitmap_height,
        shifted.x,
        shifted.y,
        text,
        font_size,
        css_color_to_argb(color),
        clip,
    );
}

fn blit_shifted_argb_image(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    image: &silksurf_render::ImageSurface,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(dst) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    blit_argb_image_rect(pixels, shifted, image, dst);
}

fn blit_shifted_argb_image_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    image: &silksurf_render::ImageSurface,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(dst) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    let Some(dst) = pixel_rect_intersection(dst, clip) else {
        return;
    };
    blit_argb_image_rect(pixels, shifted, image, dst);
}

fn blit_argb_image_rect(
    pixels: &mut [u32],
    shifted: Rect,
    image: &silksurf_render::ImageSurface,
    dst: PixelRect,
) {
    let dst_width = shifted.width.max(1.0);
    let dst_height = shifted.height.max(1.0);
    let surface_width = image.width as usize;
    for y in dst.y..dst.y + dst.height {
        let src_y = image_source_coord_argb(y as f32 - shifted.y, dst_height, image.height);
        for x in dst.x..dst.x + dst.width {
            let src_x = image_source_coord_argb(x as f32 - shifted.x, dst_width, image.width);
            let src = (src_y as usize * surface_width + src_x as usize) * 4;
            let dst = y as usize * FRAME_WIDTH as usize + x as usize;
            copy_image_pixel_argb(pixels, dst, &image.rgba, src);
        }
    }
}

fn image_has_full_rgba_argb(image: &silksurf_render::ImageSurface) -> bool {
    image.width > 0
        && image.height > 0
        && image.rgba.len() >= image.width as usize * image.height as usize * 4
}

fn image_source_coord_argb(dst_offset: f32, dst_extent: f32, src_extent: u32) -> u32 {
    let coord = (dst_offset.max(0.0) * src_extent as f32 / dst_extent).floor() as u32;
    coord.min(src_extent.saturating_sub(1))
}

fn copy_image_pixel_argb(pixels: &mut [u32], dst: usize, rgba: &[u8], src: usize) {
    if dst >= pixels.len() || src + 4 > rgba.len() {
        return;
    }
    let alpha = rgba[src + 3];
    if alpha == 255 {
        pixels[dst] = argb(rgba[src], rgba[src + 1], rgba[src + 2], 255);
        return;
    }
    pixels[dst] = blend_image_pixel_argb(pixels[dst], &rgba[src..src + 4]);
}

fn blend_image_pixel_argb(dst: u32, src: &[u8]) -> u32 {
    let alpha = u16::from(src[3]);
    let inv_alpha = 255 - alpha;
    let red = blend_argb_channel(src[0], (dst >> 16) as u8, alpha, inv_alpha);
    let green = blend_argb_channel(src[1], (dst >> 8) as u8, alpha, inv_alpha);
    let blue = blend_argb_channel(src[2], dst as u8, alpha, inv_alpha);
    argb(red, green, blue, 255)
}

fn blend_argb_channel(src: u8, dst: u8, alpha: u16, inv_alpha: u16) -> u8 {
    ((u16::from(src) * alpha + u16::from(dst) * inv_alpha + 127) / 255) as u8
}

fn viewport_damage_rect(damage: Rect, scroll_y: u32) -> Rect {
    Rect {
        x: damage.x,
        y: damage.y - scroll_y as f32,
        width: damage.width,
        height: damage.height,
    }
}

fn refresh_browser_frame_bitmap(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) -> BrowserBitmapRefresh {
    if state.frame.bitmap_scroll_y == scroll_y && state.frame.bitmap_height == bitmap_height {
        return BrowserBitmapRefresh::Clean;
    }
    if let Some(damage) = scroll_browser_frame_bitmap(state, scroll_y, bitmap_height) {
        return BrowserBitmapRefresh::ScrollReuse(damage);
    }
    let Some(runtime) = state.runtime.as_mut() else {
        return BrowserBitmapRefresh::Clean;
    };
    trace_visible_document_raster(
        &runtime.display_list,
        scroll_visible_document_rect(scroll_y, bitmap_height),
    );
    rasterize_browser_viewport_argb_preferred(
        &runtime.display_list,
        scroll_y,
        bitmap_height,
        &mut runtime.rgba,
        &mut state.frame.argb,
    );
    state.frame.bitmap_height = bitmap_height;
    state.frame.bitmap_scroll_y = scroll_y;
    BrowserBitmapRefresh::Full
}

fn scroll_browser_frame_bitmap(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) -> Option<Rect> {
    if state.frame.bitmap_height != bitmap_height || state.runtime.is_none() {
        return None;
    }
    let old_scroll_y = state.frame.bitmap_scroll_y;
    if old_scroll_y == scroll_y {
        return None;
    }
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    let content_rows = bitmap_height.saturating_sub(chrome_rows);
    let scroll_delta = i64::from(scroll_y) - i64::from(old_scroll_y);
    let delta_rows = scroll_delta.unsigned_abs() as u32;
    if !scroll_reuse_is_profitable(content_rows, delta_rows) {
        return None;
    }

    let runtime = state.runtime.as_mut()?;
    if !shift_browser_argb_content_rows(
        &mut state.frame.argb,
        FRAME_WIDTH,
        chrome_rows,
        content_rows,
        scroll_delta,
    ) {
        return None;
    }
    let exposed_damage = scroll_exposed_document_rect(scroll_y, bitmap_height, scroll_delta);
    if !rasterize_browser_document_damage_argb_direct(
        &runtime.display_list,
        scroll_y,
        bitmap_height,
        exposed_damage,
        &mut state.frame.argb,
    ) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            scroll_y,
            bitmap_height,
            exposed_damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        if !sync_argb_damage_from_scratch(
            &runtime.damage_scratch,
            &mut state.frame.argb,
            FRAME_WIDTH,
        ) {
            sync_argb_damage_from_rgba(
                &runtime.rgba,
                &mut state.frame.argb,
                FRAME_WIDTH,
                bitmap_height,
                viewport_damage_rect(exposed_damage, scroll_y),
            );
        }
    }
    state.frame.bitmap_scroll_y = scroll_y;
    Some(exposed_damage)
}

fn scroll_reuse_is_profitable(content_rows: u32, delta_rows: u32) -> bool {
    delta_rows > 0 && delta_rows < content_rows && delta_rows <= content_rows / 4
}

fn scroll_exposed_document_rect(scroll_y: u32, bitmap_height: u32, scroll_delta: i64) -> Rect {
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    let content_rows = bitmap_height.saturating_sub(chrome_rows);
    let delta_rows = scroll_delta.unsigned_abs().min(u64::from(content_rows)) as u32;
    let y = if scroll_delta > 0 {
        chrome_rows
            .saturating_add(scroll_y)
            .saturating_add(content_rows.saturating_sub(delta_rows))
    } else {
        chrome_rows.saturating_add(scroll_y)
    };
    Rect {
        x: 0.0,
        y: y as f32,
        width: FRAME_WIDTH as f32,
        height: delta_rows as f32,
    }
}

fn scroll_visible_document_rect(scroll_y: u32, bitmap_height: u32) -> Rect {
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    Rect {
        x: 0.0,
        y: chrome_rows.saturating_add(scroll_y) as f32,
        width: FRAME_WIDTH as f32,
        height: bitmap_height.saturating_sub(chrome_rows) as f32,
    }
}

fn shift_browser_argb_content_rows(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    scroll_delta: i64,
) -> bool {
    let delta_rows = scroll_delta.unsigned_abs() as u32;
    if width == 0 || delta_rows == 0 || delta_rows >= content_rows {
        return false;
    }
    let total_rows = chrome_rows.saturating_add(content_rows);
    if argb.len() < total_rows as usize * width as usize {
        return false;
    }
    if scroll_delta > 0 {
        copy_browser_argb_content_rows_up(argb, width, chrome_rows, content_rows, delta_rows);
    } else {
        copy_browser_argb_content_rows_down(argb, width, chrome_rows, content_rows, delta_rows);
    }
    true
}

fn copy_browser_argb_content_rows_up(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    delta_rows: u32,
) {
    let preserved_rows = content_rows - delta_rows;
    copy_argb_rows(
        argb,
        width,
        chrome_rows + delta_rows,
        chrome_rows,
        preserved_rows,
    );
}

fn copy_browser_argb_content_rows_down(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    delta_rows: u32,
) {
    let preserved_rows = content_rows - delta_rows;
    copy_argb_rows(
        argb,
        width,
        chrome_rows,
        chrome_rows + delta_rows,
        preserved_rows,
    );
}

fn copy_argb_rows(argb: &mut [u32], width: u32, source_y: u32, dest_y: u32, rows: u32) {
    let row_words = width as usize;
    let source_start = source_y as usize * row_words;
    let source_end = source_start + rows as usize * row_words;
    let dest_start = dest_y as usize * row_words;
    argb.copy_within(source_start..source_end, dest_start);
}

fn trace_browser_bitmap_refresh(
    enabled: bool,
    refresh: BrowserBitmapRefresh,
    elapsed: std::time::Duration,
) {
    if enabled && refresh != BrowserBitmapRefresh::Clean {
        eprintln!("[SilkSurf] bitmap refresh: {refresh:?} in {elapsed:?}");
    }
}

fn browser_viewport_display_list(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
) -> silksurf_render::DisplayList {
    let viewport = Rect {
        x: 0.0,
        y: BROWSER_CHROME_HEIGHT + scroll_y as f32,
        width: FRAME_WIDTH as f32,
        height: bitmap_height.saturating_sub(BROWSER_CHROME_HEIGHT as u32) as f32,
    };
    let mut items = Vec::with_capacity(display_list.items.len().min(256));
    for item in browser_viewport_source_items(display_list, viewport) {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        items.push(shift_display_item_y(item.clone(), -(scroll_y as f32)));
    }
    silksurf_render::DisplayList { items, tiles: None }
}

fn browser_viewport_source_items(
    display_list: &silksurf_render::DisplayList,
    viewport: Rect,
) -> Vec<&silksurf_render::DisplayItem> {
    let Some(tiles) = &display_list.tiles else {
        return display_list.items.iter().collect();
    };
    let mut indices = tiles.items_for_rect(viewport);
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .filter_map(|index| display_list.items.get(index))
        .collect()
}

fn display_item_intersects_viewport(item: &silksurf_render::DisplayItem, viewport: Rect) -> bool {
    rects_intersect(display_item_rect(item), viewport)
}

fn display_item_rect(item: &silksurf_render::DisplayItem) -> Rect {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => *rect,
        silksurf_render::DisplayItem::BoxShadow { rect, shadow } => Rect {
            x: rect.x + shadow.offset_x - shadow.spread_radius,
            y: rect.y + shadow.offset_y - shadow.spread_radius,
            width: rect.width + shadow.spread_radius * 2.0,
            height: rect.height + shadow.spread_radius * 2.0,
        },
    }
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    let ax1 = a.x + a.width;
    let ay1 = a.y + a.height;
    let bx1 = b.x + b.width;
    let by1 = b.y + b.height;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.x + outer.width >= inner.x + inner.width
        && outer.y + outer.height >= inner.y + inner.height
}

fn shift_display_item_y(
    mut item: silksurf_render::DisplayItem,
    delta_y: f32,
) -> silksurf_render::DisplayItem {
    match &mut item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::BoxShadow { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => {
            rect.y += delta_y;
        }
    }
    item
}

fn initial_browser_window_height(raster_height: u32) -> u32 {
    raster_height.clamp(MIN_INITIAL_WINDOW_HEIGHT, FRAME_HEIGHT)
}

fn window_size_exposes_unpainted_area(
    last_width: u32,
    last_height: u32,
    next_width: u32,
    next_height: u32,
) -> bool {
    last_width == 0 || last_height == 0 || next_width > last_width || next_height > last_height
}

fn display_item_bottom(item: &silksurf_render::DisplayItem) -> f32 {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::BoxShadow { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => rect.y + rect.height,
    }
}

fn max_browser_scroll_offset(frame_height: u32, window_height: u32, chrome_height: u32) -> f32 {
    let source_content_height = frame_height.saturating_sub(chrome_height);
    let window_content_height = window_height.saturating_sub(chrome_height);
    source_content_height.saturating_sub(window_content_height) as f32
}

fn scroll_to_show_input_target(
    current_scroll: f32,
    rect: Rect,
    max_scroll: f32,
    chrome_height: u32,
    window_height: u32,
) -> f32 {
    let viewport_top = current_scroll + chrome_height as f32;
    let viewport_bottom = current_scroll + window_height as f32;
    let target_top = rect.y;
    let target_bottom = rect.y + rect.height;
    let padding = 24.0;
    let next_scroll = if target_bottom + padding > viewport_bottom {
        target_bottom + padding - window_height as f32
    } else if target_top < viewport_top + padding {
        target_top - chrome_height as f32 - padding
    } else {
        current_scroll
    };
    clamp_scroll_offset(next_scroll, max_scroll)
}

fn clamp_scroll_offset(scroll: f32, max_scroll: f32) -> f32 {
    if !scroll.is_finite() {
        return 0.0;
    }
    scroll.clamp(0.0, max_scroll.max(0.0))
}

fn blit_browser_frame(
    frame: &[u32],
    frame_width: u32,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    if window_width == 0 || window_height == 0 {
        return;
    }
    let background = 0xFFFF_FFFF;
    if scroll_y == 0 && frame_width == window_width && frame_height == window_height {
        let pixel_count = frame_width.saturating_mul(frame_height) as usize;
        if frame.len() >= pixel_count && pixels.len() >= pixel_count {
            pixels[..pixel_count].copy_from_slice(&frame[..pixel_count]);
            return;
        }
    }

    let copy_width = frame_width.min(window_width);
    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    copy_frame_rows(
        frame,
        frame_width,
        0,
        pixels,
        window_width,
        0,
        copy_width,
        chrome_rows,
    );

    let content_rows = window_height.saturating_sub(chrome_rows);
    let source_y = chrome_height.saturating_add(scroll_y).min(frame_height);
    let available_rows = frame_height.saturating_sub(source_y);
    let rows = content_rows.min(available_rows);
    copy_frame_rows(
        frame,
        frame_width,
        source_y,
        pixels,
        window_width,
        chrome_rows,
        copy_width,
        rows,
    );
    let copied_rows = chrome_rows.saturating_add(rows).min(window_height);
    if copy_width < window_width {
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            copy_width,
            0,
            window_width - copy_width,
            copied_rows,
            background,
        );
    }
    if copied_rows < window_height {
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            0,
            copied_rows,
            window_width,
            window_height - copied_rows,
            background,
        );
    }
}

fn blit_browser_frame_damage(
    frame: &[u32],
    frame_width: u32,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
    damage: Rect,
    pixels: &mut [u32],
) {
    if damage.width <= 0.0 || damage.height <= 0.0 {
        return;
    }

    let x0 = damage.x.floor().max(0.0) as u32;
    let y0 = damage.y.floor().max(0.0) as u32;
    let x1 = (damage.x + damage.width)
        .ceil()
        .max(0.0)
        .min(frame_width as f32)
        .min(window_width as f32) as u32;
    let y1 = (damage.y + damage.height).ceil().max(0.0) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    if y0 < chrome_rows {
        let chrome_y1 = y1.min(chrome_rows);
        copy_frame_rect(
            frame,
            frame_width,
            x0,
            y0,
            pixels,
            window_width,
            x0,
            y0,
            x1 - x0,
            chrome_y1 - y0,
        );
    }

    let content_rows = window_height.saturating_sub(chrome_rows);
    let visible_source_y0 = chrome_height.saturating_add(scroll_y);
    let visible_source_y1 = visible_source_y0.saturating_add(content_rows);
    let source_y0 = y0.max(chrome_height).max(visible_source_y0);
    let source_y1 = y1.min(visible_source_y1);
    if source_y1 <= source_y0 {
        return;
    }
    let viewport_y = source_y0.saturating_sub(scroll_y);
    copy_frame_rect(
        frame,
        frame_width,
        x0,
        viewport_y,
        pixels,
        window_width,
        x0,
        viewport_y,
        x1 - x0,
        source_y1 - source_y0,
    );
}

fn browser_present_damage(
    redraw_mode: BrowserRedrawMode,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    match redraw_mode {
        BrowserRedrawMode::Clean | BrowserRedrawMode::Scroll => {
            silksurf_gui::WinitPresentDamage::Clean
        }
        BrowserRedrawMode::Full => silksurf_gui::WinitPresentDamage::Full,
        BrowserRedrawMode::AddressChrome | BrowserRedrawMode::AddressFocusChrome => {
            silksurf_gui::WinitPresentDamage::rect(
                ADDRESS_BAR_X,
                ADDRESS_BAR_Y,
                ADDRESS_BAR_WIDTH,
                ADDRESS_BAR_HEIGHT,
            )
        }
        BrowserRedrawMode::AddressFullTextChrome => silksurf_gui::WinitPresentDamage::rect(
            ADDRESS_BAR_X + 10,
            ADDRESS_BAR_Y + 7,
            ADDRESS_BAR_WIDTH - 22,
            ADDRESS_BAR_HEIGHT - 14,
        ),
        BrowserRedrawMode::AddressTextChrome => silksurf_gui::WinitPresentDamage::rect(
            ADDRESS_BAR_X + 10,
            ADDRESS_BAR_Y + 7,
            ADDRESS_BAR_WIDTH - 22,
            ADDRESS_BAR_HEIGHT - 14,
        ),
        BrowserRedrawMode::StatusChrome => {
            browser_status_present_damage(window_width, window_height)
        }
        BrowserRedrawMode::NavigationStartChrome => {
            browser_navigation_start_present_damage(window_width, window_height)
        }
        BrowserRedrawMode::Chrome => {
            silksurf_gui::WinitPresentDamage::rect(0, 0, window_width, chrome_height)
        }
        BrowserRedrawMode::Damage(damage) => browser_content_damage_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
        BrowserRedrawMode::PageInputFocus(damage) => browser_content_damage_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
        BrowserRedrawMode::DamageWithChrome(damage) => browser_content_damage_with_chrome_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
    }
}

fn browser_render_seeds_full_buffer(redraw_mode: BrowserRedrawMode, buffer_age: u8) -> bool {
    buffer_age == 0
        && matches!(
            redraw_mode,
            BrowserRedrawMode::Damage(_)
                | BrowserRedrawMode::PageInputFocus(_)
                | BrowserRedrawMode::DamageWithChrome(_)
                | BrowserRedrawMode::AddressChrome
                | BrowserRedrawMode::AddressFocusChrome
                | BrowserRedrawMode::AddressFullTextChrome
                | BrowserRedrawMode::AddressTextChrome
                | BrowserRedrawMode::NavigationStartChrome
                | BrowserRedrawMode::StatusChrome
                | BrowserRedrawMode::Chrome
        )
}

fn browser_status_present_damage(
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    let Some(rect) = browser_status_text_band_rect(window_width, window_height) else {
        return silksurf_gui::WinitPresentDamage::Clean;
    };
    silksurf_gui::WinitPresentDamage::Rect(rect)
}

fn browser_navigation_start_present_damage(
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    silksurf_gui::WinitPresentDamage::rects(&[
        navigation_button_present_damage_rect(RELOAD_BUTTON_X, window_width, window_height),
        navigation_button_present_damage_rect(STOP_BUTTON_X, window_width, window_height),
        browser_status_text_band_rect(window_width, window_height).unwrap_or_else(zero_damage_rect),
    ])
}

fn zero_damage_rect() -> silksurf_gui::WinitDamageRect {
    silksurf_gui::WinitDamageRect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

fn navigation_button_present_damage_rect(
    x: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitDamageRect {
    silksurf_gui::WinitDamageRect {
        x,
        y: NAV_BUTTON_Y,
        width: window_width.saturating_sub(x).min(NAV_BUTTON_WIDTH),
        height: window_height
            .saturating_sub(NAV_BUTTON_Y)
            .min(NAV_BUTTON_HEIGHT),
    }
}

fn browser_content_damage_with_chrome_rect(
    damage: Rect,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    let mut output = match browser_content_damage_rect(
        damage,
        frame_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    ) {
        silksurf_gui::WinitPresentDamage::Clean => None,
        silksurf_gui::WinitPresentDamage::Full => return silksurf_gui::WinitPresentDamage::Full,
        silksurf_gui::WinitPresentDamage::Rect(rect) => Some(rect),
        silksurf_gui::WinitPresentDamage::Rects(rects) => {
            let mut output = None;
            for rect in rects.as_slice() {
                union_present_damage_rect(&mut output, *rect);
            }
            output
        }
    };
    union_present_damage_rect(
        &mut output,
        silksurf_gui::WinitDamageRect {
            x: 0,
            y: 0,
            width: window_width,
            height: chrome_height.min(window_height),
        },
    );
    output.map_or(
        silksurf_gui::WinitPresentDamage::Clean,
        silksurf_gui::WinitPresentDamage::Rect,
    )
}

fn browser_content_damage_rect(
    damage: Rect,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    if damage.width <= 0.0 || damage.height <= 0.0 || window_width == 0 || window_height == 0 {
        return silksurf_gui::WinitPresentDamage::Clean;
    }

    let x0 = damage.x.floor().max(0.0).min(window_width as f32) as u32;
    let x1 = (damage.x + damage.width)
        .ceil()
        .max(0.0)
        .min(window_width as f32) as u32;
    let y0 = damage.y.floor().max(0.0) as u32;
    let y1 = (damage.y + damage.height)
        .ceil()
        .max(0.0)
        .min(frame_height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return silksurf_gui::WinitPresentDamage::Clean;
    }

    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    let mut output: Option<silksurf_gui::WinitDamageRect> = None;
    if y0 < chrome_rows {
        let chrome_y1 = y1.min(chrome_rows);
        union_present_damage_rect(
            &mut output,
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: chrome_y1 - y0,
            },
        );
    }

    let content_rows = window_height.saturating_sub(chrome_rows);
    let visible_source_y0 = chrome_height.saturating_add(scroll_y);
    let visible_source_y1 = visible_source_y0
        .saturating_add(content_rows)
        .min(frame_height);
    let source_y0 = y0.max(chrome_height).max(visible_source_y0);
    let source_y1 = y1.min(visible_source_y1);
    if source_y1 > source_y0 {
        union_present_damage_rect(
            &mut output,
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: chrome_rows.saturating_add(source_y0.saturating_sub(visible_source_y0)),
                width: x1 - x0,
                height: source_y1 - source_y0,
            },
        );
    }

    match output {
        Some(rect) => silksurf_gui::WinitPresentDamage::Rect(rect),
        None => silksurf_gui::WinitPresentDamage::Clean,
    }
}

fn union_present_damage_rect(
    output: &mut Option<silksurf_gui::WinitDamageRect>,
    rect: silksurf_gui::WinitDamageRect,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    *output = Some(match *output {
        None => rect,
        Some(existing) => {
            let x0 = existing.x.min(rect.x);
            let y0 = existing.y.min(rect.y);
            let x1 = existing
                .x
                .saturating_add(existing.width)
                .max(rect.x.saturating_add(rect.width));
            let y1 = existing
                .y
                .saturating_add(existing.height)
                .max(rect.y.saturating_add(rect.height));
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: y0,
                width: x1.saturating_sub(x0),
                height: y1.saturating_sub(y0),
            }
        }
    });
}

fn copy_frame_rows(
    frame: &[u32],
    frame_width: u32,
    source_y: u32,
    pixels: &mut [u32],
    window_width: u32,
    dest_y: u32,
    copy_width: u32,
    rows: u32,
) {
    if frame_width == 0 || window_width == 0 || copy_width == 0 || rows == 0 {
        return;
    }

    let frame_stride = frame_width as usize;
    let window_stride = window_width as usize;
    let copy_width = copy_width as usize;
    if copy_width == frame_stride && copy_width == window_stride {
        let frame_start = source_y as usize * frame_stride;
        let frame_end = frame_start + rows as usize * frame_stride;
        let window_start = dest_y as usize * window_stride;
        let window_end = window_start + rows as usize * window_stride;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
        return;
    }
    for row in 0..rows as usize {
        let frame_start = (source_y as usize + row) * frame_stride;
        let frame_end = frame_start + copy_width;
        let window_start = (dest_y as usize + row) * window_stride;
        let window_end = window_start + copy_width;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
    }
}

fn copy_frame_rect(
    frame: &[u32],
    frame_width: u32,
    source_x: u32,
    source_y: u32,
    pixels: &mut [u32],
    window_width: u32,
    dest_x: u32,
    dest_y: u32,
    copy_width: u32,
    rows: u32,
) {
    if frame_width == 0 || window_width == 0 || copy_width == 0 || rows == 0 {
        return;
    }

    let frame_stride = frame_width as usize;
    let window_stride = window_width as usize;
    let source_x = source_x as usize;
    let dest_x = dest_x as usize;
    let copy_width = copy_width as usize;
    if source_x == 0 && dest_x == 0 && copy_width == frame_stride && copy_width == window_stride {
        let frame_start = source_y as usize * frame_stride;
        let frame_end = frame_start + rows as usize * frame_stride;
        let window_start = dest_y as usize * window_stride;
        let window_end = window_start + rows as usize * window_stride;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
        return;
    }
    for row in 0..rows as usize {
        let frame_start = (source_y as usize + row) * frame_stride + source_x;
        let frame_end = frame_start + copy_width;
        let window_start = (dest_y as usize + row) * window_stride + dest_x;
        let window_end = window_start + copy_width;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
    }
}

fn draw_browser_chrome_overlays(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_buttons(state, pixels, window_width, window_height);
    draw_browser_status_from_state(state, pixels, window_width, window_height);
    draw_browser_address_from_state(state, pixels, window_width, window_height);
}

fn draw_browser_navigation_start_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_start_buttons(state, pixels, window_width, window_height);
    draw_browser_status_from_state(state, pixels, window_width, window_height);
}

fn browser_status_text(state: &BrowserState) -> &str {
    state
        .hover_status_text
        .as_deref()
        .unwrap_or(state.status_text.as_str())
}

fn set_browser_status(state: &mut BrowserState, status: impl Into<String>) {
    state.status_text = status.into();
    state.hover_status_text = None;
}

fn draw_browser_status_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_status(
        pixels,
        window_width,
        window_height,
        browser_status_text(state),
    );
}

fn draw_browser_navigation_buttons(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    for action in [
        BrowserChromeAction::Back,
        BrowserChromeAction::Forward,
        BrowserChromeAction::Home,
        BrowserChromeAction::Reload,
        BrowserChromeAction::Stop,
    ] {
        draw_browser_navigation_button(
            pixels,
            window_width,
            window_height,
            chrome_action_button_x(action),
            chrome_action_label(action),
            chrome_action_enabled(state, action),
        );
    }
}

fn draw_browser_navigation_start_buttons(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    for action in [BrowserChromeAction::Reload, BrowserChromeAction::Stop] {
        draw_browser_navigation_button(
            pixels,
            window_width,
            window_height,
            chrome_action_button_x(action),
            chrome_action_label(action),
            chrome_action_enabled(state, action),
        );
    }
}

fn draw_navigation_start_retained_chrome(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_button(
        pixels,
        window_width,
        window_height,
        RELOAD_BUTTON_X,
        chrome_action_label(BrowserChromeAction::Reload),
        false,
    );
    draw_browser_navigation_button(
        pixels,
        window_width,
        window_height,
        STOP_BUTTON_X,
        chrome_action_label(BrowserChromeAction::Stop),
        true,
    );
    draw_browser_status(pixels, window_width, window_height, "loading");
}

fn chrome_action_button_x(action: BrowserChromeAction) -> u32 {
    match action {
        BrowserChromeAction::Back => BACK_BUTTON_X,
        BrowserChromeAction::Forward => FORWARD_BUTTON_X,
        BrowserChromeAction::Home => HOME_BUTTON_X,
        BrowserChromeAction::Reload => RELOAD_BUTTON_X,
        BrowserChromeAction::Stop => STOP_BUTTON_X,
    }
}

fn chrome_action_label(action: BrowserChromeAction) -> &'static str {
    match action {
        BrowserChromeAction::Back => "B",
        BrowserChromeAction::Forward => "F",
        BrowserChromeAction::Home => "H",
        BrowserChromeAction::Reload => "R",
        BrowserChromeAction::Stop => "S",
    }
}

fn draw_browser_navigation_button(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    label: &str,
    enabled: bool,
) {
    let fill = if enabled {
        argb(229, 231, 235, 255)
    } else {
        argb(243, 244, 246, 255)
    };
    let label_color = if enabled {
        argb(31, 41, 55, 255)
    } else {
        argb(156, 163, 175, 255)
    };
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        x,
        NAV_BUTTON_Y,
        NAV_BUTTON_WIDTH,
        NAV_BUTTON_HEIGHT,
        fill,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        x,
        NAV_BUTTON_Y,
        NAV_BUTTON_WIDTH,
        1,
        argb(209, 213, 219, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        x.saturating_add(4),
        NAV_BUTTON_Y.saturating_add(10),
        label,
        x.saturating_add(NAV_BUTTON_WIDTH),
        label_color,
    );
}

fn draw_browser_address_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_overlay(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
        state.address_editing,
    );
}

fn draw_browser_address_text_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_text_strip(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

fn draw_browser_address_focus_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_focus_overlay(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

fn draw_browser_address_full_text_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_full_text_strip(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

fn address_cursor_for_state(state: &BrowserState, text: &str) -> usize {
    if state.address_editing {
        clamp_address_cursor(text, state.address_cursor)
    } else {
        text.len()
    }
}

fn draw_browser_address_overlay(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
    editing: bool,
) {
    let fill = argb(255, 255, 255, 255);
    let border = if editing {
        argb(37, 99, 235, 255)
    } else {
        argb(209, 213, 219, 255)
    };
    fill_address_bar_box(pixels, window_width, window_height, fill, border);
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
        argb(31, 41, 55, 255),
    );
    if editing {
        let cursor_x = bitmap_text_prefix_end_x(
            text_x,
            text,
            cursor_byte,
            ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
        );
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            cursor_x.saturating_add(1),
            ADDRESS_BAR_Y + 7,
            1,
            ADDRESS_BAR_HEIGHT - 14,
            argb(31, 41, 55, 255),
        );
    }
}

fn draw_browser_address_focus_overlay(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    fill_address_bar_border(pixels, window_width, window_height, argb(37, 99, 235, 255));
    let cursor_x = bitmap_text_prefix_end_x(
        ADDRESS_BAR_X + 10,
        text,
        cursor_byte,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        ADDRESS_BAR_Y + 7,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

fn draw_browser_address_text_strip(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    let strip_y = ADDRESS_BAR_Y + 7;
    let text_max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;
    let cursor_x = bitmap_text_prefix_end_x(text_x, text, cursor_byte, text_max_x);
    let text_end_x = bitmap_text_prefix_end_x(text_x, text, text.len(), text_max_x);
    let strip_end_x = text_end_x
        .max(cursor_x.saturating_add(2))
        .saturating_add(6)
        .min(text_max_x);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        strip_y,
        strip_end_x.saturating_sub(text_x),
        ADDRESS_BAR_HEIGHT - 14,
        argb(255, 255, 255, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        text_max_x,
        argb(31, 41, 55, 255),
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        strip_y,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

fn draw_browser_address_full_text_strip(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    let strip_y = ADDRESS_BAR_Y + 7;
    let text_max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;
    let cursor_x = bitmap_text_prefix_end_x(text_x, text, cursor_byte, text_max_x);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        strip_y,
        text_max_x.saturating_sub(text_x),
        ADDRESS_BAR_HEIGHT - 14,
        argb(255, 255, 255, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        text_max_x,
        argb(31, 41, 55, 255),
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        strip_y,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

fn fill_address_bar_box(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    fill: u32,
    border: u32,
) {
    if window_width == 0 || window_height == 0 {
        return;
    }
    let x0 = ADDRESS_BAR_X.min(window_width);
    let x1 = ADDRESS_BAR_X
        .saturating_add(ADDRESS_BAR_WIDTH)
        .min(window_width);
    let y0 = ADDRESS_BAR_Y.min(window_height);
    let y1 = ADDRESS_BAR_Y
        .saturating_add(ADDRESS_BAR_HEIGHT)
        .min(window_height);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let stride = window_width as usize;
    let x0_usize = x0 as usize;
    let x1_usize = x1 as usize;
    let left_x = ADDRESS_BAR_X;
    let right_x = ADDRESS_BAR_X.saturating_add(ADDRESS_BAR_WIDTH - 1);
    let bottom_y = ADDRESS_BAR_Y.saturating_add(ADDRESS_BAR_HEIGHT - 1);
    for y in y0..y1 {
        let row_start = y as usize * stride + x0_usize;
        let row_end = y as usize * stride + x1_usize;
        if row_end > pixels.len() {
            return;
        }
        if y == ADDRESS_BAR_Y || y == bottom_y {
            pixels[row_start..row_end].fill(border);
            continue;
        }
        pixels[row_start..row_end].fill(fill);
        if left_x >= x0 && left_x < x1 {
            pixels[y as usize * stride + left_x as usize] = border;
        }
        if right_x >= x0 && right_x < x1 {
            pixels[y as usize * stride + right_x as usize] = border;
        }
    }
}

fn fill_address_bar_border(pixels: &mut [u32], window_width: u32, window_height: u32, color: u32) {
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y,
        ADDRESS_BAR_WIDTH,
        1,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y + ADDRESS_BAR_HEIGHT - 1,
        ADDRESS_BAR_WIDTH,
        1,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y,
        1,
        ADDRESS_BAR_HEIGHT,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 1,
        ADDRESS_BAR_Y,
        1,
        ADDRESS_BAR_HEIGHT,
        color,
    );
}

fn draw_browser_status(pixels: &mut [u32], window_width: u32, window_height: u32, status: &str) {
    let Some((x, y, width, height)) = browser_status_rect(window_width, window_height) else {
        return;
    };
    let text_x = x.saturating_add(10);
    let text_y = y.saturating_add(6);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        width.saturating_sub(10),
        height.saturating_sub(6).min(7),
        argb(243, 244, 246, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        status,
        x.saturating_add(160),
        argb(75, 85, 99, 255),
    );
}

fn browser_status_rect(window_width: u32, window_height: u32) -> Option<(u32, u32, u32, u32)> {
    if window_width == 0 || window_height == 0 {
        return None;
    }
    let x = 1000_u32.min(window_width.saturating_sub(1));
    let y = 8_u32.min(window_height.saturating_sub(1));
    let width = window_width.saturating_sub(x).min(170);
    let height = 28_u32.min(window_height.saturating_sub(y));
    (width > 0 && height > 0).then_some((x, y, width, height))
}

fn browser_status_text_band_rect(
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitDamageRect> {
    let (x, y, width, height) = browser_status_rect(window_width, window_height)?;
    let text_x = x.saturating_add(10);
    let text_y = y.saturating_add(6);
    let width = width.saturating_sub(10);
    let height = height.saturating_sub(6).min(7);
    (width > 0 && height > 0).then_some(silksurf_gui::WinitDamageRect {
        x: text_x,
        y: text_y,
        width,
        height,
    })
}

fn draw_bitmap_text(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    y: u32,
    text: &str,
    max_x: u32,
    color: u32,
) -> u32 {
    let mut cursor_x = x;
    for byte in text.bytes() {
        if cursor_x.saturating_add(5) > max_x {
            break;
        }
        draw_bitmap_byte(
            pixels,
            window_width,
            window_height,
            cursor_x,
            y,
            byte,
            color,
        );
        cursor_x = cursor_x.saturating_add(6);
    }
    cursor_x
}

fn bitmap_text_prefix_end_x(x: u32, text: &str, cursor_byte: usize, max_x: u32) -> u32 {
    let cursor_byte = clamp_address_cursor(text, cursor_byte);
    let mut cursor_x = x;
    for (index, _) in text.char_indices() {
        if index >= cursor_byte || cursor_x.saturating_add(5) > max_x {
            break;
        }
        cursor_x = cursor_x.saturating_add(6);
    }
    cursor_x
}

fn draw_bitmap_byte(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    y: u32,
    byte: u8,
    color: u32,
) {
    let Some(glyph) = chrome_glyph_byte(byte) else {
        return;
    };
    if x.saturating_add(5) <= window_width && y.saturating_add(7) <= window_height {
        let stride = window_width as usize;
        let base_x = x as usize;
        let base_y = y as usize;
        for (row, bits) in glyph.iter().enumerate() {
            let row_start = (base_y + row) * stride + base_x;
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    pixels[row_start + col] = color;
                }
            }
        }
        return;
    }

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 1 {
                put_argb_pixel(
                    pixels,
                    window_width,
                    window_height,
                    x + col,
                    y + row as u32,
                    color,
                );
            }
        }
    }
}

fn draw_page_bitmap_text_clipped(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    text: &str,
    font_size: f32,
    color: u32,
    clip: PixelRect,
) -> bool {
    let Some((scale, advance, line_height, space_advance)) = page_bitmap_text_metrics(font_size)
    else {
        return false;
    };
    let mut cursor_x = x.round() as i32;
    let mut cursor_y = y.round() as i32;
    let line_origin_x = cursor_x;
    for ch in text.chars() {
        match ch {
            '\n' => {
                cursor_x = line_origin_x;
                cursor_y = cursor_y.saturating_add(line_height);
            }
            '\r' => {}
            '\t' => {
                cursor_x = cursor_x.saturating_add(space_advance.saturating_mul(4));
            }
            ' ' => {
                cursor_x = cursor_x.saturating_add(space_advance);
            }
            _ => {
                if !ch.is_ascii() {
                    return false;
                }
                let Some(glyph) = chrome_glyph_byte(ch as u8) else {
                    return false;
                };
                draw_page_bitmap_glyph_clipped(
                    pixels, width, height, cursor_x, cursor_y, scale, glyph, color, clip,
                );
                cursor_x = cursor_x.saturating_add(advance);
            }
        }
    }
    true
}

fn page_bitmap_text_metrics(font_size: f32) -> Option<(i32, i32, i32, i32)> {
    if !font_size.is_finite() || font_size <= 0.0 {
        return None;
    }
    Some((
        ((font_size / 12.0).round() as i32).max(1),
        (font_size * 0.55).round().max(6.0) as i32,
        (font_size * 1.2).round().max(8.0) as i32,
        (font_size * 0.33).round().max(3.0) as i32,
    ))
}

fn draw_page_bitmap_glyph_clipped(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    scale: i32,
    glyph: [u8; 7],
    color: u32,
    clip: PixelRect,
) {
    let clip_x1 = clip.x.saturating_add(clip.width);
    let clip_y1 = clip.y.saturating_add(clip.height);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let pixel_x = x + col * scale + dx;
                    let pixel_y = y + row as i32 * scale + dy;
                    if pixel_x < clip.x as i32
                        || pixel_y < clip.y as i32
                        || pixel_x >= clip_x1 as i32
                        || pixel_y >= clip_y1 as i32
                    {
                        continue;
                    }
                    put_argb_pixel(pixels, width, height, pixel_x as u32, pixel_y as u32, color);
                }
            }
        }
    }
}

const CHROME_GLYPHS: &[(u8, [u8; 7])] = &[
    (b' ', [0, 0, 0, 0, 0, 0, 0]),
    (
        b'!',
        [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    ),
    (
        b'"',
        [
            0b01010, 0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'#',
        [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
    ),
    (
        b'$',
        [
            0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
        ],
    ),
    (
        b'%',
        [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
    ),
    (
        b'&',
        [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        b'\'',
        [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'(',
        [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
    ),
    (
        b')',
        [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
    ),
    (
        b'*',
        [
            0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000,
        ],
    ),
    (
        b'+',
        [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
    ),
    (
        b',',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'-',
        [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'.',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
    ),
    (
        b'/',
        [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
    ),
    (
        b'0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        b'1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        b'2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        b'3',
        [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b'4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        b'5',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b'6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        b'8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (
        b':',
        [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
    ),
    (
        b';',
        [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'<',
        [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
    ),
    (
        b'=',
        [
            0b00000, 0b11111, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
    ),
    (
        b'>',
        [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
    ),
    (
        b'?',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
    ),
    (
        b'@',
        [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
    ),
    (
        b'[',
        [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
    ),
    (
        b'\\',
        [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
    ),
    (
        b']',
        [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
    ),
    (
        b'^',
        [
            0b00100, 0b01010, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'_',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
    ),
    (
        b'`',
        [
            0b01000, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'a',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'b',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        b'c',
        [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
    ),
    (
        b'd',
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        b'e',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        b'f',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        b'g',
        [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        b'h',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'i',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
    ),
    (
        b'j',
        [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'k',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        b'l',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        b'm',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'n',
        [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'o',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'p',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        b'q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        b'r',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        b's',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b't',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'u',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'v',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        b'w',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
    ),
    (
        b'x',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        b'y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        b'{',
        [
            0b00010, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00010,
        ],
    ),
    (
        b'|',
        [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'}',
        [
            0b01000, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'~',
        [
            0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000,
        ],
    ),
];

fn chrome_glyph_byte(byte: u8) -> Option<[u8; 7]> {
    let ascii = byte.to_ascii_lowercase();
    CHROME_GLYPHS
        .iter()
        .find_map(|(candidate, glyph)| (*candidate == ascii).then_some(*glyph))
}

fn fill_argb_rect(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    rect_width: u32,
    rect_height: u32,
    color: u32,
) {
    if width == 0 || height == 0 || rect_width == 0 || rect_height == 0 {
        return;
    }
    let x_end = x.saturating_add(rect_width).min(width);
    let y_end = y.saturating_add(rect_height).min(height);
    if x == 0 && x_end == width {
        let start = y as usize * width as usize;
        let end = y_end as usize * width as usize;
        if end <= pixels.len() {
            pixels[start..end].fill(color);
        }
        return;
    }
    for row in y..y_end {
        let start = row as usize * width as usize + x as usize;
        let end = row as usize * width as usize + x_end as usize;
        if end <= pixels.len() {
            pixels[start..end].fill(color);
        }
    }
}

fn put_argb_pixel(pixels: &mut [u32], width: u32, height: u32, x: u32, y: u32, color: u32) {
    if x >= width || y >= height {
        return;
    }
    let index = y as usize * width as usize + x as usize;
    if index < pixels.len() {
        pixels[index] = color;
    }
}

fn argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

fn css_color_to_argb(color: silksurf_css::Color) -> u32 {
    argb(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use silksurf_dom::{NodeId, NodeKind};
    use silksurf_render::DisplayItem;

    #[test]
    fn document_css_follows_user_agent_defaults() {
        let css = stylesheet_text_with_user_agent_defaults("body { margin: 0; }");
        let ua_pos = css.find("body {").expect("ua body rule");
        let document_pos = css
            .rfind("body { margin: 0; }")
            .expect("document body rule");

        assert!(ua_pos < document_pos);
    }

    #[test]
    fn viewport_argb_direct_paints_supported_items() {
        let image_rgba: Arc<[u8]> = Arc::from(vec![255, 0, 0, 255].into_boxed_slice());
        let display_list = silksurf_render::DisplayList {
            items: vec![
                DisplayItem::SolidColor {
                    rect: Rect {
                        x: 0.0,
                        y: BROWSER_CHROME_HEIGHT,
                        width: 32.0,
                        height: 32.0,
                    },
                    color: rgba(1, 2, 3, 255),
                },
                DisplayItem::Text {
                    rect: Rect {
                        x: 2.0,
                        y: BROWSER_CHROME_HEIGHT + 2.0,
                        width: 64.0,
                        height: 16.0,
                    },
                    node: silksurf_dom::NodeId::from_raw(1),
                    text_len: 2,
                    text: "ok".to_string(),
                    font_size: 12.0,
                    color: rgba(255, 255, 255, 255),
                },
                DisplayItem::Image {
                    rect: Rect {
                        x: 8.0,
                        y: BROWSER_CHROME_HEIGHT + 8.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    image: silksurf_render::ImageSurface {
                        width: 1,
                        height: 1,
                        rgba: image_rgba,
                    },
                },
            ],
            tiles: None,
        };
        let mut pixels = Vec::new();

        assert!(rasterize_browser_viewport_argb_direct(
            &display_list,
            0,
            96,
            &mut pixels
        ));
        assert_eq!(pixels.len(), FRAME_WIDTH as usize * 96);
        assert_eq!(
            pixels[BROWSER_CHROME_HEIGHT as usize * FRAME_WIDTH as usize],
            argb(1, 2, 3, 255)
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 8) * FRAME_WIDTH as usize + 8],
            argb(255, 0, 0, 255)
        );
    }

    #[test]
    fn viewport_argb_direct_rejects_unsupported_items() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut pixels = vec![0x12345678];

        assert!(!rasterize_browser_viewport_argb_direct(
            &display_list,
            0,
            96,
            &mut pixels
        ));
        assert_eq!(pixels, vec![0x12345678]);
    }

    #[test]
    fn viewport_argb_preferred_keeps_rgba_empty_on_direct_hit() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: FRAME_WIDTH as f32,
                    height: 52.0,
                },
                color: rgba(7, 8, 9, 255),
            }],
            tiles: None,
        };
        let mut rgba = Vec::new();
        let mut argb = Vec::new();

        assert!(rasterize_browser_viewport_argb_preferred(
            &display_list,
            0,
            96,
            &mut rgba,
            &mut argb
        ));
        assert!(rgba.is_empty());
        assert_eq!(argb.len(), FRAME_WIDTH as usize * 96);
    }

    #[test]
    fn viewport_argb_preferred_falls_back_for_gradient() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut rgba = Vec::new();
        let mut argb = Vec::new();

        assert!(!rasterize_browser_viewport_argb_preferred(
            &display_list,
            0,
            96,
            &mut rgba,
            &mut argb
        ));
        assert_eq!(rgba.len(), FRAME_WIDTH as usize * 96 * 4);
        assert_eq!(argb.len(), FRAME_WIDTH as usize * 96);
    }

    #[test]
    fn document_damage_argb_direct_paints_clipped_strip() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: FRAME_WIDTH as f32,
                    height: 96.0,
                },
                color: rgba(11, 12, 13, 255),
            }],
            tiles: None,
        };
        let mut pixels = vec![0; FRAME_WIDTH as usize * 96];
        let damage = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT + 12.0,
            width: FRAME_WIDTH as f32,
            height: 4.0,
        };

        assert!(rasterize_browser_document_damage_argb_direct(
            &display_list,
            0,
            96,
            damage,
            &mut pixels
        ));
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 11) * FRAME_WIDTH as usize],
            0
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 12) * FRAME_WIDTH as usize],
            argb(11, 12, 13, 255)
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 16) * FRAME_WIDTH as usize],
            0
        );
    }

    #[test]
    fn document_damage_argb_direct_rejects_gradient() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut pixels = vec![0x12345678; FRAME_WIDTH as usize * 96];
        let damage = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: 32.0,
            height: 32.0,
        };

        assert!(!rasterize_browser_document_damage_argb_direct(
            &display_list,
            0,
            96,
            damage,
            &mut pixels
        ));
        assert!(pixels.iter().all(|pixel| *pixel == 0x12345678));
    }

    #[test]
    fn display_backend_arg_accepts_auto_wayland_and_x11() {
        assert_eq!(
            parse_display_backend_arg(&args(&["silksurf-app"])).unwrap(),
            silksurf_gui::WinitDisplayBackend::Auto
        );
        assert_eq!(
            parse_display_backend_arg(&args(&["silksurf-app", "--display-backend", "wayland"]))
                .unwrap(),
            silksurf_gui::WinitDisplayBackend::Wayland
        );
        assert_eq!(
            parse_display_backend_arg(&args(&["silksurf-app", "--display-backend=x11"])).unwrap(),
            silksurf_gui::WinitDisplayBackend::X11
        );
        assert!(
            parse_display_backend_arg(&args(&["silksurf-app", "--display-backend", "quartz"]))
                .is_err()
        );
    }

    #[test]
    fn positional_url_skips_option_values() {
        assert_eq!(
            positional_url_arg(&args(&[
                "silksurf-app",
                "--backend",
                "winit",
                "--display-backend",
                "wayland",
                "https://example.com/"
            ])),
            Some("https://example.com/".to_string())
        );
        assert_eq!(
            positional_url_arg(&args(&[
                "silksurf-app",
                "--backend=winit",
                "--display-backend=x11"
            ])),
            None
        );
    }

    #[test]
    fn browser_frame_blit_keeps_chrome_fixed_while_scrolling() {
        let frame_width = 3;
        let frame_height = 5;
        let chrome_height = 1;
        let frame = vec![
            10, 10, 10, // chrome
            20, 20, 20, // content row 0
            30, 30, 30, // content row 1
            40, 40, 40, // content row 2
            50, 50, 50, // content row 3
        ];
        let mut pixels = vec![0; 3 * 3];

        blit_browser_frame(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            2,
            3,
            3,
            &mut pixels,
        );

        assert_eq!(
            pixels,
            vec![
                10, 10, 10, // chrome remains pinned
                40, 40, 40, // content starts at chrome + scroll
                50, 50, 50,
            ]
        );
    }

    #[test]
    fn browser_frame_blit_fills_uncovered_window_margin() {
        let frame = vec![0xFF00_0001; 2 * 2];
        let mut pixels = vec![0x1234_5678; 3 * 3];

        blit_browser_frame(&frame, 2, 2, 0, 0, 3, 3, &mut pixels);

        assert_eq!(pixels[0], 0xFF00_0001);
        assert_eq!(pixels[1], 0xFF00_0001);
        assert_eq!(pixels[2], 0xFFFF_FFFF);
        assert_eq!(pixels[3], 0xFF00_0001);
        assert_eq!(pixels[4], 0xFF00_0001);
        assert_eq!(pixels[5], 0xFFFF_FFFF);
        assert_eq!(pixels[6], 0xFFFF_FFFF);
        assert_eq!(pixels[7], 0xFFFF_FFFF);
        assert_eq!(pixels[8], 0xFFFF_FFFF);
    }

    #[test]
    fn browser_frame_blit_copies_same_size_frame_contiguously() {
        let frame = vec![
            0xFF00_0001,
            0xFF00_0002,
            0xFF00_0003,
            0xFF00_0004,
            0xFF00_0005,
            0xFF00_0006,
        ];
        let mut pixels = vec![0xFFFF_FFFF; frame.len()];

        blit_browser_frame(&frame, 3, 2, 1, 0, 3, 2, &mut pixels);

        assert_eq!(pixels, frame);
    }

    #[test]
    fn browser_frame_height_allows_short_pages() {
        let items = vec![DisplayItem::SolidColor {
            rect: Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT,
                width: 320.0,
                height: 80.0,
            },
            color: silksurf_css::Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        }];

        assert_eq!(
            browser_frame_height(&items, BROWSER_CHROME_HEIGHT as u32),
            124
        );
    }

    #[test]
    fn initial_browser_window_height_is_bounded() {
        assert_eq!(initial_browser_window_height(44), MIN_INITIAL_WINDOW_HEIGHT);
        assert_eq!(initial_browser_window_height(320), 320);
        assert_eq!(initial_browser_window_height(640), 640);
        assert_eq!(initial_browser_window_height(1200), FRAME_HEIGHT);
    }

    #[test]
    fn window_size_repaint_policy_skips_clean_shrinks() {
        assert!(window_size_exposes_unpainted_area(0, 0, 1280, 320));
        assert!(window_size_exposes_unpainted_area(1280, 320, 1281, 320));
        assert!(window_size_exposes_unpainted_area(1280, 320, 1280, 321));
        assert!(!window_size_exposes_unpainted_area(1280, 320, 1280, 319));
        assert!(!window_size_exposes_unpainted_area(1280, 320, 1279, 319));
    }

    #[test]
    fn address_chrome_present_damage_tracks_address_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X,
                y: ADDRESS_BAR_Y,
                width: ADDRESS_BAR_WIDTH,
                height: ADDRESS_BAR_HEIGHT,
            })
        );
    }

    #[test]
    fn address_focus_present_damage_tracks_address_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressFocusChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X,
                y: ADDRESS_BAR_Y,
                width: ADDRESS_BAR_WIDTH,
                height: ADDRESS_BAR_HEIGHT,
            })
        );
    }

    #[test]
    fn address_text_present_damage_tracks_text_strip() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressTextChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X + 10,
                y: ADDRESS_BAR_Y + 7,
                width: ADDRESS_BAR_WIDTH - 22,
                height: ADDRESS_BAR_HEIGHT - 14,
            })
        );
    }

    #[test]
    fn address_full_text_present_damage_tracks_text_strip() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressFullTextChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X + 10,
                y: ADDRESS_BAR_Y + 7,
                width: ADDRESS_BAR_WIDTH - 22,
                height: ADDRESS_BAR_HEIGHT - 14,
            })
        );
    }

    #[test]
    fn status_present_damage_tracks_status_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::StatusChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 1010,
                y: 14,
                width: 160,
                height: 7,
            })
        );
    }

    #[test]
    fn navigation_start_present_damage_tracks_active_chrome_parts() {
        let damage = browser_present_damage(
            BrowserRedrawMode::NavigationStartChrome,
            320,
            BROWSER_CHROME_HEIGHT as u32,
            0,
            1280,
            320,
        );
        let silksurf_gui::WinitPresentDamage::Rects(rects) = damage else {
            panic!("navigation start should present disjoint damage rects");
        };

        assert_eq!(
            rects.as_slice(),
            &[
                silksurf_gui::WinitDamageRect {
                    x: RELOAD_BUTTON_X,
                    y: NAV_BUTTON_Y,
                    width: NAV_BUTTON_WIDTH,
                    height: NAV_BUTTON_HEIGHT,
                },
                silksurf_gui::WinitDamageRect {
                    x: STOP_BUTTON_X,
                    y: NAV_BUTTON_Y,
                    width: NAV_BUTTON_WIDTH,
                    height: NAV_BUTTON_HEIGHT,
                },
                silksurf_gui::WinitDamageRect {
                    x: 1010,
                    y: 14,
                    width: 160,
                    height: 7,
                },
            ]
        );
    }

    #[test]
    fn content_present_damage_maps_scrolled_frame_rect_to_window_rect() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::Damage(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 12,
                y: BROWSER_CHROME_HEIGHT as u32 + 20,
                width: 30,
                height: 12,
            })
        );
    }

    #[test]
    fn page_input_focus_present_damage_maps_scrolled_rect() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::PageInputFocus(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 12,
                y: BROWSER_CHROME_HEIGHT as u32 + 20,
                width: 30,
                height: 12,
            })
        );
    }

    #[test]
    fn content_damage_with_chrome_unions_present_rects() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::DamageWithChrome(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 0,
                y: 0,
                width: 1280,
                height: BROWSER_CHROME_HEIGHT as u32 + 32,
            })
        );
    }

    #[test]
    fn age_zero_partial_redraw_seeds_full_buffer() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT,
            width: 30.0,
            height: 12.0,
        };

        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressFocusChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressTextChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::Chrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::Damage(damage),
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::PageInputFocus(damage),
            0
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressChrome,
            1
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressFocusChrome,
            1
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::Full,
            0
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::Clean,
            0
        ));
    }

    #[test]
    fn scroll_offset_clamps_to_content_range() {
        assert_eq!(clamp_scroll_offset(-12.0, 100.0), 0.0);
        assert_eq!(clamp_scroll_offset(125.0, 100.0), 100.0);
        assert_eq!(clamp_scroll_offset(f32::NAN, 100.0), 0.0);
        assert_eq!(max_browser_scroll_offset(1200, 800, 44), 400.0);
    }

    #[test]
    fn scroll_exposed_document_rect_tracks_direction() {
        let down = scroll_exposed_document_rect(48, 100, 4);
        assert_eq!(down.y, 144.0);
        assert_eq!(down.height, 4.0);

        let up = scroll_exposed_document_rect(40, 100, -4);
        assert_eq!(up.y, 84.0);
        assert_eq!(up.height, 4.0);
    }

    #[test]
    fn scroll_reuse_only_handles_small_deltas() {
        assert!(!scroll_reuse_is_profitable(756, 0));
        assert!(scroll_reuse_is_profitable(756, 96));
        assert!(!scroll_reuse_is_profitable(756, 682));
        assert!(!scroll_reuse_is_profitable(756, 756));
    }

    #[test]
    fn background_modulepreload_caps_large_rounds() {
        assert!(background_modulepreload_round_fits(
            MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
        ));
        assert!(!background_modulepreload_round_fits(
            MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS + 1
        ));
    }

    #[test]
    fn rgba_bytes_to_argb_words_into_packs_and_reuses_capacity() {
        let rgba = [
            0x11, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x10, 0x20, 0x30, 0x40, 0xab, 0xbc,
            0xcd, 0xde, 0x01, 0x02, 0x03, 0x04,
        ];
        let mut argb = Vec::with_capacity(8);
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let capacity = argb.capacity();

        assert_eq!(
            argb,
            vec![0x44112233, 0xddaabbcc, 0x40102030, 0xdeabbccd, 0x04010203]
        );
        rgba_bytes_to_argb_words_into(&rgba[..4], &mut argb);
        assert_eq!(argb, vec![0x44112233]);
        assert_eq!(argb.capacity(), capacity);
    }

    #[test]
    fn rgba_bytes_to_argb_words_into_packs_simd_lanes_and_tail() {
        let mut rgba = Vec::new();
        let mut expected = Vec::new();
        for index in 0..17u8 {
            let r = index.wrapping_mul(3).wrapping_add(1);
            let g = index.wrapping_mul(5).wrapping_add(2);
            let b = index.wrapping_mul(7).wrapping_add(3);
            let a = index.wrapping_mul(11).wrapping_add(4);
            rgba.extend_from_slice(&[r, g, b, a]);
            expected.push(
                (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            );
        }

        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);

        assert_eq!(argb, expected);
    }

    #[test]
    fn scratch_damage_pack_matches_retained_rgba_pack() {
        let display_list = silksurf_render::DisplayList {
            items: vec![silksurf_render::DisplayItem::SolidColor {
                rect: Rect {
                    x: 1.0,
                    y: 1.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: silksurf_css::Color {
                    r: 220,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            }],
            tiles: None,
        };
        let damage = Rect {
            x: 1.0,
            y: 1.0,
            width: 2.0,
            height: 2.0,
        };
        let mut rgba = vec![255; 4 * 4 * 4];
        let mut scratch = silksurf_render::DamageScratch::default();
        silksurf_render::rasterize_skia_damage_into(
            &display_list,
            4,
            4,
            damage,
            &mut rgba,
            &mut scratch,
        );
        let mut from_rgba = vec![0xffff_ffff; 16];
        let mut from_scratch = from_rgba.clone();

        sync_argb_damage_from_rgba(&rgba, &mut from_rgba, 4, 4, damage);
        assert!(sync_argb_damage_from_scratch(
            &scratch,
            &mut from_scratch,
            4
        ));

        assert_eq!(from_scratch, from_rgba);
    }

    #[test]
    fn shift_browser_argb_content_rows_reuses_rows_when_scrolling_down() {
        let width = 2;
        let chrome_rows = 1;
        let content_rows = 4;
        let mut argb = (0..10).collect::<Vec<u32>>();

        assert!(shift_browser_argb_content_rows(
            &mut argb,
            width,
            chrome_rows,
            content_rows,
            1,
        ));

        assert_eq!(argb, vec![0, 1, 4, 5, 6, 7, 8, 9, 8, 9]);
    }

    #[test]
    fn shift_browser_argb_content_rows_reuses_rows_when_scrolling_up() {
        let width = 2;
        let chrome_rows = 1;
        let content_rows = 4;
        let mut argb = (0..10).collect::<Vec<u32>>();

        assert!(shift_browser_argb_content_rows(
            &mut argb,
            width,
            chrome_rows,
            content_rows,
            -1,
        ));

        assert_eq!(argb, vec![0, 1, 2, 3, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn link_targets_resolve_anchor_text_rects() {
        let document = parse_html(
            "<!doctype html><html><body><a href=\"/docs/start\">Example</a></body></html>",
        )
        .expect("html parses");
        let text = find_text_node(&document.dom, document.document, "Example").expect("text node");
        let rect = Rect {
            x: 12.0,
            y: 64.0,
            width: 90.0,
            height: 18.0,
        };
        let items = vec![DisplayItem::Text {
            rect,
            node: text,
            text_len: 7,
            text: "Example".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        }];

        let targets = collect_link_targets(&document.dom, &items, "https://example.com/root/");

        assert_eq!(
            targets,
            vec![LinkTarget {
                rect,
                href: "https://example.com/docs/start".to_string(),
            }]
        );
    }

    #[test]
    fn image_nodes_emit_image_display_items() {
        let document = parse_html(
            "<!doctype html><html><body><img src=\"/asset.webp\" width=\"40\" height=\"20\" alt=\"demo\"></body></html>",
        )
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let images = vec![DecodedPageImage {
            url: "https://example.com/asset.webp".to_string(),
            surface: silksurf_render::ImageSurface {
                width: 1,
                height: 1,
                rgba: Arc::<[u8]>::from(vec![255, 0, 0, 255]),
            },
        }];
        let replaced_sizes = collect_image_replaced_sizes(
            &document.dom,
            document.document,
            "https://example.com/page",
            &images,
        );
        let mut fused = fused_style_layout_paint_with_replaced_sizes(
            &document.dom,
            &stylesheet,
            document.document,
            viewport,
            &replaced_sizes,
        );
        let mut items = std::mem::take(&mut fused.display_items);

        append_image_display_items(
            &document.dom,
            &fused,
            "https://example.com/page",
            &images,
            &mut items,
        );

        assert!(items.iter().any(|item| matches!(
            item,
            silksurf_render::DisplayItem::Image { rect, .. }
                if rect.width == 40.0 && rect.height == 20.0
        )));
    }

    #[test]
    fn image_resource_cache_returns_decoded_surface() {
        let mut cache = ImageResourceCache::with_capacity(4, 1024);
        let image = DecodedPageImage {
            url: "https://example.com/asset.webp".to_string(),
            surface: silksurf_render::ImageSurface {
                width: 2,
                height: 1,
                rgba: Arc::<[u8]>::from(vec![255, 0, 0, 255, 0, 255, 0, 255]),
            },
        };

        cache.insert(image.clone());
        let cached = cache
            .get("https://example.com/asset.webp")
            .expect("image cache returns inserted image");

        assert_eq!(cached.url, image.url);
        assert_eq!(cached.surface.width, 2);
        assert_eq!(cached.surface.rgba.as_ref(), image.surface.rgba.as_ref());
        assert_eq!(cache.len(), 1);
        assert!(cache.bytes() >= image.surface.rgba.len() as u64);
    }

    #[test]
    fn dedupe_resource_urls_preserves_first_seen_order() {
        let urls = vec![
            "https://example.com/a.png".to_string(),
            "https://example.com/b.png".to_string(),
            "https://example.com/a.png".to_string(),
        ];

        assert_eq!(
            dedupe_resource_urls(&urls),
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_modulepreload_links_as_rel_tokens() {
        let document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"modulepreload\" href=\"/app.mjs\">\
             <link rel=\"stylesheet modulepreload\" href=\"/shared.js\">\
             <link rel=\"stylesheet\" href=\"/style.css\">\
             </head></html>",
        )
        .expect("fixture parses");

        assert_eq!(
            extract_modulepreload_urls(&document.dom, document.document, "https://example.com/"),
            vec![
                "https://example.com/app.mjs".to_string(),
                "https://example.com/shared.js".to_string(),
            ]
        );
        assert_eq!(
            extract_stylesheet_urls(&document.dom, document.document, "https://example.com/"),
            vec![
                "https://example.com/shared.js".to_string(),
                "https://example.com/style.css".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_module_warm_urls_from_module_scripts_and_inline_imports() {
        let document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"modulepreload\" href=\"/preload.mjs\">\
             <script type=\"module\" src=\"/entry.mjs\"></script>\
             <script type=\"module\">import './inline-dep.mjs';</script>\
             <script src=\"/classic.js\"></script>\
             </head></html>",
        )
        .expect("fixture parses");

        assert_eq!(
            extract_module_warm_urls(&document.dom, document.document, "https://example.com/app/"),
            vec![
                "https://example.com/preload.mjs".to_string(),
                "https://example.com/entry.mjs".to_string(),
                "https://example.com/app/inline-dep.mjs".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_static_module_import_specifiers() {
        let source = r#"
            import "./side-effect.mjs";
            import value from './value.mjs';
            import { x as y } from "./named.mjs";
            import * as ns from "./namespace.mjs";
            export { y } from "./reexport.mjs";
            export * from './all.mjs';
            import("./dynamic.mjs");
        "#;

        assert_eq!(
            module_static_import_specifiers(source),
            vec![
                "./side-effect.mjs".to_string(),
                "./value.mjs".to_string(),
                "./named.mjs".to_string(),
                "./namespace.mjs".to_string(),
                "./reexport.mjs".to_string(),
                "./all.mjs".to_string(),
            ]
        );
    }

    #[test]
    fn address_focus_overlay_preserves_existing_text_pixels() {
        let mut pixels = vec![0; 1100 * 44];
        draw_browser_address_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
            false,
        );
        let text_color = argb(31, 41, 55, 255);
        let text_pixels_before = pixels.iter().filter(|pixel| **pixel == text_color).count();

        draw_browser_address_focus_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
        );

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            argb(37, 99, 235, 255)
        );
        assert!(pixels.iter().filter(|pixel| **pixel == text_color).count() >= text_pixels_before);
    }

    #[test]
    fn hit_test_link_accounts_for_scroll_and_chrome() {
        let targets = vec![LinkTarget {
            rect: Rect {
                x: 20.0,
                y: 244.0,
                width: 80.0,
                height: 20.0,
            },
            href: "https://example.com/next".to_string(),
        }];

        assert_eq!(
            hit_test_link(&targets, 30.0, 144.0, 100.0, 44),
            Some("https://example.com/next")
        );
        assert_eq!(hit_test_link(&targets, 30.0, 24.0, 100.0, 44), None);
        assert_eq!(hit_test_link(&targets, 8.0, 144.0, 100.0, 44), None);
    }

    #[test]
    fn input_targets_resolve_empty_controls_from_layout_rects() {
        let document = parse_html("<!doctype html><html><body><input id=\"q\"></body></html>")
            .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);

        let targets = collect_input_targets(&document.dom, &fused);

        assert_eq!(targets.len(), 1);
        assert!(targets[0].rect.width > 0.0);
        assert!(targets[0].rect.height > 0.0);
    }

    #[test]
    fn input_targets_include_contenteditable_controls() {
        let document = parse_html(
            "<!doctype html><html><body><div id=\"composer\" contenteditable=\"true\">Hi</div></body></html>",
        )
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);

        let targets = collect_input_targets(&document.dom, &fused);

        assert_eq!(targets.len(), 1);
        assert!(is_text_content_editable_node(
            &document.dom,
            targets[0].node
        ));
    }

    #[test]
    fn checkbox_and_radio_submission_uses_checked_controls() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<input name=\"q\" value=\"silk\">",
            "<input type=\"checkbox\" name=\"opt\" checked>",
            "<input type=\"checkbox\" name=\"skip\" value=\"no\">",
            "<input type=\"radio\" name=\"tier\" value=\"basic\">",
            "<input type=\"radio\" name=\"tier\" value=\"pro\" checked>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![
                ("q".to_string(), "silk".to_string()),
                ("opt".to_string(), "on".to_string()),
                ("tier".to_string(), "pro".to_string()),
            ]
        );
    }

    #[test]
    fn checkbox_toggle_marks_control_dirty() {
        let mut document = parse_html(
            "<!doctype html><html><body><form><input type=\"checkbox\" name=\"opt\"></form></body></html>",
        )
        .expect("html parses");
        let checkbox =
            first_element_by_name(&document.dom, document.document, "input").expect("input exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(toggle_checkbox_control(&mut document.dom, checkbox).expect("checkbox toggles"));
        assert!(input_checked(&document.dom, checkbox));
        assert_eq!(document.dom.take_dirty_nodes(), vec![checkbox]);

        assert!(toggle_checkbox_control(&mut document.dom, checkbox).expect("checkbox untoggles"));
        assert!(!input_checked(&document.dom, checkbox));
        assert_eq!(document.dom.take_dirty_nodes(), vec![checkbox]);
    }

    #[test]
    fn radio_check_unchecks_same_named_group() {
        let mut document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<input type=\"radio\" name=\"tier\" value=\"basic\" checked>",
            "<input type=\"radio\" name=\"tier\" value=\"pro\">",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let basic = element_by_attr(&document.dom, document.document, "input", "value", "basic")
            .expect("basic radio exists");
        let pro = element_by_attr(&document.dom, document.document, "input", "value", "pro")
            .expect("pro radio exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(
            document
                .dom
                .with_mutation_batch(|dom| check_radio_control(dom, document.document, pro))
                .expect("radio group updates")
        );

        assert!(!input_checked(&document.dom, basic));
        assert!(input_checked(&document.dom, pro));
        assert_eq!(document.dom.take_dirty_nodes(), vec![basic, pro]);
    }

    #[test]
    fn checkbox_is_interactive_but_not_text_editable() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<input type=\"text\" name=\"q\">",
            "<input type=\"checkbox\" name=\"opt\">",
            "<textarea name=\"note\">Hi</textarea>",
            "</body></html>"
        ))
        .expect("html parses");
        let text = element_by_attr(&document.dom, document.document, "input", "name", "q")
            .expect("text input exists");
        let checkbox = element_by_attr(&document.dom, document.document, "input", "name", "opt")
            .expect("checkbox exists");
        let textarea = first_element_by_name(&document.dom, document.document, "textarea")
            .expect("textarea exists");

        assert!(is_editable_input_node(&document.dom, checkbox));
        assert!(is_text_editable_input_node(&document.dom, text));
        assert!(is_text_editable_input_node(&document.dom, textarea));
        assert!(!is_text_editable_input_node(&document.dom, checkbox));
    }

    #[test]
    fn select_submission_uses_selected_option_value() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"recent\">Recent</option>",
            "<option value=\"popular\" selected>Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![("sort".to_string(), "popular".to_string())]
        );
    }

    #[test]
    fn select_submission_defaults_to_first_enabled_option_text() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"skip\" disabled>Skip</option>",
            "<option>Recent</option>",
            "<option value=\"popular\">Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![("sort".to_string(), "Recent".to_string())]
        );
    }

    #[test]
    fn select_cycle_marks_changed_options_dirty() {
        let mut document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"recent\" selected>Recent</option>",
            "<option value=\"popular\">Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let select = first_element_by_name(&document.dom, document.document, "select")
            .expect("select exists");
        let recent = element_by_attr(
            &document.dom,
            document.document,
            "option",
            "value",
            "recent",
        )
        .expect("recent option exists");
        let popular = element_by_attr(
            &document.dom,
            document.document,
            "option",
            "value",
            "popular",
        )
        .expect("popular option exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(cycle_select_control(&mut document.dom, select).expect("select cycles"));

        assert!(!option_selected(&document.dom, recent));
        assert!(option_selected(&document.dom, popular));
        assert_eq!(document.dom.take_dirty_nodes(), vec![recent, popular]);
    }

    #[test]
    fn select_is_interactive_but_not_text_editable() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<select name=\"sort\"><option>Recent</option></select>",
            "</body></html>"
        ))
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);
        let select = first_element_by_name(&document.dom, document.document, "select")
            .expect("select exists");

        assert!(is_editable_input_node(&document.dom, select));
        assert!(!is_text_editable_input_node(&document.dom, select));
        assert_eq!(collect_input_targets(&document.dom, &fused).len(), 1);
    }

    #[test]
    fn viewport_source_items_use_tile_index_without_duplicates() {
        let display_list = silksurf_render::DisplayList {
            items: vec![
                solid_item(0.0, 44.0, 1280.0, 200.0),
                solid_item(10.0, 260.0, 20.0, 20.0),
                solid_item(10.0, 1200.0, 20.0, 20.0),
            ],
            tiles: None,
        }
        .with_tiles(FRAME_WIDTH, 1400, DOCUMENT_TILE_SIZE);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: 756.0,
        };

        let items = browser_viewport_source_items(&display_list, viewport);

        assert_eq!(items.len(), 2);
        assert_eq!(display_item_rect(items[0]).y, 44.0);
        assert_eq!(display_item_rect(items[1]).y, 260.0);
    }

    #[test]
    fn build_browser_page_reuses_supplied_frame_buffer_capacity() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p>Hello</p></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let rgba_capacity = (FRAME_WIDTH * FRAME_HEIGHT * 4) as usize;
        let argb_capacity = (FRAME_WIDTH * FRAME_HEIGHT) as usize;
        let buffers = BrowserFrameBuffers {
            rgba: Vec::with_capacity(rgba_capacity),
            argb: Vec::with_capacity(argb_capacity),
        };

        let page = build_browser_page_with_buffers(payload, buffers).expect("payload builds page");

        assert!(page.runtime.rgba.capacity() >= rgba_capacity);
        assert!(page.frame.argb.capacity() >= argb_capacity);
    }

    #[test]
    fn browser_page_suppresses_style_and_script_metadata_text() {
        let inline_css = concat!(
            "body{background:#eee;width:60vw;margin:15vh auto;",
            "font-family:system-ui,sans-serif}",
            "h1{font-size:1.5em}",
            "div{opacity:0.8}",
            "a:link,a:visited{color:#348}"
        );
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: format!(
                concat!(
                    "<!doctype html><html><head><style>{}</style>",
                    "<script type=\"application/json\">hidden-script-text</script>",
                    "</head><body><div><h1>Example Domain</h1>",
                    "<p>This domain is for use in documentation examples ",
                    "without needing permission.</p>",
                    "<a href=\"https://www.iana.org/domains/example\">Learn more</a>",
                    "</div></body></html>"
                ),
                inline_css
            ),
            css_text: stylesheet_text_with_user_agent_defaults(inline_css),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let text_items: Vec<&str> = page
            .runtime
            .display_list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            text_items
                .iter()
                .any(|text| text.contains("Example Domain"))
        );
        assert!(
            text_items
                .iter()
                .any(|text| text.contains("documentation examples"))
        );
        assert!(!text_items.iter().any(|text| text.contains("body{")));
        assert!(
            !text_items
                .iter()
                .any(|text| text.contains("hidden-script-text"))
        );
    }

    #[test]
    fn navigation_page_build_uses_live_window_height() {
        let payload = BrowserPagePayload {
            url: "https://example.com/results/".to_string(),
            html: "<!doctype html><html><body><p>Result</p></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let page = build_browser_page_with_buffers_for_height(
            payload,
            BrowserFrameBuffers::default(),
            Some(FRAME_HEIGHT),
        )
        .expect("payload builds page");

        assert_eq!(page.frame.bitmap_height, FRAME_HEIGHT);
        assert_eq!(page.frame.argb.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
    }

    #[test]
    fn focused_input_typing_updates_value_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"q\" value=\"Hi\"></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "Hi!");
    }

    #[test]
    fn focused_textarea_typing_updates_text_content_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><textarea id=\"q\">Hi</textarea></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let textarea_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, textarea_node), "Hi!");
        assert!(find_text_node(&dom, textarea_node, "Hi!").is_some());
    }

    #[test]
    fn focused_contenteditable_typing_updates_text_content_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><div id=\"q\" contenteditable=\"plaintext-only\">Hi</div></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let editable_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(editable_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, editable_node), "Hi!");
        assert!(find_text_node(&dom, editable_node, "Hi!").is_some());
    }

    #[test]
    fn focused_textarea_enter_appends_newline_to_text_content() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><textarea id=\"q\">Hi</textarea></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let textarea_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_textarea_newline(&mut state));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, textarea_node), "Hi\n");
        assert!(find_text_node(&dom, textarea_node, "Hi\n").is_some());
    }

    #[test]
    fn focused_textarea_enter_ignores_plain_input_controls() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"q\" value=\"Hi\"></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(!push_focused_textarea_newline(&mut state));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "Hi");
    }

    #[test]
    fn focused_input_enter_builds_get_form_target() {
        let payload = BrowserPagePayload {
            url: "https://example.com/base/page.html".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form action=\"/search?source=fixture\">",
                "<input name=\"q\" value=\"rust gui\">",
                "<textarea name=\"note\">fast path</textarea>",
                "<input name=\"skip\" value=\"ignored\" disabled>",
                "<input value=\"unnamed\">",
                "</form>",
                "</body></html>"
            )
            .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/base/page.html".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/base/page.html".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::Get(
                "https://example.com/search?source=fixture&q=rust+gui&note=fast+path".to_string()
            ))
        );
    }

    #[test]
    fn focused_input_enter_builds_post_form_target() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form method=\"POST\" action=\"/submit\">",
                "<input name=\"q\" value=\"rust\">",
                "<textarea name=\"note\">fast path</textarea>",
                "</form>",
                "</body></html>"
            )
            .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::Post(
                BrowserNavigationRequest::post_form(
                    "https://example.com/submit".to_string(),
                    b"q=rust&note=fast+path".to_vec()
                )
            ))
        );
    }

    #[test]
    fn focused_input_enter_reports_unsupported_dialog_form() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form method=\"dialog\" action=\"/submit\">",
                "<input name=\"q\" value=\"rust\">",
                "</form>",
                "</body></html>"
            )
            .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::UnsupportedMethod(
                "dialog".to_string()
            ))
        );
    }

    #[test]
    fn focused_textarea_enter_does_not_submit_parent_form() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form action=\"/search\">",
                "<textarea name=\"q\">rust</textarea>",
                "</form>",
                "</body></html>"
            )
            .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let textarea_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(focused_form_submission_target(&state), None);
    }

    #[test]
    fn focused_input_text_damage_tracks_changed_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 5,
            text: "Hello".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            focused_input_text_damage_rect(&item, "Hello!").expect("text item gives damage");

        assert!(damage.x > 140.0);
        assert!(damage.width < 32.0);
        assert!(damage.height < 32.0);
    }

    #[test]
    fn focused_input_text_damage_tracks_next_line_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 6,
            text: "Hello\n".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            focused_input_text_damage_rect(&item, "Hello\n!").expect("text item gives damage");

        assert_eq!(damage.x, 100.0);
        assert!(damage.y > 220.0);
        assert!(damage.width < 32.0);
    }

    #[test]
    fn runtime_text_damage_tracks_changed_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 6,
            text: "stable".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            text_item_in_place_damage_rect(&item, "staple").expect("text item gives damage");

        assert!(damage.x > 125.0);
        assert!(damage.width < 48.0);
        assert!(damage.height < 32.0);
    }

    #[test]
    fn focused_empty_insert_damage_marks_first_text_cells() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let mut state = test_browser_state("https://example.com/");
        state.frame.input_targets = vec![InputTarget {
            rect: Rect {
                x: 8.0,
                y: 1436.0,
                width: 1264.0,
                height: 22.0,
            },
            node: input_node,
        }];

        let damage =
            focused_empty_insert_damage(&state.frame, input_node, "", "!").expect("damage exists");

        assert_eq!(damage.x, 8.0);
        assert_eq!(damage.y, 1436.0);
        assert!(damage.width < 32.0);
        assert_eq!(damage.height, 22.0);
    }

    #[test]
    fn focus_next_page_input_cycles_targets() {
        let first = silksurf_dom::NodeId::from_raw(10);
        let second = silksurf_dom::NodeId::from_raw(11);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.input_targets = vec![
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 20.0,
                    height: 20.0,
                },
                node: first,
            },
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT + 24.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: second,
            },
        ];

        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(first));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::PageInputFocus(_)
        ));
        state.redraw_mode = BrowserRedrawMode::Clean;
        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(second));
        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(first));
    }

    #[test]
    fn focus_next_page_input_prefers_text_editable_targets() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html:
                "<!doctype html><html><body><input type=\"checkbox\"><input id=\"q\"></body></html>"
                    .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let checkbox_node = page.frame.input_targets[0].node;
        let text_node = page.frame.input_targets[1].node;
        let mut state = test_browser_state_from_page(page);
        state.redraw_mode = BrowserRedrawMode::Clean;

        assert!(focus_next_page_input(&mut state));

        assert_ne!(state.focused_input, Some(checkbox_node));
        assert_eq!(state.focused_input, Some(text_node));
    }

    #[test]
    fn prepared_focus_target_matches_text_editable_focus_order() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html:
                "<!doctype html><html><body><input type=\"checkbox\"><input id=\"q\"></body></html>"
                    .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let checkbox_node = page.frame.input_targets[0].node;
        let text_node = page.frame.input_targets[1].node;
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let target =
            first_prepared_focus_target(&dom, &page.frame.input_targets).expect("target exists");

        assert_ne!(target.node, checkbox_node);
        assert_eq!(target.node, text_node);
    }

    #[test]
    fn focus_next_visible_page_input_prefers_viewport_targets() {
        let hidden = silksurf_dom::NodeId::from_raw(10);
        let visible = silksurf_dom::NodeId::from_raw(11);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.input_targets = vec![
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: 2_000.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: hidden,
            },
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT + 24.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: visible,
            },
        ];

        assert!(focus_next_visible_page_input(
            &mut state,
            0.0,
            BROWSER_CHROME_HEIGHT as u32,
            FRAME_HEIGHT
        ));
        assert_eq!(state.focused_input, Some(visible));
    }

    #[test]
    fn scroll_to_show_input_target_keeps_visible_target_stable() {
        let rect = Rect {
            x: 8.0,
            y: 120.0,
            width: 200.0,
            height: 24.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 2_000.0, BROWSER_CHROME_HEIGHT as u32, 800),
            0.0
        );
    }

    #[test]
    fn scroll_to_show_input_target_reveals_below_viewport_target() {
        let rect = Rect {
            x: 8.0,
            y: 1436.0,
            width: 1264.0,
            height: 22.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 2_000.0, BROWSER_CHROME_HEIGHT as u32, 800),
            682.0
        );
    }

    #[test]
    fn scroll_to_show_input_target_clamps_to_page_end() {
        let rect = Rect {
            x: 8.0,
            y: 3_000.0,
            width: 1264.0,
            height: 22.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 900.0, BROWSER_CHROME_HEIGHT as u32, 800),
            900.0
        );
    }

    #[test]
    fn first_focus_target_scroll_tracks_offscreen_input() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let targets = vec![InputTarget {
            rect: Rect {
                x: 8.0,
                y: 1436.0,
                width: 1264.0,
                height: 22.0,
            },
            node: input_node,
        }];

        assert_eq!(
            first_focus_target_scroll(&targets, 1500, 800, BROWSER_CHROME_HEIGHT as u32),
            Some(682)
        );
    }

    #[test]
    fn focus_viewport_cache_redraw_marks_visible_content_damage() {
        assert_eq!(
            focus_viewport_cache_redraw_mode(682, 800),
            BrowserRedrawMode::Damage(Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT + 682.0,
                width: FRAME_WIDTH as f32,
                height: 756.0,
            })
        );
    }

    #[test]
    fn apply_focus_viewport_cache_swaps_cached_pixels_once() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.focus_viewport_cache = Some(FocusViewportCache {
            scroll_y: 682,
            bitmap_height: 800,
            argb: vec![0x0102_0304, 0x0506_0708],
        });

        assert!(apply_focus_viewport_cache(&mut state, 682, 800));
        assert_eq!(state.frame.argb, vec![0x0102_0304, 0x0506_0708]);
        assert_eq!(state.frame.bitmap_scroll_y, 682);
        assert_eq!(state.frame.bitmap_height, 800);
        assert!(state.frame.focus_viewport_cache.is_none());
        assert!(!apply_focus_viewport_cache(&mut state, 682, 800));
    }

    #[test]
    fn focus_retained_buffer_update_sends_cache_once() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.focus_viewport_cache = Some(FocusViewportCache {
            scroll_y: 682,
            bitmap_height: FRAME_HEIGHT,
            argb: vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize],
        });
        let state = Rc::new(RefCell::new(state));

        let update =
            browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).expect("cache sends");

        assert_eq!(update.tag, FOCUS_VIEWPORT_RETAINED_TAG);
        assert_eq!(update.width, FRAME_WIDTH);
        assert_eq!(update.height, FRAME_HEIGHT);
        assert_eq!(update.pixels.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
        assert!(!state.borrow().frame.focus_viewport_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.focus_viewport_retained_sent);
        assert!(browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).is_none());
    }

    #[test]
    fn current_view_retained_buffer_update_feeds_page_focus() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let mut state = test_browser_state("https://example.com/");
        let rect = Rect {
            x: 32.0,
            y: 443.0,
            width: 320.0,
            height: 22.0,
        };
        state.frame.argb = vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize];
        state.frame.input_targets.push(InputTarget {
            rect,
            node: input_node,
        });
        let state = Rc::new(RefCell::new(state));

        let update = browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("current view sends");

        assert_eq!(update.tag, CURRENT_VIEW_RETAINED_TAG);
        assert!(!state.borrow().frame.current_view_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.current_view_retained_sent);

        {
            let mut state = state.borrow_mut();
            assert!(focus_page_input(&mut state, input_node));
            assert_eq!(
                state.retained_present,
                Some(BrowserRetainedPresent {
                    tag: CURRENT_VIEW_RETAINED_TAG,
                    damage: silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                        x: 32,
                        y: 443,
                        width: 320,
                        height: 22,
                    },),
                })
            );
        }
    }

    #[test]
    fn navigation_start_retained_buffer_update_prepaints_loading_chrome() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.argb = vec![0; (FRAME_WIDTH * FRAME_HEIGHT) as usize];
        state.frame.current_view_retained_sent = true;
        let state = Rc::new(RefCell::new(state));

        let update = browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("navigation start sends");

        assert_eq!(update.tag, NAVIGATION_START_RETAINED_TAG);
        assert!(!state.borrow().frame.navigation_start_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.navigation_start_retained_sent);
        assert_ne!(
            update.pixels[NAV_BUTTON_Y as usize * FRAME_WIDTH as usize + RELOAD_BUTTON_X as usize],
            0
        );
        assert_ne!(
            update.pixels[14 * FRAME_WIDTH as usize + 1010],
            0,
            "loading status band should be prepainted"
        );
    }

    #[test]
    fn scroll_retained_targets_cover_wheel_probe_deltas() {
        assert_eq!(scroll_retained_targets(200, 1_000.0), vec![296, 152]);
        assert_eq!(scroll_retained_targets(0, 1_000.0), vec![96]);
    }

    #[test]
    fn scroll_retained_buffer_update_sends_cache_once() {
        let mut state = test_browser_state("https://example.com/");
        let tag = scroll_retained_tag_for_scroll_y(96);
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag,
                argb: vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize],
                retained_sent: false,
            });

        let update = take_scroll_retained_buffer_update(&mut state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("cache sends");

        assert_eq!(update.tag, tag);
        assert_eq!(update.width, FRAME_WIDTH);
        assert_eq!(update.height, FRAME_HEIGHT);
        assert_eq!(update.pixels.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
        assert!(!state.frame.scroll_viewport_caches[0].retained_sent);
        let state = Rc::new(RefCell::new(state));
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.scroll_viewport_caches[0].retained_sent);
        assert!(browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).is_none());
    }

    #[test]
    fn scroll_viewport_cache_apply_sets_retained_present_state() {
        let mut state = test_browser_state("https://example.com/");
        let tag = scroll_retained_tag_for_scroll_y(96);
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag,
                argb: vec![0x0102_0304, 0x0506_0708],
                retained_sent: true,
            });

        let retained = apply_scroll_viewport_cache(
            &mut state,
            96,
            FRAME_HEIGHT,
            BROWSER_CHROME_HEIGHT as u32,
            FRAME_WIDTH,
            FRAME_HEIGHT,
        )
        .expect("retained present applies");

        assert_eq!(retained.tag, tag);
        assert_eq!(state.frame.argb, vec![0x0102_0304, 0x0506_0708]);
        assert_eq!(state.frame.bitmap_scroll_y, 96);
        assert_eq!(state.frame.bitmap_height, FRAME_HEIGHT);
        assert!(state.frame.scroll_viewport_caches.is_empty());
    }

    #[test]
    fn mark_redraw_clears_scroll_viewport_caches() {
        let mut state = test_browser_state("https://example.com/");
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag: scroll_retained_tag_for_scroll_y(96),
                argb: Vec::new(),
                retained_sent: true,
            });

        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert!(state.frame.scroll_viewport_caches.is_empty());
    }

    #[test]
    fn retained_present_action_clears_after_matching_present() {
        let state = Rc::new(RefCell::new(test_browser_state("https://example.com/")));
        let last_width = Cell::new(FRAME_WIDTH);
        let last_height = Cell::new(FRAME_HEIGHT);
        let damage = silksurf_gui::WinitPresentDamage::rect(0, 44, FRAME_WIDTH, 756);
        {
            let mut state = state.borrow_mut();
            state.redraw_mode = BrowserRedrawMode::Damage(Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT + 682.0,
                width: FRAME_WIDTH as f32,
                height: 756.0,
            });
            state.retained_present = Some(BrowserRetainedPresent {
                tag: FOCUS_VIEWPORT_RETAINED_TAG,
                damage,
            });
        }

        assert_eq!(
            browser_render_action(&state, &last_width, &last_height, FRAME_WIDTH, FRAME_HEIGHT),
            silksurf_gui::WinitRenderAction::Retained {
                tag: FOCUS_VIEWPORT_RETAINED_TAG,
                damage,
            }
        );

        handle_browser_presented_frame(
            &state,
            &last_width,
            &last_height,
            silksurf_gui::WinitPresentedFrame {
                width: FRAME_WIDTH,
                height: FRAME_HEIGHT,
                damage,
                retained_tag: Some(FOCUS_VIEWPORT_RETAINED_TAG),
            },
        );

        let state = state.borrow();
        assert_eq!(state.redraw_mode, BrowserRedrawMode::Clean);
        assert!(state.retained_present.is_none());
    }

    #[test]
    fn page_input_focus_only_repaints_address_when_address_was_editing() {
        let input_node = silksurf_dom::NodeId::from_raw(15);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.address_editing = true;

        assert!(focus_page_input(&mut state, input_node));
        assert_eq!(state.focused_input, Some(input_node));
        assert!(!state.address_editing);
        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);
    }

    #[test]
    fn page_input_focus_uses_copy_free_focus_damage() {
        let input_node = silksurf_dom::NodeId::from_raw(15);
        let mut state = test_browser_state("https://example.com/");
        let rect = Rect {
            x: 32.0,
            y: 443.0,
            width: 320.0,
            height: 22.0,
        };
        state.frame.input_targets.push(InputTarget {
            rect,
            node: input_node,
        });
        state.redraw_mode = BrowserRedrawMode::Clean;

        assert!(focus_page_input(&mut state, input_node));
        assert_eq!(state.focused_input, Some(input_node));
        assert_eq!(state.redraw_mode, BrowserRedrawMode::PageInputFocus(rect));
    }

    #[test]
    fn browser_cursor_shape_uses_chrome_and_page_targets() {
        let input_node = silksurf_dom::NodeId::from_raw(21);
        let mut state = test_browser_state("https://example.com/");
        state.history = vec![
            "https://example.com/start".to_string(),
            "https://example.com/".to_string(),
        ];
        state.history_index = 1;
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 16.0,
                y: 90.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });
        state.frame.input_targets.push(InputTarget {
            rect: Rect {
                x: 24.0,
                y: 130.0,
                width: 180.0,
                height: 24.0,
            },
            node: input_node,
        });

        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 15.0, 22.0, 0.0),
            silksurf_gui::WinitCursorShape::Pointer
        );
        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                ADDRESS_BAR_X as f32 + 8.0,
                ADDRESS_BAR_Y as f32 + 8.0,
                0.0
            ),
            silksurf_gui::WinitCursorShape::Text
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 32.0, 90.0, 0.0),
            silksurf_gui::WinitCursorShape::Pointer
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 32.0, 130.0, 0.0),
            silksurf_gui::WinitCursorShape::Text
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 400.0, 400.0, 0.0),
            silksurf_gui::WinitCursorShape::Default
        );
    }

    #[test]
    fn link_hover_status_updates_only_when_target_changes() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 16.0,
                y: 90.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });

        assert!(update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            32.0,
            90.0,
            0.0
        ));
        assert_eq!(browser_status_text(&state), "https://example.com/docs");
        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);

        state.redraw_mode = BrowserRedrawMode::Clean;
        assert!(!update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            32.0,
            90.0,
            0.0
        ));
        assert_eq!(state.redraw_mode, BrowserRedrawMode::Clean);

        assert!(update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            400.0,
            400.0,
            0.0
        ));
        assert_eq!(browser_status_text(&state), "ready");
        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);
    }

    #[cfg(feature = "accessibility")]
    #[test]
    fn accessibility_snapshot_exposes_chrome_links_and_inputs() {
        let input_node = silksurf_dom::NodeId::from_raw(21);
        let mut state = test_browser_state("https://example.com/");
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 10.0,
                y: 80.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });
        state.frame.input_targets.push(InputTarget {
            rect: Rect {
                x: 30.0,
                y: 120.0,
                width: 240.0,
                height: 24.0,
            },
            node: input_node,
        });
        state.focused_input = Some(input_node);

        let update = build_browser_accessibility_update(&state);
        let root = accessibility_node(&update, ACCESSIBILITY_ROOT_ID);
        let address = accessibility_node(&update, ACCESSIBILITY_ADDRESS_ID);
        let link = accessibility_node(&update, ACCESSIBILITY_LINK_BASE_ID);
        let input = accessibility_node(
            &update,
            ACCESSIBILITY_INPUT_BASE_ID + input_node.raw() as u64,
        );

        assert_eq!(
            update.tree.as_ref().expect("tree exists").root,
            accesskit::NodeId(ACCESSIBILITY_ROOT_ID)
        );
        assert_eq!(
            update.focus,
            accesskit::NodeId(ACCESSIBILITY_INPUT_BASE_ID + input_node.raw() as u64)
        );
        assert!(
            root.children()
                .contains(&accesskit::NodeId(ACCESSIBILITY_BACK_ID))
        );
        assert!(
            root.children()
                .contains(&accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID))
        );
        assert_eq!(address.role(), accesskit::Role::UrlInput);
        assert_eq!(address.value(), Some("https://example.com/"));
        assert_eq!(link.role(), accesskit::Role::Link);
        assert_eq!(link.url(), Some("https://example.com/docs"));
        assert_eq!(input.role(), accesskit::Role::TextInput);
    }

    #[cfg(feature = "accessibility")]
    #[test]
    fn accessibility_snapshot_focuses_address_while_editing() {
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/search".to_string();

        let update = build_browser_accessibility_update(&state);
        let address = accessibility_node(&update, ACCESSIBILITY_ADDRESS_ID);

        assert_eq!(update.focus, accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID));
        assert_eq!(address.value(), Some("https://example.com/search"));
    }

    #[test]
    fn link_targets_ignore_unsupported_schemes() {
        assert_eq!(
            resolve_page_url("mailto:ops@example.com", "https://example.com/"),
            None
        );
        assert_eq!(
            resolve_page_url("#top", "https://example.com/docs/page"),
            Some("https://example.com/docs/page#top".to_string())
        );
    }

    #[test]
    fn browser_status_overlay_updates_chrome_pixels() {
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_status(&mut pixels, 1100, 44, "loading");

        let rect = browser_status_text_band_rect(1100, 44).expect("status rect exists");
        assert_eq!(
            pixels[rect.y as usize * 1100 + (rect.x + rect.width - 1) as usize],
            argb(243, 244, 246, 255)
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(75, 85, 99, 255)),
            "status glyph should write foreground pixels"
        );
    }

    #[test]
    fn chrome_action_enabled_tracks_history_and_pending_navigation() {
        let mut state = test_browser_state("https://example.com/b");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;

        assert!(chrome_action_enabled(&state, BrowserChromeAction::Back));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Forward));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Home));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Reload));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Stop));

        state.navigation_pending = true;
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Back));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Forward));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Home));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Reload));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Stop));
    }

    #[test]
    fn chrome_action_hit_test_tracks_button_bounds() {
        assert_eq!(
            hit_test_chrome_action(BACK_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Back)
        );
        assert_eq!(
            hit_test_chrome_action(FORWARD_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Forward)
        );
        assert_eq!(
            hit_test_chrome_action(HOME_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Home)
        );
        assert_eq!(
            hit_test_chrome_action(RELOAD_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Reload)
        );
        assert_eq!(
            hit_test_chrome_action(STOP_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Stop)
        );
        assert_eq!(
            hit_test_chrome_action(ADDRESS_BAR_X as f32 + 2.0, ADDRESS_BAR_Y as f32 + 2.0),
            None
        );
    }

    #[test]
    fn disabled_chrome_buttons_do_not_request_pointer_cursor() {
        let state = test_browser_state("https://example.com/");

        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                BACK_BUTTON_X as f32 + 2.0,
                NAV_BUTTON_Y as f32 + 2.0,
                0.0,
            ),
            silksurf_gui::WinitCursorShape::Default
        );
    }

    #[test]
    fn enabled_chrome_buttons_request_pointer_cursor() {
        let mut state = test_browser_state("https://example.com/b");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;

        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                BACK_BUTTON_X as f32 + 2.0,
                NAV_BUTTON_Y as f32 + 2.0,
                0.0,
            ),
            silksurf_gui::WinitCursorShape::Pointer
        );
    }

    #[test]
    fn navigation_button_overlay_shows_disabled_state() {
        let state = test_browser_state("https://example.com/");
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_navigation_buttons(&state, &mut pixels, 1100, 44);

        assert_eq!(
            pixels[NAV_BUTTON_Y as usize * 1100 + BACK_BUTTON_X as usize],
            argb(209, 213, 219, 255)
        );
        assert!(
            pixels
                .iter()
                .any(|pixel| *pixel == argb(156, 163, 175, 255)),
            "disabled button glyph should use disabled foreground"
        );
    }

    #[test]
    fn browser_toolbar_background_fills_cached_rgba_rows() {
        let mut rgba = vec![0; 1100 * 44 * 4];

        fill_browser_toolbar_background_rgba(&mut rgba, 1100, 44);

        assert_eq!(&rgba[0..4], &[243, 244, 246, 255]);
        let separator_offset = 43 * 1100 * 4;
        assert_eq!(
            &rgba[separator_offset..separator_offset + 4],
            &[209, 213, 219, 255]
        );
    }

    #[test]
    fn address_input_normalizes_browser_urls() {
        assert_eq!(
            normalize_address_input("example.com/path"),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            normalize_address_input("http://example.com/"),
            Some("http://example.com/".to_string())
        );
        assert_eq!(normalize_address_input("mailto:ops@example.com"), None);
        assert_eq!(normalize_address_input("example .com"), None);
    }

    #[test]
    fn address_editing_updates_buffer_without_navigation() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        assert!(state.address_editing);
        assert!(state.address_select_all);
        assert_eq!(state.address_text, "https://example.com/");
        assert_eq!(state.address_cursor, "https://example.com/".len());
        assert!(push_address_char(&mut state, 'x'));
        assert!(!state.address_select_all);
        assert_eq!(state.address_text, "x");
        assert_eq!(state.address_cursor, 1);
        assert!(push_address_char(&mut state, 'y'));
        assert_eq!(state.address_text, "xy");
        assert_eq!(state.address_cursor, 2);
        assert!(move_address_caret(&mut state, AddressCaretMotion::Backward));
        assert_eq!(state.address_cursor, 1);
        assert!(push_address_char(&mut state, 'z'));
        assert_eq!(state.address_text, "xzy");
        assert_eq!(state.address_cursor, 2);
        assert!(edit_address_backspace(&mut state));
        assert_eq!(state.address_text, "xy");
        assert_eq!(state.address_cursor, 1);
        assert!(focus_address_bar(&mut state));
        assert!(edit_address_backspace(&mut state));
        assert_eq!(state.address_text, "");
        assert_eq!(state.address_cursor, 0);
    }

    #[test]
    fn address_caret_home_end_and_selection_collapse() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        assert!(move_address_caret(&mut state, AddressCaretMotion::Start));
        assert_eq!(state.address_cursor, 0);
        assert!(!state.address_select_all);
        assert!(push_address_char(&mut state, 'x'));
        assert_eq!(state.address_text, "xhttps://example.com/");
        assert!(move_address_caret(&mut state, AddressCaretMotion::End));
        assert_eq!(state.address_cursor, state.address_text.len());
    }

    #[test]
    fn address_clipboard_helpers_follow_selection_model() {
        let mut state = test_browser_state("https://example.com/");

        assert_eq!(address_clipboard_text(&state), None);
        assert!(focus_address_bar(&mut state));
        assert_eq!(address_clipboard_text(&state), Some("https://example.com/"));
        assert!(paste_address_text(
            &mut state,
            "https://chat.example/\nignored"
        ));
        assert_eq!(state.address_text, "https://chat.example/ignored");
        assert!(!state.address_select_all);
        assert!(!paste_address_text(&mut state, "\n\t"));
        assert!(!cut_address_text(&mut state));

        assert!(focus_address_bar(&mut state));
        assert!(cut_address_text(&mut state));
        assert_eq!(state.address_text, "");
        assert!(!state.address_select_all);
        assert_eq!(address_clipboard_text(&state), None);
    }

    #[test]
    fn address_paste_respects_text_limit() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        let pasted = "x".repeat(ADDRESS_TEXT_MAX_CHARS + 32);
        assert!(paste_address_text(&mut state, pasted.as_str()));
        assert_eq!(state.address_text.len(), ADDRESS_TEXT_MAX_CHARS);
    }

    #[test]
    fn address_overlay_draws_border_and_text_pixels() {
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_address_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
            true,
        );

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            argb(37, 99, 235, 255)
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)),
            "address overlay should write foreground pixels"
        );
    }

    #[test]
    fn chrome_overlay_leaves_content_pixels_untouched() {
        let mut pixels = vec![0x1234_5678; 1100 * 120];
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/edit".to_string();
        state.status_text = "loading".to_string();

        draw_browser_chrome_overlays(&state, &mut pixels, 1100, 120);

        assert_eq!(pixels[60 * 1100 + 20], 0x1234_5678);
        assert_ne!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            0x1234_5678
        );
    }

    #[test]
    fn full_redraw_request_survives_later_chrome_request() {
        let mut state = test_browser_state("https://example.com/");

        mark_redraw(&mut state, BrowserRedrawMode::Full);
        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Full);
    }

    #[test]
    fn damage_redraw_tracks_later_chrome_request() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Chrome;
        let damage = Rect {
            x: 8.0,
            y: BROWSER_CHROME_HEIGHT + 16.0,
            width: 32.0,
            height: 10.0,
        };

        mark_redraw(&mut state, BrowserRedrawMode::Damage(damage));
        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(
            state.redraw_mode,
            BrowserRedrawMode::DamageWithChrome(damage)
        );
    }

    #[test]
    fn clean_redraw_accepts_next_dirty_request() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::StatusChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);
    }

    #[test]
    fn navigation_start_redraw_stays_narrow_when_uncombined() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::NavigationStartChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::NavigationStartChrome);
    }

    #[test]
    fn status_redraw_promotes_when_address_also_changes() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::StatusChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Chrome);
    }

    #[test]
    fn address_chrome_merges_without_status_redraw() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);

        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Chrome);
    }

    #[test]
    fn address_text_chrome_merges_until_larger_chrome_damage() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressTextChrome);

        mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);
    }

    #[test]
    fn address_text_strip_leaves_border_pixels_untouched() {
        let mut pixels = vec![0x1234_5678; 1100 * 44];

        draw_browser_address_text_strip(&mut pixels, 1100, 44, "abc", 3);

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            0x1234_5678
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)),
            "address text strip should write foreground pixels"
        );
        assert_eq!(
            pixels[(ADDRESS_BAR_Y as usize + 10) * 1100 + (ADDRESS_BAR_X as usize + 240)],
            0x1234_5678
        );
    }

    #[test]
    fn address_cursor_x_uses_text_prefix() {
        let text_x = ADDRESS_BAR_X + 10;
        let max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;

        assert_eq!(bitmap_text_prefix_end_x(text_x, "abc", 0, max_x), text_x);
        assert_eq!(
            bitmap_text_prefix_end_x(text_x, "abc", 1, max_x),
            text_x + 6
        );
        assert_eq!(
            bitmap_text_prefix_end_x(text_x, "abc", 3, max_x),
            text_x + 18
        );
    }

    #[test]
    fn clean_redraw_does_not_downgrade_existing_damage() {
        let mut state = test_browser_state("https://example.com/");
        let damage = Rect {
            x: 8.0,
            y: BROWSER_CHROME_HEIGHT + 16.0,
            width: 32.0,
            height: 10.0,
        };
        state.redraw_mode = BrowserRedrawMode::Damage(damage);

        mark_redraw(&mut state, BrowserRedrawMode::Clean);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Damage(damage));
    }

    #[test]
    fn browser_frame_damage_blit_copies_visible_scrolled_rect() {
        let frame_width = 8;
        let frame_height = 12;
        let window_width = 8;
        let window_height = 8;
        let chrome_height = 2;
        let mut frame = vec![0_u32; (frame_width * frame_height) as usize];
        for y in 0..frame_height {
            for x in 0..frame_width {
                frame[(y * frame_width + x) as usize] = 0xAA00_0000 | (y << 8) | x;
            }
        }
        let mut pixels = vec![0xFFFF_FFFF; (window_width * window_height) as usize];

        blit_browser_frame_damage(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            3,
            window_width,
            window_height,
            Rect {
                x: 2.0,
                y: 6.0,
                width: 3.0,
                height: 2.0,
            },
            &mut pixels,
        );

        assert_eq!(pixels[(3 * window_width + 2) as usize], 0xAA00_0302);
        assert_eq!(pixels[(4 * window_width + 4) as usize], 0xAA00_0404);
        assert_eq!(pixels[(2 * window_width + 2) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(3 * window_width + 1) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(5 * window_width + 2) as usize], 0xFFFF_FFFF);
    }

    #[test]
    fn browser_frame_damage_blit_keeps_scrolled_damage_visible_below_viewport_height() {
        let frame_width = 8;
        let frame_height = 8;
        let window_width = 8;
        let window_height = 8;
        let chrome_height = 2;
        let scroll_y = 6;
        let mut frame = vec![0_u32; (frame_width * frame_height) as usize];
        for y in 0..frame_height {
            for x in 0..frame_width {
                frame[(y * frame_width + x) as usize] = 0xBB00_0000 | (y << 8) | x;
            }
        }
        let mut pixels = vec![0xFFFF_FFFF; (window_width * window_height) as usize];

        blit_browser_frame_damage(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
            Rect {
                x: 2.0,
                y: 10.0,
                width: 3.0,
                height: 2.0,
            },
            &mut pixels,
        );

        assert_eq!(pixels[(4 * window_width + 2) as usize], 0xBB00_0402);
        assert_eq!(pixels[(5 * window_width + 4) as usize], 0xBB00_0504);
        assert_eq!(pixels[(3 * window_width + 2) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(6 * window_width + 2) as usize], 0xFFFF_FFFF);
    }

    #[test]
    fn chrome_overlay_microbench_reports_cost() {
        let mut pixels = vec![0xFFFF_FFFF; 1280 * 800];
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/search?q=latency".to_string();
        state.status_text = "loading".to_string();
        let chrome_iters = 10_000_u32;
        let chrome_start = std::time::Instant::now();
        for _ in 0..chrome_iters {
            draw_browser_chrome_overlays(&state, &mut pixels, 1280, 800);
        }
        let chrome_avg = chrome_start.elapsed() / chrome_iters;
        assert!(pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)));

        let frame = vec![0xFFAA_AAAA; 1280 * 800];
        let full_iters = 200_u32;
        let full_start = std::time::Instant::now();
        for _ in 0..full_iters {
            blit_browser_frame(&frame, 1280, 800, 44, 0, 1280, 800, &mut pixels);
        }
        let full_avg = full_start.elapsed() / full_iters;

        eprintln!("[SilkSurf] chrome overlay avg: {chrome_avg:?}; full blit avg: {full_avg:?}");
    }

    #[test]
    fn address_typing_microbench_reports_cost() {
        let mut pixels = vec![0xFFFF_FFFF; 1280 * 320];
        let mut state = test_browser_state("https://example.com/");
        assert!(focus_address_bar(&mut state));
        let chars = b"chatgpt.com/?q=latency";
        let iterations = 10_000_u32;
        let start = std::time::Instant::now();
        for idx in 0..iterations {
            let ch = chars[(idx as usize) % chars.len()] as char;
            if push_address_char(&mut state, ch) {
                draw_browser_address_from_state(&state, &mut pixels, 1280, 320);
            }
            if state.address_text.len() > 128 {
                state.address_text.clear();
            }
        }
        let avg = start.elapsed() / iterations;

        assert!(pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)));
        let strip_start = std::time::Instant::now();
        for idx in 0..iterations {
            let ch = chars[(idx as usize) % chars.len()] as char;
            if push_address_char(&mut state, ch) {
                draw_browser_address_text_from_state(&state, &mut pixels, 1280, 320);
            }
            if state.address_text.len() > 128 {
                state.address_text.clear();
            }
        }
        let strip_avg = strip_start.elapsed() / iterations;

        eprintln!("[SilkSurf] address typing full avg: {avg:?}; text strip avg: {strip_avg:?}");
    }

    #[test]
    fn history_targets_track_back_and_forward_urls() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
            "https://example.com/c".to_string(),
        ];
        state.history_index = 1;

        assert_eq!(
            history_back_target(&state),
            Some((0, "https://example.com/a".to_string()))
        );
        assert_eq!(
            history_forward_target(&state),
            Some((2, "https://example.com/c".to_string()))
        );
    }

    #[test]
    fn push_history_truncates_forward_entries() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
            "https://example.com/c".to_string(),
        ];
        state.history_index = 1;
        state.pending_history = Some(PendingHistoryAction::Push);

        apply_history_success(&mut state, "https://example.com/d");

        assert_eq!(
            state.history,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/d".to_string(),
            ]
        );
        assert_eq!(state.history_index, 2);
    }

    #[test]
    fn move_history_changes_cursor_without_rewriting_entries() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;
        state.pending_history = Some(PendingHistoryAction::MoveTo(0));

        apply_history_success(&mut state, "https://example.com/a");

        assert_eq!(state.history_index, 0);
        assert_eq!(state.history.len(), 2);
    }

    #[test]
    fn navigation_generation_marks_stale_results() {
        let mut state = test_browser_state("https://example.com/a");
        state.navigation_generation = 7;
        state.navigation_pending = true;
        state.pending_history = Some(PendingHistoryAction::Push);
        state.status_text = "loading".to_string();

        state.navigation_generation = state.navigation_generation.saturating_add(1);
        state.navigation_pending = false;
        state.pending_history = None;
        state.status_text = "ready".to_string();

        assert_ne!(7, state.navigation_generation);
        assert!(!state.navigation_pending);
        assert_eq!(state.pending_history, None);
        assert_eq!(state.status_text, "ready");
    }

    #[test]
    fn text_only_dom_diff_produces_damage_rect() {
        let old_doc = parse_html("<!doctype html><html><body><p>Hello</p></body></html>")
            .expect("old html parses");
        let new_doc = parse_html("<!doctype html><html><body><p>World</p></body></html>")
            .expect("new html parses");
        let stylesheet = test_stylesheet(&old_doc.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&old_doc.dom, &stylesheet, old_doc.document, viewport);
        let new_fused =
            fused_style_layout_paint(&new_doc.dom, &stylesheet, new_doc.document, viewport);
        let diff = silksurf_dom::diff::diff_doms(
            &old_doc.dom,
            old_doc.document,
            &new_doc.dom,
            new_doc.document,
        );

        let damage = text_only_diff_damage_rect(&diff, &old_fused, &new_fused)
            .expect("text-only diff yields damage");

        assert!(damage.width > 0.0);
        assert!(damage.height > 0.0);
        assert!(damage.y >= BROWSER_CHROME_HEIGHT);
    }

    #[test]
    fn structural_dom_diff_requires_full_repaint() {
        let old_doc =
            parse_html("<!doctype html><html><body><p>Hello</p></body></html>").expect("old html");
        let new_doc =
            parse_html("<!doctype html><html><body><p>Hello</p><p>Second</p></body></html>")
                .expect("new html");
        let stylesheet = test_stylesheet(&old_doc.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&old_doc.dom, &stylesheet, old_doc.document, viewport);
        let new_fused =
            fused_style_layout_paint(&new_doc.dom, &stylesheet, new_doc.document, viewport);
        let diff = silksurf_dom::diff::diff_doms(
            &old_doc.dom,
            old_doc.document,
            &new_doc.dom,
            new_doc.document,
        );

        assert!(text_only_diff_damage_rect(&diff, &old_fused, &new_fused).is_none());
    }

    #[test]
    fn js_text_mutation_dirty_nodes_produce_damage_rect() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);
        let dom_arc = Arc::new(Mutex::new(document.dom));
        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }

        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 el.firstChild.textContent = 'Updated';",
            )
            .expect("script mutates text");

        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dirty_nodes = dom.take_dirty_nodes();
        let new_fused = fused_style_layout_paint(&dom, &stylesheet, document.document, viewport);
        let damage = dirty_nodes_damage_rect(&dom, &dirty_nodes, &old_fused, &new_fused)
            .expect("dirty text node yields damage");

        assert_eq!(dirty_nodes.len(), 1);
        assert!(damage.width > 0.0);
        assert!(damage.height > 0.0);
    }

    #[test]
    fn initial_host_tick_runs_deferred_dom_text_mutation() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let document_node = document.document;
        let dom_arc = Arc::new(Mutex::new(document.dom));
        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }

        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 setTimeout(function () { el.firstChild.textContent = 'Deferred'; }, 0);",
            )
            .expect("script schedules deferred mutation");
        {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(find_text_node(&dom, document_node, "Deferred").is_none());
        }

        drain_initial_host_callbacks(&mut js_ctx);

        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text_node =
            find_text_node(&dom, document_node, "Deferred").expect("host tick mutates text");
        assert_eq!(dom.take_dirty_nodes(), vec![text_node]);
    }

    #[test]
    fn retained_runtime_tick_repaints_dirty_text_damage() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let dom_arc = Arc::new(Mutex::new(document.dom));
        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
        let mut fused = {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fused_style_layout_paint(&dom, &stylesheet, document.document, viewport)
        };
        let display_list = silksurf_render::DisplayList {
            items: std::mem::take(&mut fused.display_items),
            tiles: None,
        };
        let raster_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
        let bitmap_height = initial_browser_window_height(raster_height);
        let mut rgba = Vec::new();
        rasterize_browser_viewport_into(&display_list, 0, bitmap_height, &mut rgba);
        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let old_argb = argb.clone();
        let runtime_display_list = display_list.clone();

        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 requestAnimationFrame(function () { el.firstChild.textContent = 'Runtime'; });",
            )
            .expect("script schedules frame mutation");

        let mut state = BrowserState {
            frame: BrowserFrame {
                url: "https://example.com/".to_string(),
                argb,
                raster_height,
                bitmap_height,
                bitmap_scroll_y: 0,
                focus_viewport_cache: None,
                focus_viewport_retained_sent: false,
                current_view_retained_sent: false,
                navigation_start_retained_sent: false,
                scroll_viewport_caches: Vec::new(),
                link_targets: Vec::new(),
                input_targets: Vec::new(),
            },
            runtime: Some(BrowserPageRuntime {
                dom: Arc::clone(&dom_arc),
                document: document.document,
                stylesheet,
                style_index,
                viewport,
                js_ctx,
                fused,
                fused_workspace: FusedWorkspace::new(),
                display_list: runtime_display_list,
                images: Vec::new(),
                rgba,
                damage_scratch: silksurf_render::DamageScratch::default(),
            }),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Chrome,
            retained_present: None,
        };

        assert!(tick_browser_runtime(&mut state));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::DamageWithChrome(_)
        ));
        assert_ne!(state.frame.argb, old_argb);

        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, document.document, "Runtime").is_some());
    }

    #[test]
    fn retained_runtime_text_mutation_skips_layout_when_text_fits() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let dom_arc = Arc::new(Mutex::new(document.dom));
        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
        let mut fused = {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fused_style_layout_paint(&dom, &stylesheet, document.document, viewport)
        };
        let display_list = silksurf_render::DisplayList {
            items: std::mem::take(&mut fused.display_items),
            tiles: None,
        };
        let raster_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
        let bitmap_height = initial_browser_window_height(raster_height);
        let mut rgba = Vec::new();
        rasterize_browser_viewport_into(&display_list, 0, bitmap_height, &mut rgba);
        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let old_argb = argb.clone();
        let runtime_display_list = display_list.clone();

        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 requestAnimationFrame(function () { el.firstChild.textContent = 'Jello'; });",
            )
            .expect("script schedules frame mutation");

        let mut state = BrowserState {
            frame: BrowserFrame {
                url: "https://example.com/".to_string(),
                argb,
                raster_height,
                bitmap_height,
                bitmap_scroll_y: 0,
                focus_viewport_cache: None,
                focus_viewport_retained_sent: false,
                current_view_retained_sent: false,
                navigation_start_retained_sent: false,
                scroll_viewport_caches: Vec::new(),
                link_targets: Vec::new(),
                input_targets: Vec::new(),
            },
            runtime: Some(BrowserPageRuntime {
                dom: Arc::clone(&dom_arc),
                document: document.document,
                stylesheet,
                style_index,
                viewport,
                js_ctx,
                fused,
                fused_workspace: FusedWorkspace::new(),
                display_list: runtime_display_list,
                images: Vec::new(),
                rgba,
                damage_scratch: silksurf_render::DamageScratch::default(),
            }),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Chrome,
            retained_present: None,
        };

        assert!(tick_browser_runtime(&mut state));
        let BrowserRedrawMode::DamageWithChrome(damage) = state.redraw_mode else {
            panic!("runtime mutation produces damage with chrome");
        };
        assert!(damage.width < 80.0);
        assert_ne!(state.frame.argb, old_argb);

        let runtime = state.runtime.as_ref().expect("runtime remains installed");
        assert_eq!(runtime.fused_workspace.node_count(), 0);
        assert!(runtime.damage_scratch.last_damage().is_none());
        assert!(
            runtime
                .display_list
                .items
                .iter()
                .any(|item| matches!(item, silksurf_render::DisplayItem::Text { text, .. } if text == "Jello"))
        );
        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, document.document, "Jello").is_some());
    }

    #[test]
    fn browser_page_payload_builds_retained_runtime() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script>requestAnimationFrame(function(){setTimeout(function(){document.getElementById('msg').firstChild.textContent='Runtime';},0);});</script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let mut page = build_browser_page(payload).expect("payload builds page");
        assert_eq!(page.frame.url, "https://example.com/");
        assert!(page.runtime.js_ctx.has_pending_host_callbacks());

        assert!(matches!(
            repaint_runtime_host_callbacks(&mut page.runtime, &mut page.frame)
                .expect("runtime callback repaints"),
            Some(BrowserRedrawMode::Damage(_))
        ));
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, page.runtime.document, "Runtime").is_some());
    }

    #[test]
    fn browser_page_payload_executes_external_script_text() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script src=\"/app.js\"></script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: vec![
                "document.getElementById('msg').firstChild.textContent='External';".to_string(),
            ],
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, page.runtime.document, "External").is_some());
    }

    #[test]
    fn browser_page_payload_executes_external_module_graph() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><script type=\"module\" src=\"/module.js\"></script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: vec![
                (
                    "/module.js".to_string(),
                    "import { fixtureGraph } from '/module-child.js'; document.body.setAttribute('data-module-graph', fixtureGraph);".to_string(),
                ),
                (
                    "/module-child.js".to_string(),
                    "export const fixtureGraph = 'module-child';".to_string(),
                ),
            ],
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let body = first_element_by_name(&dom, page.runtime.document, "body").expect("body exists");
        let attrs = dom.attributes(body).expect("body has attributes");
        assert!(attrs.iter().any(|attr| {
            attr.name.as_str() == "data-module-graph" && attr.value.as_str() == "module-child"
        }));
    }

    #[test]
    fn browser_page_payload_executes_dynamic_inline_script() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script>\
                   var script = document.createElement('script');\
                   script.innerHTML = \"document.getElementById('msg').firstChild.textContent='Dynamic';\";\
                   document.head.appendChild(script);\
                   </script></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, page.runtime.document, "Dynamic").is_some());
    }

    #[test]
    fn retained_runtime_repaints_js_input_value_viewport() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"prompt\" value=\"Hi\"><script>requestAnimationFrame(function(){setTimeout(function(){document.getElementById('prompt').value='AI';},0);});</script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
        };

        let mut page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;

        assert!(matches!(
            repaint_runtime_host_callbacks(&mut page.runtime, &mut page.frame)
                .expect("runtime callback repaints"),
            Some(BrowserRedrawMode::Damage(_))
        ));
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "AI");
    }

    fn find_text_node(dom: &silksurf_dom::Dom, root: NodeId, text: &str) -> Option<NodeId> {
        let node = dom.node(root).ok()?;
        if let NodeKind::Text { text: candidate } = node.kind()
            && candidate == text
        {
            return Some(root);
        }
        for &child in dom.children(root).ok()? {
            if let Some(found) = find_text_node(dom, child, text) {
                return Some(found);
            }
        }
        None
    }

    fn element_by_attr(
        dom: &silksurf_dom::Dom,
        root: NodeId,
        tag: &str,
        attr_name: &str,
        attr_value: &str,
    ) -> Option<NodeId> {
        if dom
            .element_name(root)
            .ok()
            .flatten()
            .is_some_and(|element| element.eq_ignore_ascii_case(tag))
            && element_attribute(dom, root, attr_name).is_some_and(|value| value == attr_value)
        {
            return Some(root);
        }
        for &child in dom.children(root).ok()? {
            if let Some(found) = element_by_attr(dom, child, tag, attr_name, attr_value) {
                return Some(found);
            }
        }
        None
    }

    fn test_stylesheet(dom: &silksurf_dom::Dom) -> silksurf_css::Stylesheet {
        let css = stylesheet_text_with_user_agent_defaults("");
        dom.with_interner_mut(|interner| {
            silksurf_css::parse_stylesheet_with_interner(&css, interner)
        })
        .expect("stylesheet parses")
    }

    fn test_browser_state(url: &str) -> BrowserState {
        BrowserState {
            frame: BrowserFrame {
                url: url.to_string(),
                argb: Vec::new(),
                raster_height: FRAME_HEIGHT,
                bitmap_height: FRAME_HEIGHT,
                bitmap_scroll_y: 0,
                focus_viewport_cache: None,
                focus_viewport_retained_sent: false,
                current_view_retained_sent: false,
                navigation_start_retained_sent: false,
                scroll_viewport_caches: Vec::new(),
                link_targets: Vec::new(),
                input_targets: Vec::new(),
            },
            runtime: None,
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec![url.to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: url.to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Full,
            retained_present: None,
        }
    }

    fn test_browser_state_from_page(page: BrowserPage) -> BrowserState {
        BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Full,
            retained_present: None,
        }
    }

    #[cfg(feature = "accessibility")]
    fn accessibility_node(update: &accesskit::TreeUpdate, id: u64) -> &accesskit::Node {
        update
            .nodes
            .iter()
            .find_map(|(node_id, node)| (*node_id == accesskit::NodeId(id)).then_some(node))
            .expect("accessibility node exists")
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn solid_item(x: f32, y: f32, width: f32, height: f32) -> DisplayItem {
        DisplayItem::SolidColor {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            color: silksurf_css::Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        }
    }
}

/// Extract href values from `<link rel="stylesheet">` tags.
fn extract_stylesheet_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_link_resource_urls(dom, root, base_url, "stylesheet", &mut urls);
    urls
}

fn extract_modulepreload_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_link_resource_urls(dom, root, base_url, "modulepreload", &mut urls);
    urls
}

fn extract_module_warm_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = extract_modulepreload_urls(dom, root, base_url);
    collect_module_script_warm_urls(dom, root, base_url, &mut urls);
    dedupe_resource_urls(&urls)
}

fn collect_module_script_warm_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = module_script_external_url(dom, node, base_url) {
        urls.push(url);
    }
    if let Some(text) = inline_module_script_text(dom, node) {
        urls.extend(module_static_import_urls(base_url, &text));
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_module_script_warm_urls(dom, child, base_url, urls);
        }
    }
}

fn external_module_script_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_external_module_script_urls(dom, root, base_url, &mut urls);
    dedupe_resource_urls(&urls)
}

fn collect_external_module_script_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = module_script_external_url(dom, node, base_url) {
        urls.push(url);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_external_module_script_urls(dom, child, base_url, urls);
        }
    }
}

fn module_script_external_url(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !script_type_is_module(Some(&attrs)) {
        return None;
    }
    let src = script_src(Some(&attrs))?;
    let resolved = resolve_resource_url(base_url, src);
    (!resolved.is_empty()).then_some(resolved)
}

fn module_path_for_url(module_url: &str) -> String {
    let Ok(parsed) = url::Url::parse(module_url) else {
        return module_url.to_string();
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

fn inline_module_script_text(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !script_type_is_module(Some(&attrs)) || script_src(Some(&attrs)).is_some() {
        return None;
    }
    let text = script_text_content(dom, node);
    (!text.trim().is_empty()).then_some(text)
}

fn collect_link_resource_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    rel_token: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = link_resource_url_for_node(dom, node, base_url, rel_token) {
        urls.push(url);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_link_resource_urls(dom, child, base_url, rel_token, urls);
        }
    }
}

fn link_resource_url_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    rel_token: &str,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "link" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !link_rel_contains(&attrs, rel_token) {
        return None;
    }
    let href = link_href(&attrs)?;
    let resolved = resolve_resource_url(base_url, href);
    (!resolved.is_empty()).then_some(resolved)
}

fn link_rel_contains(attrs: &[silksurf_dom::Attribute], token: &str) -> bool {
    let Some(rel) = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("rel"))
    else {
        return false;
    };
    rel.value
        .as_str()
        .split_ascii_whitespace()
        .any(|value| value.eq_ignore_ascii_case(token))
}

fn link_href(attrs: &[silksurf_dom::Attribute]) -> Option<&str> {
    attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("href"))
        .map(|attr| attr.value.as_str())
        .filter(|href| !href.trim().is_empty())
}

fn extract_image_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_img_tags(dom, root, base_url, &mut urls);
    urls
}

fn collect_img_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(src) = image_src_for_node(dom, node) {
        let resolved = resolve_resource_url(base_url, &src);
        if !resolved.is_empty() {
            urls.push(resolved);
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_img_tags(dom, child, base_url, urls);
        }
    }
}

fn fetch_decoded_images(
    renderer: &mut SpeculativeRenderer,
    image_cache: &mut ImageResourceCache,
    image_urls: &[String],
) -> Vec<DecodedPageImage> {
    let mut images = Vec::new();
    let mut missing_urls = Vec::new();
    for image_url in dedupe_resource_urls(image_urls) {
        if let Some(image) = image_cache.get(&image_url) {
            images.push(image);
        } else {
            missing_urls.push(image_url);
        }
    }
    if missing_urls.is_empty() {
        if !image_urls.is_empty() {
            eprintln!(
                "[SilkSurf] Image cache hit: {} entries, {} bytes",
                image_cache.len(),
                image_cache.bytes()
            );
        }
        return images;
    }

    let accept_header = [(
        "Accept".to_string(),
        "image/avif,image/webp,image/png,image/jpeg,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = missing_urls
        .iter()
        .map(|image_url| (image_url.as_str(), accept_header.as_slice()))
        .collect();
    for image in renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(missing_urls.iter())
        .filter_map(|(result, image_url)| decode_image_response(result, image_url))
    {
        image_cache.insert(image.clone());
        images.push(image);
    }
    eprintln!(
        "[SilkSurf] Image cache: {} entries, {} bytes",
        image_cache.len(),
        image_cache.bytes()
    );
    images
}

#[derive(Clone)]
enum DocumentScriptRef {
    Inline(String),
    External(String),
}

#[derive(Clone)]
struct DocumentScriptNode {
    node: silksurf_dom::NodeId,
    source: DocumentScriptRef,
}

fn load_document_script_texts(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let scripts = extract_document_scripts(dom, root, base_url);
    let external_urls: Vec<String> = scripts
        .iter()
        .filter_map(|script| match script {
            DocumentScriptRef::External(url) => Some(url.clone()),
            DocumentScriptRef::Inline(_) => None,
        })
        .collect();
    let fetched = fetch_external_script_texts(renderer, &external_urls);
    scripts
        .into_iter()
        .filter_map(|script| match script {
            DocumentScriptRef::Inline(text) => Some(text),
            DocumentScriptRef::External(url) => fetched
                .iter()
                .find_map(|(fetched_url, text)| (fetched_url == &url).then(|| text.clone())),
        })
        .collect()
}

fn load_document_module_texts(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<(String, String)> {
    let roots = external_module_script_urls(dom, root, base_url);
    let roots = dedupe_resource_urls(&roots);
    if roots.is_empty() {
        return Vec::new();
    }
    if roots.len() > MAX_NAVIGATION_MODULE_ROOTS {
        eprintln!(
            "[SilkSurf] Module execution skipped: {} roots exceed {} root cap",
            roots.len(),
            MAX_NAVIGATION_MODULE_ROOTS
        );
        return Vec::new();
    }
    let modules = fetch_module_graph_texts(renderer, &roots);
    let total_bytes: usize = modules.iter().map(|(_, text)| text.len()).sum();
    if total_bytes > MAX_NAVIGATION_MODULE_GRAPH_BYTES {
        eprintln!(
            "[SilkSurf] Module execution skipped: {total_bytes} bytes exceed {MAX_NAVIGATION_MODULE_GRAPH_BYTES} byte cap"
        );
        return Vec::new();
    }
    modules
        .into_iter()
        .map(|(module_url, text)| (module_path_for_url(&module_url), text))
        .collect()
}

fn fetch_module_graph_texts(
    renderer: &mut SpeculativeRenderer,
    roots: &[String],
) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut pending = dedupe_resource_urls(roots);
    let mut fetched = Vec::new();
    for _round in 0..MAX_MODULE_GRAPH_ROUNDS {
        if pending.is_empty() || seen.len() >= MAX_MODULE_GRAPH_URLS {
            break;
        }
        let round_urls = take_module_graph_round_urls(&mut pending, &mut seen);
        if round_urls.is_empty() {
            break;
        }
        let round_fetched = fetch_module_round_texts(renderer, &round_urls, "Module");
        pending.extend(module_graph_child_urls(&round_fetched));
        fetched.extend(round_fetched);
    }
    fetched
}

fn fetch_external_script_texts(
    renderer: &mut SpeculativeRenderer,
    script_urls: &[String],
) -> Vec<(String, String)> {
    let urls = dedupe_resource_urls(script_urls);
    if urls.is_empty() {
        return Vec::new();
    }
    let accept_header = [(
        "Accept".to_string(),
        "text/javascript,application/javascript,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = urls
        .iter()
        .map(|script_url| (script_url.as_str(), accept_header.as_slice()))
        .collect();
    renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(urls.iter())
        .filter_map(|(result, script_url)| script_response_text(result, script_url))
        .collect()
}

fn preload_module_scripts(module_urls: &[String], config: &BrowserRenderConfig) {
    let urls = dedupe_resource_urls(module_urls);
    if urls.is_empty() {
        return;
    }
    for module_url in &urls {
        eprintln!("[SilkSurf] Modulepreload {module_url}: scheduled");
    }
    let config = config.clone();
    if let Err(err) = thread::Builder::new()
        .name("silksurf-modulepreload".to_string())
        .spawn(move || match renderer_from_config(&config) {
            Ok(mut renderer) => preload_module_scripts_with_renderer(&mut renderer, &urls),
            Err(message) => eprintln!("[SilkSurf] Modulepreload renderer: {message}"),
        })
    {
        eprintln!("[SilkSurf] Modulepreload thread: {err}");
    }
}

fn preload_module_scripts_with_renderer(renderer: &mut SpeculativeRenderer, urls: &[String]) {
    let mut seen = HashSet::new();
    let mut pending = dedupe_resource_urls(urls);
    for round in 0..MAX_MODULE_GRAPH_ROUNDS {
        if pending.is_empty() || seen.len() >= MAX_MODULE_GRAPH_URLS {
            return;
        }
        let round_urls = take_module_graph_round_urls(&mut pending, &mut seen);
        if round_urls.is_empty() {
            return;
        }
        if !background_modulepreload_round_fits(round_urls.len()) {
            eprintln!(
                "[SilkSurf] Modulepreload graph stopped: {} round URLs exceed {} background cap",
                round_urls.len(),
                MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
            );
            return;
        }
        let fetched = preload_module_round(renderer, &round_urls);
        pending.extend(module_graph_child_urls(&fetched));
        eprintln!(
            "[SilkSurf] Modulepreload graph round {round}: {} fetched, {} pending",
            fetched.len(),
            pending.len()
        );
    }
}

fn background_modulepreload_round_fits(round_url_count: usize) -> bool {
    round_url_count <= MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
}

fn take_module_graph_round_urls(
    pending: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Vec<String> {
    let mut round_urls = Vec::new();
    for url in std::mem::take(pending) {
        if seen.len() >= MAX_MODULE_GRAPH_URLS {
            break;
        }
        if seen.insert(url.clone()) {
            round_urls.push(url);
        }
    }
    round_urls
}

fn preload_module_round(
    renderer: &mut SpeculativeRenderer,
    urls: &[String],
) -> Vec<(String, String)> {
    fetch_module_round_texts(renderer, urls, "Modulepreload")
}

fn fetch_module_round_texts(
    renderer: &mut SpeculativeRenderer,
    urls: &[String],
    label: &str,
) -> Vec<(String, String)> {
    let accept_header = [(
        "Accept".to_string(),
        "text/javascript,application/javascript,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = urls
        .iter()
        .map(|module_url| (module_url.as_str(), accept_header.as_slice()))
        .collect();
    renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(urls.iter())
        .filter_map(|(result, module_url)| module_response_text(result, module_url, label))
        .collect()
}

fn module_graph_child_urls(fetched: &[(String, String)]) -> Vec<String> {
    fetched
        .iter()
        .flat_map(|(module_url, text)| module_static_import_urls(module_url, text))
        .collect()
}

fn module_static_import_urls(base_url: &str, source: &str) -> Vec<String> {
    module_static_import_specifiers(source)
        .into_iter()
        .map(|specifier| resolve_resource_url(base_url, &specifier))
        .filter(|url| !url.is_empty())
        .collect()
}

fn module_static_import_specifiers(source: &str) -> Vec<String> {
    let mut specifiers = Vec::new();
    for statement in source.split(';') {
        let trimmed = statement.trim_start();
        if trimmed.starts_with("import") {
            collect_import_statement_specifier(trimmed, &mut specifiers);
        } else if trimmed.starts_with("export") {
            collect_export_statement_specifier(trimmed, &mut specifiers);
        }
    }
    specifiers
}

fn collect_import_statement_specifier(statement: &str, specifiers: &mut Vec<String>) {
    let rest = statement.trim_start_matches("import").trim_start();
    if rest.starts_with('(') {
        return;
    }
    if let Some(specifier) = quoted_prefix(rest) {
        specifiers.push(specifier);
        return;
    }
    if let Some(from_index) = rest.rfind(" from ") {
        let after_from = rest[from_index + " from ".len()..].trim_start();
        if let Some(specifier) = quoted_prefix(after_from) {
            specifiers.push(specifier);
        }
    }
}

fn collect_export_statement_specifier(statement: &str, specifiers: &mut Vec<String>) {
    let Some(from_index) = statement.rfind(" from ") else {
        return;
    };
    let after_from = statement[from_index + " from ".len()..].trim_start();
    if let Some(specifier) = quoted_prefix(after_from) {
        specifiers.push(specifier);
    }
}

fn quoted_prefix(text: &str) -> Option<String> {
    let quote = text.as_bytes().first().copied()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let rest = &text[1..];
    let end = rest.as_bytes().iter().position(|byte| *byte == quote)?;
    Some(rest[..end].to_string())
}

fn module_response_text(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    module_url: &str,
    label: &str,
) -> Option<(String, String)> {
    match result {
        Ok((response, origin, elapsed)) if response.status == 200 => {
            eprintln!(
                "[SilkSurf] {label} {module_url}: {} bytes ({origin:?} {elapsed:?})",
                response.body.len()
            );
            if response.body.len() > MAX_MODULE_GRAPH_SCAN_BYTES {
                eprintln!(
                    "[SilkSurf] {label} {module_url}: graph scan skipped at {} bytes",
                    response.body.len()
                );
                return None;
            }
            Some((
                module_url.to_string(),
                String::from_utf8_lossy(&response.body).to_string(),
            ))
        }
        Ok((response, _, _)) => {
            eprintln!("[SilkSurf] {label} {module_url}: HTTP {}", response.status);
            None
        }
        Err(err) => {
            eprintln!(
                "[SilkSurf] {label} {module_url}: fetch error: {}",
                err.message
            );
            None
        }
    }
}

fn script_response_text(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    script_url: &str,
) -> Option<(String, String)> {
    let (response, origin, elapsed) = match result {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "[SilkSurf] Script {script_url}: fetch error: {}",
                err.message
            );
            return None;
        }
    };
    if response.status != 200 {
        eprintln!("[SilkSurf] Script {script_url}: HTTP {}", response.status);
        return None;
    }
    let text = String::from_utf8_lossy(&response.body).to_string();
    eprintln!(
        "[SilkSurf] Script {script_url}: {} bytes ({origin:?} {elapsed:?})",
        response.body.len()
    );
    Some((script_url.to_string(), text))
}

fn dedupe_resource_urls(urls: &[String]) -> Vec<String> {
    let mut deduped = Vec::new();
    for url in urls {
        if !deduped.iter().any(|existing| existing == url) {
            deduped.push(url.clone());
        }
    }
    deduped
}

fn decode_image_response(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    image_url: &str,
) -> Option<DecodedPageImage> {
    let (response, origin, elapsed) = match result {
        Ok(value) => value,
        Err(err) => {
            eprintln!("[SilkSurf] Image {image_url}: fetch error: {}", err.message);
            return None;
        }
    };
    if response.status != 200 {
        eprintln!("[SilkSurf] Image {image_url}: HTTP {}", response.status);
        return None;
    }
    let content_type = response.header("content-type");
    let decoded = match silksurf_image::decode_image(&response.body, content_type) {
        Ok(decoded) => decoded,
        Err(err) => {
            eprintln!(
                "[SilkSurf] Image {image_url}: decode error: {}",
                err.message
            );
            return None;
        }
    };
    eprintln!(
        "[SilkSurf] Image {image_url}: {}x{} {} bytes ({origin:?} {elapsed:?})",
        decoded.width,
        decoded.height,
        response.body.len()
    );
    Some(DecodedPageImage {
        url: image_url.to_string(),
        surface: silksurf_render::ImageSurface {
            width: decoded.width,
            height: decoded.height,
            rgba: std::sync::Arc::from(decoded.rgba),
        },
    })
}

fn append_image_display_items(
    dom: &silksurf_dom::Dom,
    fused: &FusedResult,
    base_url: &str,
    images: &[DecodedPageImage],
    items: &mut Vec<silksurf_render::DisplayItem>,
) {
    for &node in &fused.table.bfs_order {
        let Some(src) = image_src_for_node(dom, node) else {
            continue;
        };
        let resolved = resolve_resource_url(base_url, &src);
        let Some(image) = images.iter().find(|image| image.url == resolved) else {
            continue;
        };
        let Some(rect) = fused_node_rect(fused, node) else {
            continue;
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            continue;
        }
        items.push(silksurf_render::DisplayItem::Image {
            rect,
            image: image.surface.clone(),
        });
    }
}

fn collect_image_replaced_sizes(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
) -> Vec<ReplacedSize> {
    let mut sizes = Vec::new();
    collect_image_replaced_sizes_for_node(dom, root, base_url, images, &mut sizes);
    sizes
}

fn collect_image_replaced_sizes_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
    sizes: &mut Vec<ReplacedSize>,
) {
    if let Some(size) = image_replaced_size_for_node(dom, node, base_url, images) {
        sizes.push(size);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_image_replaced_sizes_for_node(dom, child, base_url, images, sizes);
        }
    }
}

fn image_replaced_size_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
) -> Option<ReplacedSize> {
    let src = image_src_for_node(dom, node)?;
    let resolved = resolve_resource_url(base_url, &src);
    let image = images.iter().find(|image| image.url == resolved)?;
    let (attr_width, attr_height) = image_dimension_attrs(dom, node);
    let width = attr_width.unwrap_or(image.surface.width as f32);
    let height = attr_height.unwrap_or_else(|| inferred_image_height(attr_width, &image.surface));
    Some(ReplacedSize {
        node,
        width,
        height,
    })
}

fn image_dimension_attrs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> (Option<f32>, Option<f32>) {
    let Ok(attrs) = dom.attributes(node) else {
        return (None, None);
    };
    let width = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("width"))
        .and_then(|attr| parse_html_dimension(attr.value.as_str()));
    let height = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("height"))
        .and_then(|attr| parse_html_dimension(attr.value.as_str()));
    (width, height)
}

fn parse_html_dimension(value: &str) -> Option<f32> {
    let number = value.trim().trim_end_matches("px").parse::<f32>().ok()?;
    (number > 0.0).then_some(number)
}

fn inferred_image_height(attr_width: Option<f32>, surface: &silksurf_render::ImageSurface) -> f32 {
    let Some(width) = attr_width else {
        return surface.height as f32;
    };
    if surface.width == 0 {
        return surface.height as f32;
    }
    (width * surface.height as f32 / surface.width as f32).max(1.0)
}

fn image_src_for_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "img" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    let src = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("src"))?
        .value
        .as_str();
    if src.trim().is_empty() {
        return None;
    }
    Some(src.to_string())
}

fn resolve_resource_url(base_url: &str, resource_url: &str) -> String {
    if resource_url.starts_with("http://") || resource_url.starts_with("https://") {
        return resource_url.to_string();
    }
    url::Url::parse(base_url)
        .and_then(|base| base.join(resource_url))
        .map(|url| url.to_string())
        .unwrap_or_default()
}

fn extract_inline_css(dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) -> String {
    let mut css = String::new();
    collect_style_tags(dom, root, &mut css);
    css
}

fn collect_style_tags(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId, css: &mut String) {
    if let Ok(name) = dom.element_name(node)
        && name == Some("style")
        && let Ok(children) = dom.children(node)
    {
        for &child in children {
            if let Ok(n) = dom.node(child)
                && let silksurf_dom::NodeKind::Text { text } = n.kind()
            {
                css.push_str(text);
                css.push('\n');
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_style_tags(dom, child, css);
        }
    }
}

fn extract_document_scripts(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<DocumentScriptRef> {
    let mut scripts = Vec::new();
    collect_document_script_refs(dom, root, base_url, &mut scripts);
    scripts
}

fn collect_document_script_refs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    scripts: &mut Vec<DocumentScriptRef>,
) {
    if let Some(script) = script_ref_for_node(dom, node, base_url) {
        scripts.push(script);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_document_script_refs(dom, child, base_url, scripts);
        }
    }
}

fn collect_classic_script_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    scripts: &mut HashSet<silksurf_dom::NodeId>,
) {
    if script_ref_for_node(dom, node, base_url).is_some() {
        scripts.insert(node);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_classic_script_nodes(dom, child, base_url, scripts);
        }
    }
}

fn script_ref_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<DocumentScriptRef> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok();
    if !script_type_is_classic(attrs.as_deref()) {
        return None;
    }
    if let Some(src) = script_src(attrs.as_deref()) {
        let resolved = resolve_resource_url(base_url, src);
        return (!resolved.is_empty()).then_some(DocumentScriptRef::External(resolved));
    }
    let text = script_text_content(dom, node);
    (!text.trim().is_empty()).then_some(DocumentScriptRef::Inline(text))
}

fn script_type_is_classic(attrs: Option<&[silksurf_dom::Attribute]>) -> bool {
    let script_type = script_type_value(attrs);
    matches!(
        script_type,
        None | Some("" | "text/javascript" | "application/javascript")
    )
}

fn script_type_is_module(attrs: Option<&[silksurf_dom::Attribute]>) -> bool {
    script_type_value(attrs).is_some_and(|script_type| script_type.eq_ignore_ascii_case("module"))
}

fn script_type_value(attrs: Option<&[silksurf_dom::Attribute]>) -> Option<&str> {
    attrs.and_then(|attrs| {
        attrs
            .iter()
            .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("type"))
            .map(|attr| attr.value.as_str())
    })
}

fn script_src(attrs: Option<&[silksurf_dom::Attribute]>) -> Option<&str> {
    attrs?
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("src"))
        .map(|attr| attr.value.as_str())
        .filter(|src| !src.trim().is_empty())
}

fn script_text_content(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    let mut text = String::new();
    if let Ok(children) = dom.children(node) {
        for &child in children {
            if let Ok(n) = dom.node(child)
                && let silksurf_dom::NodeKind::Text { text: t } = n.kind()
            {
                text.push_str(t);
            }
        }
    }
    text
}

/// Extract text content from inline `<script>` tags.
fn extract_inline_scripts(dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) -> Vec<String> {
    let mut scripts = Vec::new();
    collect_script_tags(dom, root, &mut scripts);
    scripts
}

fn collect_script_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    scripts: &mut Vec<String>,
) {
    if let Some(DocumentScriptRef::Inline(text)) = script_ref_for_node(dom, node, "") {
        scripts.push(text);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_script_tags(dom, child, scripts);
        }
    }
}
