//! test262 conformance test runner CLI
//!
//! WHY: SilkSurf needs a quantitative, reproducible measurement of how much
//! of the JavaScript language the bytecode VM actually executes. The earlier
//! revision of this binary lexed each test262 source and reported tokeniser
//! behaviour as if it were conformance, which materially overstated coverage.
//!
//! WHAT: A dual-mode runner.
//!
//!   1. VM-execute mode (default, P5.S1):
//!      - Walks `silksurf-js/conformance/test262/fixtures/` (or any `--dir`).
//!      - For every `*.js` file, parses + compiles + executes it on a fresh
//!        `Vm`. PASS = `Vm::execute` returned `Ok`. FAIL = parse error,
//!        compile error, runtime exception, or panic in the engine.
//!      - Emits a scorecard at
//!        `silksurf-js/conformance/test262-scorecard.json` (or `--scorecard`)
//!        in the SNAZZY-WAFFLE schema:
//!        `{total, pass, fail, skip, rate, timestamp, runner_version}`.
//!      - Exit 0 iff `rate >= 0.5`, exit 1 otherwise (CI gates on this).
//!
//!   2. Legacy lex-only mode (`--lex-only`):
//!      - Preserves the original behaviour for existing CI scripts that
//!        target the upstream `tc39/test262` corpus checked out at
//!        `silksurf-js/test262/`. The lex-only scorecard schema is the same
//!        as before so consumers do not break.
//!
//! HOW: Per-test we wrap the entire pipeline in `std::panic::catch_unwind`
//! using `AssertUnwindSafe` so a single bad fixture cannot abort the entire
//! run. The VM mutates state during execution but is dropped immediately
//! afterwards, so the unwind-safety assertion is sound for our use.
//!
//! Usage:
//!   test262                              # run synthetic fixtures via VM
//!   test262 --dir <path>                 # custom fixture dir, VM mode
//!   test262 --scorecard <path>           # write scorecard to <path>
//!   test262 --lex-only --test262 <root>  # legacy lexer-only conformance
//!   test262 -v                           # verbose per-file output
//!   test262 --list-features              # list supported features
//!   test262 -h                           # print help
//!
//! See: silksurf-js/src/vm/mod.rs run_script (test helper) for the parse +
//! compile + load-strings + execute pipeline this binary mirrors.

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;

use silksurf_js::bytecode::{Compiler, Constant};
use silksurf_js::parser::Parser;
use silksurf_js::parser::ast_arena::AstArena;
use silksurf_js::vm::Vm;
use silksurf_js::{Lexer, TokenKind};

/// Runner version stamped into every scorecard. Bump when the runner's
/// classification logic or output schema changes meaningfully.
const RUNNER_VERSION: &str = "synthetic-fixture-v1";

/// Default fixture directory for VM-execute mode.
///
/// WHY: vendoring the upstream tc39/test262 corpus (tens of thousands of
/// files) just to measure baseline coverage would bloat the repo. Until the
/// VM handles enough of the language to make per-file iteration tractable
/// against the real corpus, we ship a small synthetic fixture set co-located
/// with the runner.
const DEFAULT_FIXTURE_DIR: &str = "silksurf-js/conformance/test262/fixtures";

/// Default scorecard output path for VM-execute mode.
const DEFAULT_SCORECARD_PATH: &str = "silksurf-js/conformance/test262-scorecard.json";

/// Pass-rate threshold for exit-0. CI gates on this.
const PASS_RATE_GATE: f64 = 0.5;

/// CLI mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Walk `--dir`, parse+compile+execute each `*.js` on a fresh `Vm`.
    VmExecute,
    /// Walk `--test262`/`<PATH>`, lex each `*.js` and treat lex success as
    /// pass. Kept for backward compatibility with the existing
    /// `scripts/conformance_run.sh` invocation that points at the upstream
    /// tc39/test262 checkout.
    LexOnly,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut verbose = false;
    let mut list_features = false;
    let mut mode = Mode::VmExecute;
    let mut dir_arg: Option<PathBuf> = None;
    let mut test262_root = PathBuf::from("test262");
    let mut subset_path = String::new();
    let mut scorecard_arg: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "--list-features" => list_features = true,
            "--lex-only" => mode = Mode::LexOnly,
            "--vm" => mode = Mode::VmExecute,
            "--dir" => {
                i += 1;
                if i < args.len() {
                    dir_arg = Some(PathBuf::from(&args[i]));
                } else {
                    eprintln!("--dir requires a path argument");
                    std::process::exit(2);
                }
            }
            "--test262" => {
                i += 1;
                if i < args.len() {
                    test262_root = PathBuf::from(&args[i]);
                    // Switching to legacy mode is implied when the upstream
                    // root is named explicitly.
                    mode = Mode::LexOnly;
                } else {
                    eprintln!("--test262 requires a path argument");
                    std::process::exit(2);
                }
            }
            "--scorecard" => {
                i += 1;
                if i < args.len() {
                    scorecard_arg = Some(PathBuf::from(&args[i]));
                } else {
                    eprintln!("--scorecard requires a path argument");
                    std::process::exit(2);
                }
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            arg if !arg.starts_with('-') => {
                subset_path = arg.to_string();
            }
            other => {
                eprintln!("Unknown option: {other}");
                print_help();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    if list_features {
        print_features();
        return;
    }

    match mode {
        Mode::VmExecute => run_vm_mode(verbose, dir_arg, scorecard_arg),
        Mode::LexOnly => run_lex_mode(verbose, &test262_root, &subset_path, scorecard_arg),
    }
}

// ---------------------------------------------------------------------------
// VM-execute mode (P5.S1)
// ---------------------------------------------------------------------------

/// Per-file outcome in VM-execute mode.
#[derive(Debug)]
enum VmOutcome {
    Pass,
    Fail(String),
    Skip(String),
}

/// Aggregated VM-execute results.
#[derive(Default)]
struct VmTotals {
    total: usize,
    pass: usize,
    fail: usize,
    skip: usize,
}

impl VmTotals {
    fn rate(&self) -> f64 {
        let attempted = self.pass + self.fail;
        if attempted == 0 {
            0.0
        } else {
            self.pass as f64 / attempted as f64
        }
    }
}

fn run_vm_mode(verbose: bool, dir_arg: Option<PathBuf>, scorecard_arg: Option<PathBuf>) {
    let dir = dir_arg.unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURE_DIR));
    let scorecard = scorecard_arg.unwrap_or_else(|| PathBuf::from(DEFAULT_SCORECARD_PATH));

    println!("SilkSurfJS test262 Runner (VM mode)");
    println!("===================================");
    println!("Fixture dir: {}", dir.display());
    println!("Scorecard:   {}", scorecard.display());
    println!("Verbose:     {verbose}");
    println!();

    if !dir.is_dir() {
        eprintln!(
            "Error: fixture directory not found at {} (cwd: {})",
            dir.display(),
            env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string())
        );
        eprintln!("Provide --dir <path> or run from the silksurf repo root.");
        std::process::exit(2);
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if let Err(e) = collect_js_files(&dir, &mut files) {
        eprintln!("Error walking {}: {}", dir.display(), e);
        std::process::exit(2);
    }
    files.sort();

    let mut totals = VmTotals::default();
    let start = Instant::now();
    for path in &files {
        totals.total += 1;
        let outcome = run_single_vm_test(path);
        let rel = path.strip_prefix(&dir).unwrap_or(path);
        match outcome {
            VmOutcome::Pass => {
                totals.pass += 1;
                if verbose {
                    println!("PASS  {}", rel.display());
                }
            }
            VmOutcome::Fail(reason) => {
                totals.fail += 1;
                println!("FAIL  {}  -- {}", rel.display(), reason);
            }
            VmOutcome::Skip(reason) => {
                totals.skip += 1;
                if verbose {
                    println!("SKIP  {}  -- {}", rel.display(), reason);
                }
            }
        }
    }
    let duration = start.elapsed();

    println!();
    println!("------------------------------------------------------------");
    println!(
        "PASS: {}  FAIL: {}  SKIP: {}  TOTAL: {}  RATE: {:.2}",
        totals.pass,
        totals.fail,
        totals.skip,
        totals.total,
        totals.rate()
    );
    println!("Duration: {:.3}s", duration.as_secs_f64());

    if let Err(e) = emit_vm_scorecard(&scorecard, &totals, &dir, duration) {
        eprintln!(
            "WARN: failed to write scorecard {}: {}",
            scorecard.display(),
            e
        );
    } else {
        println!("Scorecard: {}", scorecard.display());
    }

    if totals.rate() >= PASS_RATE_GATE {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}

/// Recursively gather every `*.js` under `dir` into `out`.
///
/// WHY: `std::fs::read_dir` does not recurse; we want every fixture under
/// the tree regardless of subdirectory layout so contributors can group
/// fixtures into folders without changing the runner.
fn collect_js_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_js_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "js") {
            out.push(path);
        }
    }
    Ok(())
}

/// Per-test wall-clock budget. Anything longer than this is reported as
/// FAIL("timeout") rather than allowed to hang the whole run.
///
/// WHY: the SilkSurf VM has no built-in instruction-budget cutoff yet; an
/// infinite loop or pathological allocation in one fixture can otherwise
/// take down the entire conformance run. A wall-clock fence makes the
/// scorecard robust to those regressions and surfaces them as ordinary
/// failures rather than as "test runner died".
const PER_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Run one fixture: read source, parse, compile, execute. Wraps the entire
/// pipeline in `catch_unwind` (panics) and a worker thread (hangs) so a
/// single bad fixture cannot abort or freeze the run.
///
/// HOW:
///   - `catch_unwind` with `AssertUnwindSafe` traps engine panics. The Vm
///     is local to the closure and dropped on return, so the unwind-safety
///     assertion is sound.
///   - A spawned thread runs the pipeline and signals completion via an
///     `mpsc::sync_channel(1)`. If `recv_timeout` fires first, we report
///     timeout and let the worker thread leak. Leaking is acceptable here
///     because this is a one-shot CLI tool that exits after the run.
///   - The runner mirrors the `run_script` test helper in `vm/mod.rs`. We
///     reproduce the logic here (rather than expose it from the library)
///     because the helper lives in `#[cfg(test)]`; lifting it out is a
///     separate refactor.
fn run_single_vm_test(path: &Path) -> VmOutcome {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return VmOutcome::Fail(format!("read error: {e}")),
    };

    if source.trim().is_empty() {
        return VmOutcome::Skip("empty file".to_string());
    }

    // sync_channel(1) so the worker can store its result without blocking
    // on a vanished receiver if we time out and move on.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<VmOutcome>(1);

    // The closure owns the source string clone; the worker thread owns the
    // Vm instance entirely (constructed inside execute_source). The Send
    // requirement on closures-spawned-as-threads is satisfied because all
    // captured state (String, Sender) is Send.
    let source_for_worker = source;
    std::thread::Builder::new()
        .name(format!("test262-{}", path.display()))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute_source(&source_for_worker)
            }));
            let outcome = match result {
                Ok(Ok(())) => VmOutcome::Pass,
                Ok(Err(reason)) => VmOutcome::Fail(reason),
                Err(panic_payload) => {
                    let msg = panic_payload
                        .downcast_ref::<&'static str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "engine panic (no message)".to_string());
                    VmOutcome::Fail(format!("PANIC: {msg}"))
                }
            };
            // If the receiver has been dropped (we timed out), the send
            // simply returns Err and the outcome is discarded. That is the
            // intended behaviour.
            let _ = sender.send(outcome);
        })
        .map_or_else(
            |e| VmOutcome::Fail(format!("spawn error: {e}")),
            |_handle| match receiver.recv_timeout(PER_TEST_TIMEOUT) {
                Ok(outcome) => outcome,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    VmOutcome::Fail(format!("timeout: exceeded {}s", PER_TEST_TIMEOUT.as_secs()))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    VmOutcome::Fail("worker disconnected without sending result".to_string())
                }
            },
        )
}

/// Parse + compile + execute `source` on a fresh `Vm`.
///
/// Returns `Err(reason)` for any parse error, compile error, or runtime
/// VM error. Returns `Ok(())` only when the script ran to completion
/// without surfacing any error.
fn execute_source(source: &str) -> Result<(), String> {
    let ast_arena = AstArena::new();
    let parser = Parser::new(source, &ast_arena);
    let (ast, parse_errors) = parser.parse();
    if !parse_errors.is_empty() {
        return Err(format!("parse: {parse_errors:?}"));
    }

    let compiler = Compiler::new();
    let (chunk, child_chunks, string_pool) = compiler
        .compile_with_children(&ast)
        .map_err(|e| format!("compile: {e:?}"))?;

    let mut vm = Vm::new();

    // Re-intern compiler-side strings into the VM's string table and
    // remember the mapping so we can rewrite Constant::String indices in
    // the chunks below.
    let mut str_map: HashMap<u32, u32> = HashMap::with_capacity(string_pool.len());
    for (compiler_id, s) in &string_pool {
        let vm_id = vm.strings.intern(s.clone());
        str_map.insert(*compiler_id, vm_id);
    }

    // Append child chunks first; remember the base so we can shift the main
    // chunk's Constant::Function indices to point at them.
    let child_base = vm.chunks_len();
    for mut child in child_chunks {
        for constant in child.constants_mut() {
            if let Constant::String(str_id) = constant
                && let Some(&vm_id) = str_map.get(str_id)
            {
                *str_id = vm_id;
            }
        }
        vm.add_chunk(child);
    }

    let mut main_chunk = chunk;
    for constant in main_chunk.constants_mut() {
        match constant {
            Constant::Function(idx) => *idx += child_base as u32,
            Constant::String(str_id) => {
                if let Some(&vm_id) = str_map.get(str_id) {
                    *str_id = vm_id;
                }
            }
            _ => {}
        }
    }

    let chunk_idx = vm.add_chunk(main_chunk);
    vm.execute(chunk_idx).map_err(|e| format!("vm: {e:?}"))?;
    Ok(())
}

/// Emit the SNAZZY-WAFFLE scorecard for a VM-execute run.
///
/// Schema (stable):
///   total, pass, fail, skip   -- counts
///   rate                      -- pass / (pass + fail), 0.0..=1.0
///   timestamp                 -- RFC3339 UTC
///   runner_version            -- bumped when classification or schema changes
///   fixture_dir               -- where fixtures came from (provenance)
///   duration_secs             -- wall-clock for the run
fn emit_vm_scorecard(
    path: &Path,
    totals: &VmTotals,
    dir: &Path,
    duration: std::time::Duration,
) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let timestamp = rfc3339_now();
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{{")?;
    writeln!(f, "  \"total\": {},", totals.total)?;
    writeln!(f, "  \"pass\": {},", totals.pass)?;
    writeln!(f, "  \"fail\": {},", totals.fail)?;
    writeln!(f, "  \"skip\": {},", totals.skip)?;
    writeln!(f, "  \"rate\": {:.4},", totals.rate())?;
    writeln!(f, "  \"timestamp\": \"{timestamp}\",")?;
    writeln!(f, "  \"runner_version\": \"{RUNNER_VERSION}\",")?;
    writeln!(f, "  \"runner_kind\": \"vm\",")?;
    writeln!(f, "  \"fixture_dir\": \"{}\",", dir.display())?;
    writeln!(f, "  \"duration_secs\": {:.3}", duration.as_secs_f64())?;
    writeln!(f, "}}")?;
    Ok(())
}

/// Format the current UTC time as RFC3339 (e.g. "2026-05-15T17:32:08Z")
/// without pulling in chrono. Uses Howard Hinnant's civil-from-days
/// algorithm; valid for any year representable by i64 days from epoch.
///
/// WHY: adding a date crate just for one timestamp string is overkill.
/// The math is well-understood and small.
/// See: https://howardhinnant.github.io/date_algorithms.html
fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let days = now.div_euclid(86_400);
    let secs_of_day = now.rem_euclid(86_400);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    // civil_from_days: convert days-since-1970-01-01 to (year, month, day).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, m, d, hour, minute, second
    )
}

// ---------------------------------------------------------------------------
// Legacy lex-only mode
// ---------------------------------------------------------------------------

fn run_lex_mode(
    verbose: bool,
    test262_root: &Path,
    subset_path: &str,
    scorecard_arg: Option<PathBuf>,
) {
    println!("SilkSurfJS test262 Runner (lex-only legacy mode)");
    println!("================================================");

    let test_dir = if subset_path.is_empty() {
        test262_root.join("test")
    } else {
        test262_root.join("test").join(subset_path)
    };

    if !test_dir.exists() {
        eprintln!(
            "Error: test262 directory not found at {}",
            test_dir.display()
        );
        eprintln!();
        eprintln!("To set up the upstream tc39/test262 corpus:");
        eprintln!("  git clone https://github.com/tc39/test262.git silksurf-js/test262");
        std::process::exit(2);
    }

    println!("Test directory: {}", test_dir.display());
    println!("Verbose:        {verbose}");
    println!();

    let start = Instant::now();
    let results = run_lex_tests(&test_dir, verbose);
    let duration = start.elapsed();

    println!();
    println!("============================================================");
    println!("test262 Summary (lex-only)");
    println!("============================================================");
    println!("Total:   {}", results.total);
    println!(
        "Passed:  {} ({:.1}%)",
        results.passed,
        results.pass_rate_pct()
    );
    println!("Failed:  {}", results.failed);
    println!("Skipped: {}", results.skipped);
    println!("Time:    {:.2}s", duration.as_secs_f64());

    if let Some(path) = scorecard_arg
        && let Err(e) = emit_lex_scorecard(&path, &results, &test_dir, subset_path, duration)
    {
        eprintln!("WARN: failed to write scorecard {}: {}", path.display(), e);
    }

    if results.failed > 0 {
        std::process::exit(1);
    }
}

struct LexResults {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl LexResults {
    fn pass_rate_pct(&self) -> f64 {
        let run = self.passed + self.failed;
        if run == 0 {
            0.0
        } else {
            (self.passed as f64 / run as f64) * 100.0
        }
    }
}

fn run_lex_tests(test_dir: &Path, verbose: bool) -> LexResults {
    let mut results = LexResults {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
    };
    let skip_features: HashSet<String> = UNSUPPORTED_FEATURES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    collect_and_lex(test_dir, &mut results, verbose, &skip_features);
    results
}

fn collect_and_lex(
    dir: &Path,
    results: &mut LexResults,
    verbose: bool,
    skip_features: &HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "staging" || name == "intl402" || name.starts_with('_') {
                continue;
            }
            collect_and_lex(&path, results, verbose, skip_features);
        } else if path.extension().is_some_and(|e| e == "js") {
            lex_single_test(&path, results, verbose, skip_features);
        }
    }
}

fn lex_single_test(
    path: &Path,
    results: &mut LexResults,
    verbose: bool,
    skip_features: &HashSet<String>,
) {
    results.total += 1;

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            results.failed += 1;
            if verbose {
                eprintln!("FAIL [read error]: {} - {}", path.display(), e);
            }
            return;
        }
    };

    let metadata = parse_metadata(&content);

    if let Some(features) = &metadata.features {
        for feature in features {
            if skip_features.contains(feature.as_str()) {
                results.skipped += 1;
                if verbose {
                    println!("SKIP [{}]: {}", feature, path.display());
                }
                return;
            }
        }
    }

    if metadata.is_async || metadata.is_module {
        results.skipped += 1;
        if verbose {
            let reason = if metadata.is_async { "async" } else { "module" };
            println!("SKIP [{}]: {}", reason, path.display());
        }
        return;
    }

    let test_source = extract_test_source(&content);
    let outcome = run_lexer_test(&test_source, &metadata);

    match outcome {
        LexOutcome::Pass => {
            results.passed += 1;
            if verbose {
                println!("PASS: {}", path.display());
            }
        }
        LexOutcome::Fail(reason) => {
            results.failed += 1;
            if verbose {
                eprintln!("FAIL: {} - {}", path.display(), reason);
            }
        }
    }
}

#[derive(Default)]
struct SimpleMetadata {
    features: Option<Vec<String>>,
    is_async: bool,
    is_module: bool,
    is_negative_parse: bool,
    negative_type: Option<String>,
}

fn parse_metadata(content: &str) -> SimpleMetadata {
    let mut meta = SimpleMetadata::default();
    let Some(start) = content.find("/*---") else {
        return meta;
    };
    let Some(end) = content[start..].find("---*/") else {
        return meta;
    };
    let yaml = &content[start + 5..start + end];

    for line in yaml.lines() {
        let line = line.trim();
        if line.starts_with("flags:") {
            let flags = line.strip_prefix("flags:").unwrap_or("").trim();
            if flags.contains("async") {
                meta.is_async = true;
            }
            if flags.contains("module") {
                meta.is_module = true;
            }
        } else if line.starts_with("features:") {
            let features_str = line.strip_prefix("features:").unwrap_or("").trim();
            let features: Vec<String> = features_str
                .trim_matches(['[', ']'])
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            meta.features = Some(features);
        } else if line.starts_with("phase:") && line.contains("parse") {
            meta.is_negative_parse = true;
        } else if line.starts_with("type:") {
            meta.negative_type = Some(line.strip_prefix("type:").unwrap_or("").trim().to_string());
        }
    }

    meta
}

fn extract_test_source(content: &str) -> String {
    if let Some(end) = content.find("---*/") {
        content[end + 5..].to_string()
    } else {
        content.to_string()
    }
}

enum LexOutcome {
    Pass,
    Fail(String),
}

fn run_lexer_test(source: &str, metadata: &SimpleMetadata) -> LexOutcome {
    let lexer = Lexer::new(source);
    let mut has_error = false;
    let mut error_msg = String::new();

    for token in lexer {
        if let TokenKind::Error(e) = token.kind {
            has_error = true;
            error_msg = e.to_string();
            break;
        }
    }

    if metadata.is_negative_parse {
        if has_error {
            LexOutcome::Pass
        } else {
            LexOutcome::Fail(format!(
                "Expected parse error {} but succeeded",
                metadata.negative_type.as_deref().unwrap_or("unknown")
            ))
        }
    } else if has_error {
        LexOutcome::Fail(error_msg)
    } else {
        LexOutcome::Pass
    }
}

fn emit_lex_scorecard(
    path: &Path,
    results: &LexResults,
    test_dir: &Path,
    subset_path: &str,
    duration: std::time::Duration,
) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{{")?;
    writeln!(f, "  \"runner\": \"silksurf-js test262 (lexer-only)\",")?;
    writeln!(f, "  \"runner_version\": \"0.1.0\",")?;
    writeln!(f, "  \"unix_timestamp\": {now},")?;
    writeln!(f, "  \"test262_root\": \"{}\",", test_dir.display())?;
    let path_field = if subset_path.is_empty() {
        "<all>"
    } else {
        subset_path
    };
    writeln!(f, "  \"path_filter\": \"{path_field}\",")?;
    writeln!(f, "  \"total\": {},", results.total)?;
    writeln!(f, "  \"passed\": {},", results.passed)?;
    writeln!(f, "  \"failed\": {},", results.failed)?;
    writeln!(f, "  \"skipped\": {},", results.skipped)?;
    writeln!(f, "  \"pass_rate_pct\": {:.2},", results.pass_rate_pct())?;
    writeln!(f, "  \"duration_secs\": {:.3},", duration.as_secs_f64())?;
    writeln!(f, "  \"runner_kind\": \"lexer\",")?;
    writeln!(
        f,
        "  \"runner_kind_upgrade_path\": \"vm (use --vm or omit --lex-only; see SNAZZY-WAFFLE P5.S1)\","
    )?;
    writeln!(
        f,
        "  \"notes\": \"This runner only validates that each test262 file lexes; it does not parse, compile, or evaluate. The pass/fail counts therefore reflect tokeniser behaviour, not language conformance.\"\n}}"
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Help and feature listings
// ---------------------------------------------------------------------------

fn print_help() {
    println!("SilkSurfJS test262 Conformance Test Runner");
    println!();
    println!("USAGE:");
    println!("    test262 [OPTIONS] [PATH]");
    println!();
    println!("DEFAULT MODE: --vm");
    println!("    Walks --dir (default: {DEFAULT_FIXTURE_DIR}),");
    println!("    parses + compiles + executes each .js on a fresh Vm,");
    println!("    writes a scorecard at --scorecard");
    println!("    (default: {DEFAULT_SCORECARD_PATH}),");
    println!("    exit 0 iff pass rate >= {PASS_RATE_GATE:.2}.");
    println!();
    println!("LEGACY MODE: --lex-only");
    println!("    Walks --test262 <root>/test/[PATH], lexes each .js, treats");
    println!("    lex success as conformance pass. For backward compatibility");
    println!("    with the upstream tc39/test262 corpus only.");
    println!();
    println!("ARGS:");
    println!("    [PATH]    (lex-only) Subdirectory under <test262-root>/test/");
    println!();
    println!("OPTIONS:");
    println!("    -v, --verbose        Verbose per-file output");
    println!("        --vm             Force VM-execute mode (default)");
    println!("        --lex-only       Force lex-only mode");
    println!("        --dir <path>     Fixture directory for --vm mode");
    println!("        --test262 <path> tc39/test262 root for --lex-only mode");
    println!("        --scorecard <p>  Output JSON scorecard path");
    println!("        --list-features  List supported/unsupported features");
    println!("    -h, --help           Print this help");
}

fn print_features() {
    println!("Supported Features:");
    for feature in SUPPORTED_FEATURES {
        println!("  [+] {feature}");
    }
    println!();
    println!("Unsupported Features (tests skipped in lex-only mode):");
    for feature in UNSUPPORTED_FEATURES {
        println!("  [-] {feature}");
    }
}

const SUPPORTED_FEATURES: &[&str] = &[
    "let",
    "const",
    "arrow-function",
    "class",
    "template",
    "default-parameters",
    "rest-parameters",
    "spread",
    "destructuring-binding",
    "for-of",
    "Symbol",
    "generators",
    "Promise",
    "async-functions",
    "BigInt",
    "optional-chaining",
    "nullish-coalescing",
    "numeric-separator-literal",
    "object-rest",
    "object-spread",
];

const UNSUPPORTED_FEATURES: &[&str] = &[
    "Temporal",
    "ShadowRealm",
    "decorators",
    "regexp-v-flag",
    "iterator-helpers",
    "set-methods",
    "promise-with-resolvers",
    "ArrayBuffer.prototype.transfer",
    "resizable-arraybuffer",
    "arraybuffer-transfer",
    "Atomics",
    "SharedArrayBuffer",
    "Atomics.waitAsync",
    "FinalizationRegistry",
    "WeakRef",
    "Intl.Locale",
    "Intl.ListFormat",
    "Intl.Segmenter",
    "Intl.DurationFormat",
    "Intl.DisplayNames",
    "Intl.NumberFormat-v3",
    "import.meta",
    "dynamic-import",
    "top-level-await",
    "json-modules",
    "import-assertions",
    "import-attributes",
    "Atomics.pause",
    "explicit-resource-management",
    "regexp-duplicate-named-groups",
    "symbols-as-weakmap-keys",
    "change-array-by-copy",
    "array-grouping",
    "well-formed-json-stringify",
    "String.prototype.isWellFormed",
    "String.prototype.toWellFormed",
    "Array.fromAsync",
];
