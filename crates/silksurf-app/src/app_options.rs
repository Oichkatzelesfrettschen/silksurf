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
            "--backend" | "--tls-ca-file" | "--display-backend" | "--screenshot" | "--monitor" => {
                skip_next = true;
            }
            _ if arg.starts_with("--backend=")
                || arg.starts_with("--tls-ca-file=")
                || arg.starts_with("--display-backend=")
                || arg.starts_with("--screenshot=")
                || arg.starts_with("--monitor=") => {}
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
    let screenshot = parse_path_arg(args, "--screenshot");
    let monitor = parse_monitor_arg(args, std::env::var("SILKSURF_MONITOR").ok().as_deref());
    let url = positional_url_arg(args).unwrap_or_else(|| "https://example.com".to_string());
    log_startup_options(insecure, platform_verifier, tls_ca_file.as_ref());
    Ok(AppOptions {
        speculative,
        window_mode,
        headless,
        display_backend,
        monitor,
        url,
        screenshot,
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

/*
 * Which monitor shows the browser window.
 *
 * `--list-monitors` prints the names the display server reports and exits.
 * `--monitor <name>` selects one of them. `SILKSURF_MONITOR` supplies the
 * default, so the connector name for a particular host lives in that host's
 * environment rather than in a checked-in file.
 */
pub(crate) fn parse_monitor_arg(
    args: &[String],
    env_default: Option<&str>,
) -> silksurf_gui::WinitMonitorChoice {
    if args.iter().any(|arg| arg == "--list-monitors") {
        return silksurf_gui::WinitMonitorChoice::List;
    }
    let flag = args
        .windows(2)
        .find_map(|window| (window[0] == "--monitor").then_some(window[1].as_str()))
        .or_else(|| args.iter().find_map(|arg| arg.strip_prefix("--monitor=")));
    match flag
        .or(env_default)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(selector) => silksurf_gui::WinitMonitorChoice::Named(selector.to_string()),
        None => silksurf_gui::WinitMonitorChoice::Compositor,
    }
}

pub(crate) fn parse_tls_ca_file_arg(args: &[String]) -> Option<std::path::PathBuf> {
    parse_path_arg(args, "--tls-ca-file")
}

/// A path option in either spelling: `--name value` or `--name=value`.
pub(crate) fn parse_path_arg(args: &[String], name: &str) -> Option<std::path::PathBuf> {
    let equals_form = format!("{name}=");
    args.windows(2)
        .find_map(|window| (window[0] == name).then(|| std::path::PathBuf::from(&window[1])))
        .or_else(|| {
            args.iter().find_map(|arg| {
                arg.strip_prefix(equals_form.as_str())
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
    fn a_screenshot_path_parses_in_both_spellings() {
        assert_eq!(
            parse_path_arg(
                &args(&["silksurf-app", "--screenshot", "/out/frame.png"]),
                "--screenshot"
            ),
            Some(std::path::PathBuf::from("/out/frame.png"))
        );
        assert_eq!(
            parse_path_arg(
                &args(&["silksurf-app", "--screenshot=/out/frame.png"]),
                "--screenshot"
            ),
            Some(std::path::PathBuf::from("/out/frame.png"))
        );
        assert_eq!(
            parse_path_arg(&args(&["silksurf-app"]), "--screenshot"),
            None
        );
    }

    #[test]
    fn a_screenshot_path_is_not_the_positional_url() {
        assert_eq!(
            positional_url_arg(&args(&[
                "silksurf-app",
                "--headless",
                "--screenshot",
                "/out/frame.png",
                "https://example.com/"
            ])),
            Some("https://example.com/".to_string())
        );
    }

    #[test]
    fn a_monitor_selector_parses_in_both_spellings_and_from_the_environment() {
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app", "--monitor", "DP-2"]), None),
            silksurf_gui::WinitMonitorChoice::Named("DP-2".to_string())
        );
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app", "--monitor=LG"]), None),
            silksurf_gui::WinitMonitorChoice::Named("LG".to_string())
        );
        // The flag wins over the environment default.
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app", "--monitor=LG"]), Some("DP-1")),
            silksurf_gui::WinitMonitorChoice::Named("LG".to_string())
        );
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app"]), Some("DP-1")),
            silksurf_gui::WinitMonitorChoice::Named("DP-1".to_string())
        );
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app"]), Some("  ")),
            silksurf_gui::WinitMonitorChoice::Compositor
        );
        assert_eq!(
            parse_monitor_arg(&args(&["silksurf-app"]), None),
            silksurf_gui::WinitMonitorChoice::Compositor
        );
    }

    #[test]
    fn listing_monitors_overrides_a_selector() {
        assert_eq!(
            parse_monitor_arg(
                &args(&["silksurf-app", "--monitor", "DP-2", "--list-monitors"]),
                Some("DP-1")
            ),
            silksurf_gui::WinitMonitorChoice::List
        );
    }

    #[test]
    fn a_monitor_selector_is_not_the_positional_url() {
        assert_eq!(
            positional_url_arg(&args(&[
                "silksurf-app",
                "--monitor",
                "DP-2",
                "https://example.com/"
            ])),
            Some("https://example.com/".to_string())
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
