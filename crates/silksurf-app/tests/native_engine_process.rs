use std::process::Command;

const PROBE_BINARY: &str = env!("CARGO_BIN_EXE_native_engine_process_probe");

#[test]
fn supervisor_probe_spawns_worker_and_round_trips_view_lifecycle() {
    let output = Command::new(PROBE_BINARY)
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
