use std::process::Command;

/// The browser binary owns the engine process modes, so the supervisor probe
/// re-execs the same executable the shell ships.
const BROWSER_BINARY: &str = env!("CARGO_BIN_EXE_silksurf-app");
const SUPERVISOR_PROBE_FLAG: &str = "--silksurf-native-engine-supervisor-probe";
const WORKER_FLAG: &str = "--silksurf-native-engine-worker";

#[test]
fn supervisor_probe_spawns_worker_and_round_trips_view_lifecycle() {
    let output = Command::new(BROWSER_BINARY)
        .arg(SUPERVISOR_PROBE_FLAG)
        .output()
        .expect("native engine supervisor probe must start");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "probe failed with {:?}: {stderr}",
        output.status.code()
    );
    assert!(stderr.contains("Native engine supervisor probe: OK"));
}

/// The worker flag reaches the worker loop before `parse_app_options`, which
/// ignores unrecognized `-` arguments and would otherwise open a browser
/// window on the default URL. Closed stdin ends the worker loop at once.
#[test]
fn worker_flag_claims_the_process_before_browser_option_parsing() {
    let output = Command::new(BROWSER_BINARY)
        .arg(WORKER_FLAG)
        .output()
        .expect("native engine worker mode must start");
    assert!(
        output.status.success(),
        "worker exited with {:?}",
        output.status.code()
    );
    assert!(
        output.stdout.is_empty(),
        "worker emitted unrequested events"
    );
}
