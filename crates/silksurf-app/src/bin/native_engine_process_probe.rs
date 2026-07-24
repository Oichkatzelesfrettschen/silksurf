//! DG-1 control-plane process probe.
//!
//! Running this binary with no arguments spawns a second copy in worker mode,
//! exchanges protocol-v1 CreateView/ViewCreated and CloseView/ViewClosed
//! messages, and performs a clean shutdown. It intentionally does not move the
//! page runtime or frame bytes yet; those are the next issue #53 slices.

const FRAME_WIDTH: u32 = 1280;
const FRAME_HEIGHT: u32 = 800;

mod engine_process {
    use super::{FRAME_HEIGHT, FRAME_WIDTH};

    include!("../engine_process.rs");
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        args.push("--silksurf-native-engine-supervisor-probe".to_string());
    }
    let exit_code = engine_process::run_internal_engine_process_mode(&args).unwrap_or(64);
    std::process::exit(exit_code);
}
