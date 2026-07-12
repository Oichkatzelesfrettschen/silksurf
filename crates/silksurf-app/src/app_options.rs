// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn parse_display_backend_arg(
    args: &[String],
) -> Result<silksurf_gui::WinitDisplayBackend, String> {
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

pub(crate) fn positional_url_arg(args: &[String]) -> Option<String> {
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

pub(crate) fn install_observability() {
    #[cfg(feature = "structured-tracing")]
    install_structured_tracing();
    install_panic_hook();
}

pub(crate) fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("[SilkSurf] process panicking: {info}");
        default_hook(info);
    }));
}

#[cfg(feature = "structured-tracing")]
pub(crate) fn install_structured_tracing() {
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

pub(crate) fn parse_app_options(args: &[String]) -> Result<AppOptions, String> {
    let insecure = args.iter().any(|arg| arg == "--insecure" || arg == "-k");
    let platform_verifier = args.iter().any(|arg| arg == "--platform-verifier");
    let speculative = args.iter().any(|arg| arg == "--speculative" || arg == "-s");
    let window_mode = args.iter().any(|arg| arg == "--window");
    // --backend=winit stays accepted for compatibility; the windowed UI is
    // the default, so only --headless changes the launch mode.
    let headless = args.iter().any(|arg| arg == "--headless");
    let display_backend = parse_display_backend_arg(args)?;
    let tls_ca_file = parse_tls_ca_file_arg(args);
    let url = positional_url_arg(args).unwrap_or_else(|| "https://example.com".to_string());
    log_startup_options(insecure, platform_verifier, tls_ca_file.as_ref());
    Ok(AppOptions {
        speculative,
        window_mode,
        headless,
        display_backend,
        url,
        render_config: BrowserRenderConfig {
            insecure,
            platform_verifier,
            tls_ca_file,
            cookie_jar: std::sync::Arc::default(),
            // Set per navigation from the destination URL (see load_navigation_payload).
            top_level_site: String::new(),
        },
    })
}

pub(crate) fn parse_tls_ca_file_arg(args: &[String]) -> Option<std::path::PathBuf> {
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

pub(crate) fn log_startup_options(
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

pub(crate) fn run_legacy_window_mode() -> ! {
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
pub(crate) fn run_legacy_xcb_window(window: &mut silksurf_gui::XcbWindow) -> ! {
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

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

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

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }
}
