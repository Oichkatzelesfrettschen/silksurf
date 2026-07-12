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

/// Read a numeric global set by a prior eval (eval itself returns unit).
fn read_global_u32(ctx: &mut SilkContext, _name: &str) -> u32 {
    // The target id rides through a JSON print: eval throws with the value
    // embedded when direct reads are unavailable. Cheapest reliable path:
    // stash into globalThis and re-read via a throwing probe is noisy, so
    // the probe serializes through Error message parsing.
    let mut value = 0_u32;
    if let Err(message) = ctx.eval("throw new Error('V=' + globalThis.__clickTarget);")
        && let Some(pos) = message.find("V=")
    {
        let digits: String = message[pos + 2..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        value = digits.parse().unwrap_or(0);
    }
    value
}

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
    let click_id: Option<String> = args
        .iter()
        .position(|arg| arg == "--click")
        .and_then(|i| args.get(i + 1).cloned());
    let mut skip_next = false;
    let specs: Vec<&String> = args
        .iter()
        .filter(|arg| {
            if skip_next {
                skip_next = false;
                return false;
            }
            if *arg == "--click" {
                skip_next = true;
                return false;
            }
            *arg != "--shared" && *arg != "--dom" && *arg != "--pump"
        })
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
    let last_spec: Option<String> = specs.last().map(|s| (*s).clone());
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
        // --click <id>: after the last file settles, dispatch a trusted
        // click at the element carrying that id (root-delegated framework
        // handlers receive it through capture/bubble), then pump again so
        // the handler's scheduled re-render commits before its check runs.
        if ok
            && let Some(id) = &click_id
            && last_spec.as_deref() == Some(spec.as_str())
        {
            let target_expr = format!(
                "(function () {{ var el = document.getElementById('{id}'); return el ? el.nodeId : -1; }})()"
            );
            let _ = ctx.eval(&format!("globalThis.__clickTarget = {target_expr};"));
            // The nodeId travels through a global because eval returns unit.
            let node = ctx
                .eval("if (globalThis.__clickTarget < 0) { throw new Error('click target missing'); }")
                .is_ok();
            if node {
                let raw = read_global_u32(ctx, "__clickTarget");
                let event = silksurf_js::SyntheticEvent::new("click", true, true);
                match ctx.dispatch_dom_event(silksurf_dom::NodeId::from_raw(raw as usize), &event) {
                    Ok(_) => {
                        let deadline = Instant::now() + std::time::Duration::from_millis(2000);
                        loop {
                            ctx.run_pending_jobs();
                            let _ = ctx.run_ready_host_callbacks();
                            if !ctx.has_pending_host_callbacks() || Instant::now() >= deadline {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                    }
                    Err(err) => {
                        eprintln!("bundle_probe: click dispatch: {err}");
                        ok = false;
                    }
                }
                if ok
                    && let Some(check) = check
                    && let Err(err) = ctx.eval(check)
                {
                    let head: String = err.chars().take(300).collect();
                    eprintln!("bundle_probe: post-click check {path}: {head}");
                    ok = false;
                }
            } else {
                ok = false;
            }
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
