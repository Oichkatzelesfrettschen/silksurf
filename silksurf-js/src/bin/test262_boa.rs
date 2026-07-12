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
 *   - Per-test bound is a deterministic loop-iteration budget (boa
 *     `set_loop_iteration_limit`, default 1e8, `--loop-limit`), not a
 *     wall-clock timeout. It converts an infinite JS loop into a catchable
 *     error so a runaway test cannot hang the whole run (the collector waits on
 *     every worker's result); recursion is bounded by boa's default 512-frame
 *     limit. A hang inside a single native call (no JS loop opcode) is not
 *     caught -- wall-clock was rejected as nondeterministic across machines.
 *     Budget hits are tallied separately (LIMIT), never folded into FAIL, so a
 *     too-low limit is visible instead of silently depressing the pass rate.
 *   - Static ESM (import/export) tests RUN: the harness is evaluated as a
 *     script (installing globals) and the test as a module via a
 *     `SimpleModuleLoader` rooted at the test's directory (so `_FIXTURE.js`
 *     imports resolve). Tests needing dynamic import, import.meta,
 *     top-level-await, or JSON modules stay skipped by feature flag.
 *   - async tests run: $DONE records completion (falsy/absent argument passes,
 *     truthy fails, double-call fails) and run_jobs drains the microtask queue
 *     the test enqueues. test262 async is microtask-based, so no host timer
 *     loop is needed here.
 *   - Strict-mode variants (onlyStrict flag) run as normal scripts.
 */

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use boa_engine::{
    Context, JsError, JsNativeError, JsValue, Module, NativeFunction, Source,
    builtins::promise::PromiseState,
    js_string,
    module::SimpleModuleLoader,
    object::{ObjectInitializer, builtins::JsArrayBuffer},
    property::Attribute,
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
    /// Per-test loop-iteration budget: an infinite JS loop hits this and throws
    /// a catchable error rather than hanging the run. Not a wall-clock timeout.
    loop_limit: u64,
}

/// Default loop-iteration budget. Far above any legitimate test262 loop (which
/// rarely exceeds ~1e5), yet finite so a runaway loop terminates in bounded
/// time. Sized for termination, not to tune the pass count.
const DEFAULT_LOOP_LIMIT: u64 = 100_000_000;

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut dir: Option<PathBuf> = None;
    let mut full = false;
    let mut verbose = false;
    let mut threads = 4usize;
    let mut scorecard = PathBuf::from("silksurf-js/conformance/test262-boa-scorecard.json");
    let mut loop_limit = DEFAULT_LOOP_LIMIT;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "--full" => full = true,
            "--loop-limit" => {
                i += 1;
                if i < args.len() {
                    loop_limit = args[i].parse().unwrap_or(DEFAULT_LOOP_LIMIT);
                }
            }
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

    Config {
        dir,
        full,
        verbose,
        threads,
        scorecard,
        loop_limit,
    }
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
    println!("      --loop-limit <n>  Per-test loop-iteration budget (default: 1e8)");
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
    let yaml_raw = &content[start + 5..start + rel_end];
    // str::lines() splits on \n and \r\n but not bare \r (old Mac style).
    // Normalize bare CR so that test files using CR-only line terminators
    // (e.g. line-terminator-normalisation-CR.js) have their metadata parsed.
    let yaml_buf;
    let yaml: &str = if yaml_raw.contains('\r') {
        yaml_buf = yaml_raw.replace("\r\n", "\n").replace('\r', "\n");
        &yaml_buf
    } else {
        yaml_raw
    };

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
            in_neg = false;
            in_features = false;
            in_includes = false;
            in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.flags = parse_inline_list(val);
            } else if val.is_empty() {
                in_flags = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("features:") {
            in_neg = false;
            in_features = false;
            in_includes = false;
            in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.features = parse_inline_list(val);
            } else if val.is_empty() {
                in_features = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("includes:") {
            in_neg = false;
            in_features = false;
            in_includes = false;
            in_flags = false;
            let val = rest.trim();
            if val.starts_with('[') {
                meta.includes = parse_inline_list(val);
            } else if val.is_empty() {
                in_includes = true;
            }
        } else if trimmed == "negative:" {
            in_neg = true;
            in_features = false;
            in_includes = false;
            in_flags = false;
            meta.negative = Some(NegSpec {
                phase: Phase::Parse,
                ntype: String::new(),
            });
        } else if in_neg {
            if let Some(rest) = trimmed.strip_prefix("phase:") {
                let phase_str = rest.trim();
                if let Some(neg) = &mut meta.negative {
                    neg.phase = if phase_str == "parse" {
                        Phase::Parse
                    } else {
                        Phase::Runtime
                    };
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
            if !f.is_empty() {
                meta.features.push(f);
            }
        } else if in_includes && trimmed.starts_with('-') {
            let inc = trimmed[1..].trim().to_string();
            if !inc.is_empty() {
                meta.includes.push(inc);
            }
        } else if in_flags && trimmed.starts_with('-') {
            let flag = trimmed[1..].trim().to_string();
            if !flag.is_empty() {
                meta.flags.push(flag);
            }
        } else if !trimmed.starts_with(' ') && !trimmed.starts_with('-') {
            // New top-level key resets all list contexts
            in_features = false;
            in_includes = false;
            in_flags = false;
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

// ---------------------------------------------------------------------------
// Path-substring skip list
// ---------------------------------------------------------------------------
//
// Tests are skipped when their path contains any of these substrings.
// Use this for suites where the failure is due to test-data staleness rather
// than a missing feature: the engine is correct, but the test suite was
// generated against a newer data version than the engine bundles.

const SKIP_PATH_CONTAINS: &[&str] = &[
    // Generated by mathiasbynens/unicode-property-escapes-tests against
    // Unicode 17.0.0.  boa 0.21 uses ICU4X tables built on Unicode 15.1.0;
    // new codepoints added in Unicode 16-17 cause member-set mismatches.
    // The regexp-unicode-property-escapes feature IS implemented correctly;
    // only the Unicode data version lags.  Skip until boa updates its tables.
    "RegExp/property-escapes/generated/",
];

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
    // Dynamic/async module features beyond static ESM: dynamic import()
    // resolution, import.meta host wiring, top-level-await settling, and JSON
    // modules each need more than the SimpleModuleLoader static-import path.
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
    // Tail call optimization -- boa 0.21 does not implement TCO
    "tail-call-optimization",
    // Iterator proposals not yet in boa 0.21
    "joint-iteration",
    "iterator-sequencing",
    // Uint8Array base64/hex proposal (2024) -- not in boa 0.21
    "uint8array-base64",
    // RegExp.escape proposal -- not in boa 0.21
    "RegExp.escape",
    // Error.isError proposal -- not in boa 0.21
    "Error.isError",
    // JSON.parse with source info -- not in boa 0.21
    "json-parse-with-source",
    // Align detached-buffer semantics -- spec change boa 0.21 predates
    "align-detached-buffer-semantics-with-web-reality",
    // IsHTMLDDA requires [[IsHTMLDDA]] internal slot (document.all-like object);
    // boa 0.21 does not expose the hook needed to create one via the public API.
    "IsHTMLDDA",
    // Legacy RegExp static properties ($1-$9, RegExp.input, etc.) are Annex B
    // accessors that boa 0.21 does not implement (null dereference in cross-realm).
    "legacy-regexp",
    // Function.prototype.caller / callee: boa 0.21 incorrectly throws TypeError
    // when caller is strict-mode code but callee is non-strict (should not throw
    // in the specific Annex B cases that the es5-era tests exercise).
    "caller",
    // ArrayBuffer.prototype.transferToImmutable -- 2025 spec addition not in boa 0.21.
    "immutable-arraybuffer",
    // Inline RegExp modifiers (?ims:...) -- Stage 3 proposal, not in boa 0.21.
    "regexp-modifiers",
    // FinalizationRegistry -- ES2021 feature not implemented in boa 0.21.
    // All FinalizationRegistry tests fail with "FinalizationRegistry is not defined".
    "FinalizationRegistry",
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
                if path.extension().is_some_and(|e| e == "js")
                    && let (Some(name), Ok(content)) = (
                        path.file_name().and_then(|n| n.to_str()),
                        std::fs::read_to_string(&path),
                    )
                {
                    files.insert(name.to_string(), content);
                }
            }
        }
        HarnessCache { files }
    }

    fn get(&self, name: &str) -> &str {
        self.files.get(name).map_or("", std::string::String::as_str)
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

/// Async-test completion recorded by `$DONE`. `$DONE()` (or a falsy argument)
/// passes; a truthy argument fails; a second call is a failure.
#[derive(Default)]
struct DoneCell {
    called: bool,
    result: Option<Result<(), String>>,
}

fn done_message(value: &JsValue, ctx: &mut Context) -> String {
    if let Some(object) = value.as_object() {
        let name = object
            .get(js_string!("name"), ctx)
            .ok()
            .filter(|v| !v.is_undefined())
            .and_then(|v| v.to_string(ctx).ok())
            .map(|s| s.to_std_string_lossy());
        let message = object
            .get(js_string!("message"), ctx)
            .ok()
            .filter(|v| !v.is_undefined())
            .and_then(|v| v.to_string(ctx).ok())
            .map(|s| s.to_std_string_lossy());
        if let (Some(name), Some(message)) = (&name, &message) {
            return format!("{name}: {message}");
        }
    }
    value.to_string(ctx).map_or_else(
        |_| "async test signaled failure".to_string(),
        |s| s.to_std_string_lossy(),
    )
}

fn install_test262_host(ctx: &mut Context, done: &Rc<RefCell<DoneCell>>) {
    // print() -- used by some harness files for debugging
    ctx.register_global_callable(
        js_string!("print"),
        1,
        NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
    )
    .expect("print install cannot fail");

    // $DONE(error) -- records async-test completion. test262 async tests are
    // microtask-based (no host timers), so run_jobs after eval settles them.
    let cell = Rc::clone(done);
    // SAFETY: the closure captures an Rc<RefCell<DoneCell>>, which holds no Boa
    // GC pointers, so the native function needs no trace hook.
    let done_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let outcome = match args.first() {
                Some(value) if value.to_boolean() => Err(done_message(value, ctx)),
                _ => Ok(()),
            };
            let mut cell = cell.borrow_mut();
            if cell.called {
                cell.result = Some(Err("$DONE called more than once".to_string()));
            } else {
                cell.called = true;
                cell.result = Some(outcome);
            }
            Ok(JsValue::undefined())
        })
    };
    ctx.register_global_callable(js_string!("$DONE"), 1, done_fn)
        .expect("$DONE install cannot fail");

    // $262 object -- test262 host environment interface
    let is_htmldda = ObjectInitializer::new(ctx).build();
    let dollar_262 = ObjectInitializer::new(ctx)
        .function(
            // createRealm(): create a fresh realm using boa's built-in API and
            // return { global } so cross-realm intrinsic tests can access the
            // new realm's constructors. The .eval() shim below evaluates in the
            // outer realm (not the new one); tests that call realm.eval() will
            // still fail, but the ~90 tests that only need .global will pass.
            NativeFunction::from_fn_ptr(|_, _, ctx| {
                let new_realm = ctx.create_realm()?;
                // Temporarily enter the new realm to read its global object.
                let old_realm = ctx.enter_realm(new_realm);
                let global_obj = ctx.global_object();
                ctx.enter_realm(old_realm);

                let realm_obj = ObjectInitializer::new(ctx)
                    .property(
                        js_string!("global"),
                        JsValue::from(global_obj),
                        Attribute::all(),
                    )
                    .function(
                        NativeFunction::from_fn_ptr(|_, args, ctx| {
                            let code = args
                                .first()
                                .and_then(|v| v.to_string(ctx).ok())
                                .map(|s| s.to_std_string_escaped())
                                .unwrap_or_default();
                            ctx.eval(Source::from_bytes(code.as_bytes()))
                        }),
                        js_string!("evalScript"),
                        1,
                    )
                    .build();
                Ok(JsValue::from(realm_obj))
            }),
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
            // detachArrayBuffer(ab): detach the ArrayBuffer using boa's built-in
            // API. Required by detachArrayBuffer.js harness and the ~130 test262
            // cases that call $DETACHBUFFER().
            NativeFunction::from_fn_ptr(|_, args, _ctx| {
                let val = args.first().ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("detachArrayBuffer requires one argument")
                })?;
                let obj = val.as_object().ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("detachArrayBuffer argument must be an object")
                })?;
                let ab = JsArrayBuffer::from_object(obj.clone()).map_err(|_| {
                    JsNativeError::typ()
                        .with_message("argument is not an ArrayBuffer")
                })?;
                ab.detach(&JsValue::undefined())?;
                Ok(JsValue::undefined())
            }),
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
        .function(
            // evalScript(code): evaluate a script string in the current realm.
            // Required by Annex B global-code tests that use $262.evalScript().
            NativeFunction::from_fn_ptr(|_, args, ctx| {
                let code = args
                    .first()
                    .and_then(|v| v.to_string(ctx).ok())
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_default();
                ctx.eval(Source::from_bytes(code.as_bytes()))
            }),
            js_string!("evalScript"),
            1,
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
    /// The test hit the deterministic loop-iteration budget (a probable
    /// infinite loop). Tallied separately from Fail so a spuriously-low limit
    /// is visible rather than silently depressing the pass rate.
    LimitExceeded,
}

fn run_test(
    meta: &TestMeta,
    harness: &HarnessCache,
    source: &str,
    path: &Path,
    loop_limit: u64,
) -> Outcome {
    // Skip: any unsupported feature listed in frontmatter.
    let skip_set: HashSet<&str> = SKIP_FEATURES.iter().copied().collect();
    for feat in &meta.features {
        if skip_set.contains(feat.as_str()) {
            return Outcome::Skip;
        }
    }

    let raw = meta.flags.iter().any(|f| f == "raw");
    let async_test = meta.flags.iter().any(|f| f == "async");
    let module_test = meta.flags.iter().any(|f| f == "module");

    let done = Rc::new(RefCell::new(DoneCell::default()));

    // Evaluate inside catch_unwind so no single test can crash the worker (and
    // thereby lose its result and stall the collector). One case is
    // load-bearing: a module whose loop hits the budget throws boa's
    // RuntimeLimit error, and boa PANICS converting that to a promise-rejection
    // value (it "cannot be converted to an opaque type"). Modules evaluate
    // through a loader rooted at the test's directory (so `./x_FIXTURE.js`
    // imports resolve) and see globals the harness installs as a script;
    // scripts concatenate the harness preamble and run directly.
    let eval = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if module_test {
            run_module_test(meta, harness, source, path, loop_limit, &done, raw)
        } else {
            run_script_test(meta, harness, source, loop_limit, &done, raw)
        }
    }));
    let eval_result = match eval {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            // The known RuntimeLimit panic is a budget hit; any other panic is
            // an engine failure on this test -- both stay visible, neither
            // crashes the run.
            if message.contains("RuntimeLimit") {
                return Outcome::LimitExceeded;
            }
            return Outcome::Fail(format!("engine panicked: {message}"));
        }
    };

    // A hit on the loop-iteration budget is a probable infinite loop, not a
    // conformance failure -- surface it distinctly.
    if let Err(err) = &eval_result
        && is_runtime_limit(err)
    {
        return Outcome::LimitExceeded;
    }

    decide_outcome(eval_result, meta, async_test, &done)
}

/// Extract a human-readable message from a caught panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Evaluate a non-module test: harness preamble + source in one script.
fn run_script_test(
    meta: &TestMeta,
    harness: &HarnessCache,
    source: &str,
    loop_limit: u64,
    done: &Rc<RefCell<DoneCell>>,
    raw: bool,
) -> Result<(), JsError> {
    let preamble = if raw {
        harness.preamble_raw(&meta.includes)
    } else {
        harness.preamble(&meta.includes)
    };
    let mut script = preamble;
    // onlyStrict: prepend "use strict" so the test runs in strict mode.
    if meta.flags.iter().any(|f| f == "onlyStrict") {
        script.insert_str(0, "\"use strict\";\n");
    }
    script.push_str(source);

    let mut ctx = Context::default();
    ctx.runtime_limits_mut()
        .set_loop_iteration_limit(loop_limit);
    install_test262_host(&mut ctx, done);

    let eval_result = ctx.eval(Source::from_bytes(script.as_bytes())).map(|_| ());
    let _ = ctx.run_jobs();
    eval_result
}

/// Evaluate a module test: the harness runs as a script (installing globals),
/// then the test source is parsed and evaluated as an ES module through a
/// `SimpleModuleLoader` rooted at the test file's directory. A module reports a
/// runtime throw by rejecting its evaluation promise, which is mapped back to an
/// `Err` so the shared negative/positive logic treats it like a script throw.
fn run_module_test(
    meta: &TestMeta,
    harness: &HarnessCache,
    source: &str,
    path: &Path,
    loop_limit: u64,
    done: &Rc<RefCell<DoneCell>>,
    raw: bool,
) -> Result<(), JsError> {
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let loader = Rc::new(SimpleModuleLoader::new(root)?);
    let mut ctx = Context::builder()
        .module_loader(loader)
        .build()
        .expect("context builder with module loader cannot fail");
    ctx.runtime_limits_mut()
        .set_loop_iteration_limit(loop_limit);
    install_test262_host(&mut ctx, done);

    // Harness as a script: its globals (assert, $DONE, ...) become visible to
    // the module, matching how a browser runs the harness before the module.
    if !raw {
        let preamble = harness.preamble(&meta.includes);
        ctx.eval(Source::from_bytes(preamble.as_bytes()))?;
    }

    // Parse-phase errors surface here directly (Module::parse returns Result),
    // mapping cleanly to a negative test's Parse phase. The entry module is
    // parsed with its own path so a relative `./x_FIXTURE.js` import has a
    // referrer to resolve against (via the loader's root).
    let entry = Source::from_bytes(source.as_bytes()).with_path(path);
    let module = Module::parse(entry, None, &mut ctx)?;

    let promise = module.load_link_evaluate(&mut ctx);
    let _ = ctx.run_jobs();

    match promise.state() {
        PromiseState::Fulfilled(_) => Ok(()),
        // A rejected evaluation promise carries the thrown value; wrap it as a
        // JsError so error_matches/error_type_name work as for a script throw.
        PromiseState::Rejected(value) => Err(JsError::from_opaque(value)),
        PromiseState::Pending => Err(JsError::from_opaque(
            js_string!("module evaluation did not settle").into(),
        )),
    }
}

/// Decide the outcome from an evaluation result, shared by the script and
/// module paths. Async tests report through `$DONE`; everything else is judged
/// against the frontmatter's negative expectation.
fn decide_outcome(
    eval_result: Result<(), JsError>,
    meta: &TestMeta,
    async_test: bool,
    done: &Rc<RefCell<DoneCell>>,
) -> Outcome {
    // Async tests report their outcome through $DONE. The test262 harness has
    // no host timers, so draining the microtask queue settles every promise
    // reaction the test enqueues.
    if async_test {
        if let Err(err) = eval_result {
            return Outcome::Fail(format!("async setup threw: {err}"));
        }
        return match &done.borrow().result {
            Some(Ok(())) => Outcome::Pass,
            Some(Err(message)) => Outcome::Fail(message.clone()),
            None => Outcome::Fail("async test did not call $DONE".to_string()),
        };
    }

    match (eval_result, &meta.negative) {
        // Pass: no error, no negative expectation
        (Ok(()), None) => Outcome::Pass,

        // Fail: script passed but we expected an error
        (Ok(()), Some(neg)) => Outcome::Fail(format!(
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

/// Whether a JsError is a boa runtime-limit error (loop-iteration budget hit).
fn is_runtime_limit(e: &JsError) -> bool {
    if let Some(native) = e.as_native() {
        matches!(native.kind, boa_engine::JsNativeErrorKind::RuntimeLimit)
    } else {
        format!("{e}").starts_with("RuntimeLimit")
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
    if error_type_name(e) == expected {
        return true;
    }
    // For non-native thrown values (e.g. Test262Error, user-defined classes),
    // boa formats the JsError as "<ClassName> { ... }". Check if the Display
    // string starts with the expected type name so that `type: Test262Error`
    // negative expectations match correctly.
    format!("{e}").starts_with(expected)
}

// ---------------------------------------------------------------------------
// File collection
// ---------------------------------------------------------------------------

fn collect_js_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
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
            // Files with unicode-17.0.0 in the stem test Unicode 17.0.0 codepoints
            // that boa's tables predate; skip by filename rather than feature flag.
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !stem.ends_with("_FIXTURE") && !stem.contains("unicode-17.0.0") {
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
    /// Tests that hit the loop-iteration budget (probable infinite loops).
    /// Counted as executed-but-not-passed and reported distinctly so a
    /// too-low limit is visible rather than silently depressing the pass rate.
    limit: usize,
}

impl Totals {
    fn total(&self) -> usize {
        self.pass + self.fail + self.skip + self.limit
    }
    fn executed(&self) -> usize {
        self.pass + self.fail + self.limit
    }
    /*
     * Two denominators, both always reported.  rate_executed divides by the
     * tests actually run; rate_total divides by the full suite including
     * skips (Intl, modules, async, FinalizationRegistry).  Quoting only the
     * executed rate overstates conformance -- a 99.8% executed rate over a
     * suite with 30% skips is a ~69% total rate.
     */
    fn rate_executed(&self) -> f64 {
        if self.executed() == 0 {
            0.0
        } else {
            self.pass as f64 / self.executed() as f64
        }
    }
    fn rate_total(&self) -> f64 {
        if self.total() == 0 {
            0.0
        } else {
            self.pass as f64 / self.total() as f64
        }
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

    // Suppress the default panic hook: per-test evaluation runs inside
    // catch_unwind and reports a panic as an outcome (LimitExceeded or Fail), so
    // the default stderr backtrace would only be noise on tests that are already
    // accounted for.
    std::panic::set_hook(Box::new(|_| {}));

    // Spawn worker threads
    for _ in 0..cfg.threads {
        let work_rx = Arc::clone(&work_rx);
        let result_tx = result_tx.clone();
        let harness = Arc::clone(&harness);
        let loop_limit = cfg.loop_limit;
        std::thread::spawn(move || {
            loop {
                let item = {
                    let rx = work_rx.lock().unwrap();
                    rx.recv()
                };
                match item {
                    Ok(WorkItem { path, source }) => {
                        // Path-substring skip: data-staleness issues that are
                        // independent of whether the engine feature is present.
                        let path_str = path.to_str().unwrap_or("");
                        if SKIP_PATH_CONTAINS.iter().any(|pat| path_str.contains(pat)) {
                            let _ = result_tx.send((path, Outcome::Skip));
                            continue;
                        }
                        let meta = parse_meta(&source);
                        let outcome = run_test(&meta, &harness, &source, &path, loop_limit);
                        let _ = result_tx.send((path, outcome));
                    }
                    Err(_) => break,
                }
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
            Outcome::LimitExceeded => {
                totals.limit += 1;
                if cfg.verbose {
                    println!("LIMIT {}  -- loop-iteration budget hit", path.display());
                }
            }
            Outcome::Fail(reason) => {
                totals.fail += 1;
                fail_list.push((path.clone(), reason.clone()));
                if cfg.verbose {
                    println!("FAIL  {}  -- {}", path.display(), reason);
                }
            }
        }
        if done.is_multiple_of(report_every) || done == total_files {
            let pct = done as f64 / total_files as f64 * 100.0;
            eprint!(
                "\r  {done}/{total_files} ({pct:.0}%)  pass={} fail={} skip={} limit={}   ",
                totals.pass, totals.fail, totals.skip, totals.limit
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
            println!(
                "  ... and {} more (use -v for full list)",
                fail_list.len() - show
            );
        }
    }

    println!();
    println!("------------------------------------------------------------");
    println!(
        "PASS: {}  FAIL: {}  SKIP: {}  LIMIT: {}  TOTAL: {}",
        totals.pass,
        totals.fail,
        totals.skip,
        totals.limit,
        totals.total()
    );
    println!(
        "Rate (executed): {:.2}%  Rate (total incl. skips): {:.2}%  ({:.1}s)",
        totals.rate_executed() * 100.0,
        totals.rate_total() * 100.0,
        duration.as_secs_f64()
    );

    let scope_label = if cfg.full {
        "language+built-ins+annexB"
    } else {
        "language"
    };
    if let Err(e) = emit_scorecard(&cfg.scorecard, &totals, scope_label, duration) {
        eprintln!("WARN: scorecard write failed: {e}");
    } else {
        println!("Scorecard: {}", cfg.scorecard.display());
    }

    // Exit 0 iff executed pass rate > 50% (lower gate than the real suite's
    // 80%+ expectation since module/async tests are skipped; rate_total in
    // the scorecard carries the honest all-tests denominator).
    if totals.rate_executed() >= 0.5 {
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
    writeln!(f, "  \"executed\": {},", totals.executed())?;
    writeln!(f, "  \"pass\": {},", totals.pass)?;
    writeln!(f, "  \"fail\": {},", totals.fail)?;
    writeln!(f, "  \"skip\": {},", totals.skip)?;
    writeln!(f, "  \"limit_exceeded\": {},", totals.limit)?;
    writeln!(f, "  \"rate_executed\": {:.4},", totals.rate_executed())?;
    writeln!(
        f,
        "  \"pass_pct_executed\": {:.2},",
        totals.rate_executed() * 100.0
    )?;
    writeln!(f, "  \"rate_total\": {:.4},", totals.rate_total())?;
    writeln!(
        f,
        "  \"pass_pct_total\": {:.2},",
        totals.rate_total() * 100.0
    )?;
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
