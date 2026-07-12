/*
 * bundle_probe -- measure boa throughput on production JS bundles.
 *
 * Each argument file evaluates in a fresh SilkContext; the probe times the
 * combined parse+execute wall clock, then runs an optional correctness
 * expression supplied as `path=expr` (the expression must evaluate without
 * throwing). Bundles evaluate in argument order within one shared context
 * when --shared is passed, which is how a page loads react followed by
 * react-dom.
 *
 * Output is one JSON object per line: file, bytes, eval_ms, ok. The
 * SPA-capability roadmap's boa-bundle-throughput-spike consumes these
 * numbers to decide whether the chatgpt.com rung needs upstream boa work.
 */

use std::sync::{Arc, Mutex};
use std::time::Instant;

use silksurf_js::SilkContext;

/// A context with the live DOM bridge over a minimal html/head/body tree,
/// matching what a page script sees. The stub document in SilkContext::new
/// advertises DOM presence but cannot create elements, which sends
/// DOM-detecting bundles (react-dom) down the browser path into nulls.
fn dom_backed_context() -> SilkContext {
    let mut dom = silksurf_dom::Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let head = dom.create_element("head");
    let body = dom.create_element("body");
    let _ = dom.append_child(document, html);
    let _ = dom.append_child(html, head);
    let _ = dom.append_child(html, body);
    dom.materialize_resolve_table();
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let shared = args.iter().any(|arg| arg == "--shared");
    let with_dom = args.iter().any(|arg| arg == "--dom");
    let pump = args.iter().any(|arg| arg == "--pump");
    let specs: Vec<&String> = args
        .iter()
        .filter(|arg| *arg != "--shared" && *arg != "--dom" && *arg != "--pump")
        .collect();
    if specs.is_empty() {
        eprintln!("usage: bundle_probe [--shared] [--dom] [--pump] <file[=check_expr]>...");
        std::process::exit(2);
    }

    let make_ctx = move || {
        if with_dom {
            dom_backed_context()
        } else {
            SilkContext::new()
        }
    };
    let mut shared_ctx = shared.then(make_ctx);
    let mut failures = 0;
    for spec in specs {
        let (path, check) = match spec.split_once('=') {
            Some((path, check)) => (path, Some(check)),
            None => (spec.as_str(), None),
        };
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("bundle_probe: read {path}: {err}");
                failures += 1;
                continue;
            }
        };
        let mut fresh_ctx;
        let ctx = match shared_ctx.as_mut() {
            Some(ctx) => ctx,
            None => {
                fresh_ctx = make_ctx();
                &mut fresh_ctx
            }
        };
        let start = Instant::now();
        let eval_result = ctx.eval(&source);
        // Framework schedulers defer work through setTimeout and microtasks;
        // --pump drains the host queues for up to two seconds so deferred
        // renders commit before the correctness check runs.
        if pump && eval_result.is_ok() {
            let deadline = Instant::now() + std::time::Duration::from_millis(2000);
            loop {
                ctx.run_pending_jobs();
                if let Err(err) = ctx.run_ready_host_callbacks() {
                    let head: String = err.chars().take(300).collect();
                    eprintln!("bundle_probe: host callback: {head}");
                }
                if !ctx.has_pending_host_callbacks() || Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let eval_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut ok = eval_result.is_ok();
        if let Err(err) = &eval_result {
            let head: String = err.chars().take(200).collect();
            eprintln!("bundle_probe: eval {path}: {head}");
        }
        if ok && let Some(check) = check {
            let checked = ctx.eval(check);
            if let Err(err) = &checked {
                let head: String = err.chars().take(200).collect();
                eprintln!("bundle_probe: check {path}: {head}");
            }
            ok = checked.is_ok();
        }
        if !ok {
            failures += 1;
        }
        println!(
            "{{\"file\":\"{path}\",\"bytes\":{},\"eval_ms\":{eval_ms:.2},\"ok\":{ok}}}",
            source.len()
        );
    }
    std::process::exit(i32::from(failures > 0));
}
