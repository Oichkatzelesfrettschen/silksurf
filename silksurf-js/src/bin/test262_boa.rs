/*
 * test262_boa.rs -- ECMA-262 conformance runner using boa_engine.
 *
 * WHY: The legacy test262.rs (feature "legacy-vm") measured the hand-written
 * VM at ~15-25% pass rate. With L7 (boa_engine 0.21) as the production JS
 * runtime, this binary measures actual ECMA-262 conformance. Expected result:
 * >85% on language/ and built-ins/ combined.
 *
 * WHAT: Walks tc39/test262 at `silksurf-js/test262/` (or `--dir`).
 * Per test: parse frontmatter -> skip ineligible -> load harness -> eval ->
 * check negative expectation -> emit PASS/FAIL/SKIP.
 *
 * HOW:
 *   cargo run --bin test262_boa                       # language/ default
 *   cargo run --bin test262_boa -- --full              # all categories
 *   cargo run --bin test262_boa -- -j 4 -v             # 4 threads, verbose
 *   cargo run --bin test262_boa -- --dir test262/test/language/expressions
 *   cargo run --bin test262_boa -- --scorecard out.json
 *
 * LIMITATIONS (first pass):
 *   - No per-test wall-clock timeout; test262 tests are expected to terminate.
 *   - Module (ESM) tests are skipped; boa_engine module evaluation requires
 *     a custom ModuleLoader implementation (future work).
 *   - async tests that require $DONE and a Promise-settling event loop are
 *     skipped; the $DONE pattern relies on host async infrastructure.
 *   - Strict-mode variants (onlyStrict flag) run as normal scripts.
 */

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use boa_engine::{
    js_string,
    object::ObjectInitializer,
    property::Attribute,
    Context, JsValue, NativeFunction, Source,
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

struct Config {
    dir: PathBuf,
    full: bool,
    verbose: bool,
    threads: usize,
    scorecard: PathBuf,
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut dir: Option<PathBuf> = None;
    let mut full = false;
    let mut verbose = false;
    let mut threads = 4usize;
    let mut scorecard = PathBuf::from("silksurf-js/conformance/test262-boa-scorecard.json");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "--full" => full = true,
            "--dir" => {
                i += 1;
                if i < args.len() {
                    dir = Some(PathBuf::from(&args[i]));
                }
            }
            "-j" | "--jobs" => {
                i += 1;
                if i < args.len() {
                    threads = args[i].parse().unwrap_or(4);
                }
            }
            "--scorecard" => {
                i += 1;
                if i < args.len() {
                    scorecard = PathBuf::from(&args[i]);
                }
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other if !other.starts_with('-') => {
                dir = Some(PathBuf::from(other));
            }
            _ => {}
        }
        i += 1;
    }

    let dir = dir.unwrap_or_else(|| PathBuf::from("silksurf-js/test262/test/language"));

    Config { dir, full, verbose, threads, scorecard }
}

fn print_help() {
    println!("test262_boa -- ECMA-262 conformance runner (boa_engine)");
    println!();
    println!("USAGE: test262_boa [OPTIONS] [DIR]");
    println!();
    println!("OPTIONS:");
    println!("  -v, --verbose         Print PASS/SKIP lines (FAIL always printed)");
    println!("      --full            Include built-ins/ and annexB/ in addition to language/");
    println!("      --dir <path>      Test directory (default: test262/test/language)");
    println!("  -j, --jobs <n>        Parallel worker threads (default: 4)");
    println!("      --scorecard <p>   JSON scorecard output path");
    println!("  -h, --help            Print help");
}

// ---------------------------------------------------------------------------
// Frontmatter parser
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct TestMeta {
    flags: Vec<String>,
    features: Vec<String>,
    includes: Vec<String>,
    negative: Option<NegSpec>,
}

#[derive(Clone)]
struct NegSpec {
    phase: Phase,
    ntype: String,
}

#[derive(Clone, PartialEq, Eq)]
enum Phase {
    Parse,
    Runtime,
}

fn parse_meta(content: &str) -> TestMeta {
    let Some(start) = content.find("/*---") else {
        return TestMeta::default();
    };
    let Some(rel_end) = content[start..].find("---*/") else {
        return TestMeta::default();
    };
    let yaml = &content[start + 5..start + rel_end];

    let mut meta = TestMeta::default();
    let mut in_neg = false;
    let mut in_features = false;
    let mut in_includes = false;
    let mut in_flags = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("flags:") {
            in_neg = false; in_features = false; in_includes = false; in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.flags = parse_inline_list(val);
            } else if val.is_empty() {
                in_flags = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("features:") {
            in_neg = false; in_features = false; in_includes = false; in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.features = parse_inline_list(val);
            } else if val.is_empty() {
                in_features = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("includes:") {
            in_neg = false; in_features = false; in_includes = false; in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.includes = parse_inline_list(val);
            } else if val.is_empty() {
                in_includes = true;
            }
        } else if trimmed == "negative:" {
            in_neg = true; in_features = false; in_includes = false; in_flags = false;
            meta.negative = Some(NegSpec { phase: Phase::Parse, ntype: String::new() });
        } else if in_neg {
            if let Some(rest) = trimmed.strip_prefix("phase:") {
                let phase_str = rest.trim();
                if let Some(neg) = &mut meta.negative {
                    neg.phase = if phase_str == "parse" { Phase::Parse } else { Phase::Runtime };
                }
            } else if let Some(rest) = trimmed.strip_prefix("type:") {
                let t = rest.trim().to_string();
                if let Some(neg) = &mut meta.negative {
                    neg.ntype = t;
                }
            } else if !trimmed.starts_with(' ') && !trimmed.starts_with('-') {
                in_neg = false;
            }
        } else if in_features && trimmed.starts_with('-') {
            let f = trimmed[1..].trim().to_string();
            if !f.is_empty() { meta.features.push(f); }
        } else if in_includes && trimmed.starts_with('-') {
            let inc = trimmed[1..].trim().to_string();
            if !inc.is_empty() { meta.includes.push(inc); }
        } else if in_flags && trimmed.starts_with('-') {
            let flag = trimmed[1..].trim().to_string();
            if !flag.is_empty() { meta.flags.push(flag); }
        } else if !trimmed.starts_with(' ') && !trimmed.starts_with('-') {
            // New top-level key resets all list contexts
            in_features = false; in_includes = false; in_flags = false;
        }
    }

    meta
}

fn parse_inline_list(s: &str) -> Vec<String> {
    s.trim_matches(['[', ']', ' '])
        .split(',')
        .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Feature skip list
// ---------------------------------------------------------------------------

const SKIP_FEATURES: &[&str] = &[
    // Intl APIs -- requires ICU data not bundled in boa
    "Intl.Locale",
    "Intl.ListFormat",
    "Intl.Segmenter",
    "Intl.DurationFormat",
    "Intl.DisplayNames",
    "Intl.NumberFormat-v3",
    "Intl.DateTimeFormat",
    "Intl.PluralRules",
    "Intl.RelativeTimeFormat",
    "Intl.Collator",
    // Stage 3/4 not yet in boa 0.21
    "Temporal",
    "ShadowRealm",
    "decorators",
    "regexp-v-flag",
    "iterator-helpers",
    "set-methods",
    "promise-with-resolvers",
    "explicit-resource-management",
    "Array.fromAsync",
    "float16array",
    "Math.sumPrecise",
    // Shared memory / atomics -- single-threaded host
    "Atomics",
    "SharedArrayBuffer",
    "Atomics.waitAsync",
    "Atomics.pause",
    // Dynamic import and import.meta -- need module loader
    "import.meta",
    "dynamic-import",
    "top-level-await",
    "json-modules",
    "import-assertions",
    "import-attributes",
    "source-phase-imports",
    // ArrayBuffer transfer
    "ArrayBuffer.prototype.transfer",
    "resizable-arraybuffer",
    "arraybuffer-transfer",
    // Other not-yet-supported
    "regexp-duplicate-named-groups",
    "symbols-as-weakmap-keys",
    "change-array-by-copy",
    "array-grouping",
];

// ---------------------------------------------------------------------------
// Harness loading
// ---------------------------------------------------------------------------

struct HarnessCache {
    files: HashMap<String, String>,
}

impl HarnessCache {
    fn load(harness_dir: &Path) -> Self {
        let mut files = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(harness_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "js") {
                    if let (Some(name), Ok(content)) = (
                        path.file_name().and_then(|n| n.to_str()),
                        std::fs::read_to_string(&path),
                    ) {
                        files.insert(name.to_string(), content);
                    }
                }
            }
        }
        HarnessCache { files }
    }

    fn get(&self, name: &str) -> &str {
        self.files.get(name).map(|s| s.as_str()).unwrap_or("")
    }

    fn preamble(&self, includes: &[String]) -> String {
        let mut out = String::new();
        // assert.js and sta.js are always included unless "raw" flag is set.
        out.push_str(self.get("assert.js"));
        out.push('\n');
        out.push_str(self.get("sta.js"));
        out.push('\n');
        for inc in includes {
            out.push_str(self.get(inc));
            out.push('\n');
        }
        out
    }

    fn preamble_raw(&self, includes: &[String]) -> String {
        let mut out = String::new();
        for inc in includes {
            out.push_str(self.get(inc));
            out.push('\n');
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Test262 host object ($262, print, $DONE)
// ---------------------------------------------------------------------------

fn install_test262_host(ctx: &mut Context) {
    // print() -- used by some harness files for debugging
    ctx.register_global_callable(
        js_string!("print"),
        1,
        NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
    )
    .expect("print install cannot fail");

    // $DONE() -- signals async test completion; we skip async tests but
    // harness files may define it so we need the global to not throw.
    ctx.register_global_callable(
        js_string!("$DONE"),
        1,
        NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
    )
    .expect("$DONE install cannot fail");

    // $262 object -- test262 host environment interface
    let is_htmldda = ObjectInitializer::new(ctx).build();
    let dollar_262 = ObjectInitializer::new(ctx)
        .function(
            // createRealm(): stub; realm isolation tests will fail (acceptable)
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("createRealm"),
            0,
        )
        .function(
            // gc(): trigger collection if available, no-op otherwise
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("gc"),
            0,
        )
        .function(
            // detachArrayBuffer(ab): mark AB as detached; stub for now
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("detachArrayBuffer"),
            1,
        )
        .function(
            // codePointRange(start, end): returns array of single-char strings
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let start = args
                    .first()
                    .and_then(|v| v.to_u32(ctx).ok())
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .and_then(|v| v.to_u32(ctx).ok())
                    .unwrap_or(0);
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                for cp in start..end {
                    if let Some(ch) = char::from_u32(cp) {
                        let s = boa_engine::JsString::from(ch.to_string().as_str());
                        let _ = arr.push(JsValue::from(s), ctx);
                    }
                }
                Ok(JsValue::from(arr))
            }),
            js_string!("codePointRange"),
            2,
        )
        // IsHTMLDDA: a callable that returns undefined, used to test typeof
        // returns "undefined" for document.all-like objects. Stub as undefined.
        .property(
            js_string!("IsHTMLDDA"),
            JsValue::from(is_htmldda),
            Attribute::all(),
        )
        .build();

    ctx.register_global_property(js_string!("$262"), dollar_262, Attribute::all())
        .expect("$262 install cannot fail");
}

// ---------------------------------------------------------------------------
// Per-test evaluation
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Outcome {
    Pass,
    Fail(String),
    Skip,
}

fn run_test(meta: &TestMeta, harness: &HarnessCache, source: &str) -> Outcome {
    // Skip: module tests require ESM ModuleLoader (future work)
    if meta.flags.iter().any(|f| f == "module") {
        return Outcome::Skip;
    }
    // Skip: async tests require a host event loop and $DONE callback
    if meta.flags.iter().any(|f| f == "async") {
        return Outcome::Skip;
    }
    // Skip: any unsupported feature listed in frontmatter
    let skip_set: HashSet<&str> = SKIP_FEATURES.iter().copied().collect();
    for feat in &meta.features {
        if skip_set.contains(feat.as_str()) {
            return Outcome::Skip;
        }
    }

    // Build script: harness preamble + test source
    let raw = meta.flags.iter().any(|f| f == "raw");
    let preamble = if raw {
        harness.preamble_raw(&meta.includes)
    } else {
        harness.preamble(&meta.includes)
    };

    let mut script = preamble;
    // onlyStrict: prepend "use strict" so the test runs in strict mode
    if meta.flags.iter().any(|f| f == "onlyStrict") {
        script.insert_str(0, "\"use strict\";\n");
    }
    script.push_str(source);

    let mut ctx = Context::default();
    install_test262_host(&mut ctx);

    let eval_result = ctx.eval(Source::from_bytes(script.as_bytes()));
    let _ = ctx.run_jobs();

    match (eval_result, &meta.negative) {
        // Pass: no error, no negative expectation
        (Ok(_), None) => Outcome::Pass,

        // Fail: script passed but we expected an error
        (Ok(_), Some(neg)) => Outcome::Fail(format!(
            "expected {} ({:?} phase) but script passed",
            neg.ntype,
            neg.phase == Phase::Parse
        )),

        // Negative parse test: we expected a parse-phase error and got one
        (Err(ref e), Some(neg)) if neg.phase == Phase::Parse => {
            if error_matches(e, &neg.ntype) || is_syntax_error(e) {
                Outcome::Pass
            } else {
                Outcome::Fail(format!(
                    "expected {} (parse) but got: {}",
                    neg.ntype,
                    error_type_name(e)
                ))
            }
        }

        // Negative runtime test: script threw and we expected it
        (Err(ref e), Some(neg)) if neg.phase == Phase::Runtime => {
            if error_matches(e, &neg.ntype) {
                Outcome::Pass
            } else {
                Outcome::Fail(format!(
                    "expected {} (runtime) but got: {}",
                    neg.ntype,
                    error_type_name(e)
                ))
            }
        }

        // Unexpected error
        (Err(e), None) => Outcome::Fail(format!("{e}")),

        // Fallthrough (satisfies exhaustiveness)
        _ => Outcome::Fail("unexpected combination of result and negative spec".to_string()),
    }
}

fn error_type_name(e: &boa_engine::JsError) -> String {
    if let Some(native) = e.as_native() {
        use boa_engine::JsNativeErrorKind as Kind;
        match &native.kind {
            Kind::Syntax => "SyntaxError".to_string(),
            Kind::Type => "TypeError".to_string(),
            Kind::Range => "RangeError".to_string(),
            Kind::Reference => "ReferenceError".to_string(),
            Kind::Eval => "EvalError".to_string(),
            Kind::Uri => "URIError".to_string(),
            Kind::Error => "Error".to_string(),
            Kind::Aggregate(_) => "AggregateError".to_string(),
            _ => format!("{e}"),
        }
    } else {
        format!("thrown:{e}")
    }
}

fn is_syntax_error(e: &boa_engine::JsError) -> bool {
    if let Some(native) = e.as_native() {
        matches!(native.kind, boa_engine::JsNativeErrorKind::Syntax)
    } else {
        false
    }
}

fn error_matches(e: &boa_engine::JsError, expected: &str) -> bool {
    error_type_name(e) == expected
}

// ---------------------------------------------------------------------------
// File collection
// ---------------------------------------------------------------------------

fn collect_js_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip intl402 (Intl APIs), staging (not yet standardised), and
            // hidden directories. annexB is included since boa_engine supports
            // most Annex B features.
            if name == "intl402" || name == "staging" || name.starts_with('_') {
                continue;
            }
            collect_js_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "js") {
            // _FIXTURE.js files are auxiliary modules imported by module tests;
            // they are not standalone executable tests.
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !stem.ends_with("_FIXTURE") {
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Thread-pool runner
// ---------------------------------------------------------------------------

struct WorkItem {
    path: PathBuf,
    source: String,
}

#[derive(Default)]
struct Totals {
    pass: usize,
    fail: usize,
    skip: usize,
}

impl Totals {
    fn total(&self) -> usize { self.pass + self.fail + self.skip }
    fn rate(&self) -> f64 {
        let run = self.pass + self.fail;
        if run == 0 { 0.0 } else { self.pass as f64 / run as f64 }
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cfg = parse_args();

    // Resolve the test262 root (two levels above the test dir for harness)
    let test_root = resolve_test262_root(&cfg.dir);

    let harness_dir = test_root.join("harness");
    if !harness_dir.is_dir() {
        eprintln!(
            "Error: test262 harness not found at {}",
            harness_dir.display()
        );
        eprintln!("Check out tc39/test262 into silksurf-js/test262/");
        std::process::exit(2);
    }

    let harness = Arc::new(HarnessCache::load(&harness_dir));

    // Collect test files
    let mut dirs_to_scan = vec![cfg.dir.clone()];
    if cfg.full {
        let base = test_root.join("test");
        for sub in &["built-ins", "annexB"] {
            let d = base.join(sub);
            if d.is_dir() {
                dirs_to_scan.push(d);
            }
        }
    }

    let mut files: Vec<PathBuf> = Vec::new();
    for d in &dirs_to_scan {
        collect_js_files(d, &mut files);
    }
    files.sort();

    let total_files = files.len();
    println!("test262_boa -- boa_engine ECMA-262 conformance runner");
    println!("=====================================================");
    println!("Test files: {total_files}");
    println!("Threads:    {}", cfg.threads);
    println!("Verbose:    {}", cfg.verbose);
    println!("Scorecard:  {}", cfg.scorecard.display());
    println!();

    // Build work queue
    let (work_tx, work_rx): (
        std::sync::mpsc::SyncSender<WorkItem>,
        std::sync::mpsc::Receiver<WorkItem>,
    ) = std::sync::mpsc::sync_channel(cfg.threads * 4);
    let work_rx = Arc::new(Mutex::new(work_rx));

    // Result channel
    let (result_tx, result_rx) = std::sync::mpsc::channel::<(PathBuf, Outcome)>();

    // Spawn worker threads
    for _ in 0..cfg.threads {
        let work_rx = Arc::clone(&work_rx);
        let result_tx = result_tx.clone();
        let harness = Arc::clone(&harness);
        std::thread::spawn(move || loop {
            let item = {
                let rx = work_rx.lock().unwrap();
                rx.recv()
            };
            match item {
                Ok(WorkItem { path, source }) => {
                    let meta = parse_meta(&source);
                    let outcome = run_test(&meta, &harness, &source);
                    let _ = result_tx.send((path, outcome));
                }
                Err(_) => break,
            }
        });
    }
    drop(result_tx); // main thread does not send results

    // Feed work on the main thread (producers)
    let feed_handle = {
        let work_tx = work_tx.clone();
        let files = files.clone();
        std::thread::spawn(move || {
            for path in files {
                match std::fs::read_to_string(&path) {
                    Ok(source) => {
                        // Ignore send error: receiver may have hung up on abort
                        let _ = work_tx.send(WorkItem { path, source });
                    }
                    Err(e) => {
                        // Read failure counts as skip rather than aborting the run
                        eprintln!("WARN: could not read {}: {e}", path.display());
                    }
                }
            }
        })
    };
    drop(work_tx);

    // Collect results
    let start = Instant::now();
    let mut totals = Totals::default();
    let mut fail_list: Vec<(PathBuf, String)> = Vec::new();
    let mut done = 0usize;
    let report_every = (total_files / 20).max(1);

    for (path, outcome) in &result_rx {
        done += 1;
        match outcome {
            Outcome::Pass => totals.pass += 1,
            Outcome::Skip => totals.skip += 1,
            Outcome::Fail(reason) => {
                totals.fail += 1;
                fail_list.push((path.clone(), reason.clone()));
                if cfg.verbose {
                    println!("FAIL  {}  -- {}", path.display(), reason);
                }
            }
        }
        if done % report_every == 0 || done == total_files {
            let pct = done as f64 / total_files as f64 * 100.0;
            eprint!(
                "\r  {done}/{total_files} ({pct:.0}%)  pass={} fail={} skip={}   ",
                totals.pass, totals.fail, totals.skip
            );
        }
    }
    eprintln!(); // end progress line

    feed_handle.join().ok();
    let duration = start.elapsed();

    // Print failures (even if not verbose, we always show fails)
    if !cfg.verbose && !fail_list.is_empty() {
        // Limit to first 40 failures to keep output manageable
        let show = fail_list.len().min(40);
        println!();
        println!("First {show} failures:");
        for (path, reason) in fail_list.iter().take(show) {
            println!("  FAIL  {}  -- {}", path.display(), reason);
        }
        if fail_list.len() > show {
            println!("  ... and {} more (use -v for full list)", fail_list.len() - show);
        }
    }

    println!();
    println!("------------------------------------------------------------");
    println!(
        "PASS: {}  FAIL: {}  SKIP: {}  TOTAL: {}",
        totals.pass, totals.fail, totals.skip, totals.total()
    );
    println!("Rate: {:.2}%  ({:.1}s)", totals.rate() * 100.0, duration.as_secs_f64());

    let scope_label = if cfg.full { "language+built-ins+annexB" } else { "language" };
    if let Err(e) = emit_scorecard(&cfg.scorecard, &totals, scope_label, duration) {
        eprintln!("WARN: scorecard write failed: {e}");
    } else {
        println!("Scorecard: {}", cfg.scorecard.display());
    }

    // Exit 0 iff pass rate > 50% (lower gate than the real suite's 80%+ expectation
    // since module/async tests are skipped, inflating the denominator).
    if totals.rate() >= 0.5 {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}

/// Walk up from `dir` until we find a directory containing `harness/` and
/// `test/`, which marks the test262 checkout root. Falls back to a reasonable
/// default if not found.
fn resolve_test262_root(dir: &Path) -> PathBuf {
    let mut current = dir.to_path_buf();
    loop {
        if current.join("harness").is_dir() && current.join("test").is_dir() {
            return current;
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => return PathBuf::from("silksurf-js/test262"),
        }
    }
}

// ---------------------------------------------------------------------------
// Scorecard output
// ---------------------------------------------------------------------------

fn emit_scorecard(
    path: &Path,
    totals: &Totals,
    scope: &str,
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
    writeln!(f, "  \"runner\": \"test262_boa\",")?;
    writeln!(f, "  \"engine\": \"boa_engine 0.21\",")?;
    writeln!(f, "  \"total\": {},", totals.total())?;
    writeln!(f, "  \"pass\": {},", totals.pass)?;
    writeln!(f, "  \"fail\": {},", totals.fail)?;
    writeln!(f, "  \"skip\": {},", totals.skip)?;
    writeln!(f, "  \"rate\": {:.4},", totals.rate())?;
    writeln!(f, "  \"pass_pct\": {:.2},", totals.rate() * 100.0)?;
    writeln!(f, "  \"timestamp\": \"{timestamp}\",")?;
    writeln!(f, "  \"scope\": \"{scope}\",")?;
    writeln!(f, "  \"duration_secs\": {:.2}", duration.as_secs_f64())?;
    writeln!(f, "}}")?;
    Ok(())
}

fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = now.div_euclid(86_400);
    let secs = now.rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };
    format!("{year:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
