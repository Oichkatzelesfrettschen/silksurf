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

/// Filter probe flags out of argv, leaving the `file[=check]` specs. `--click`
/// consumes the following argument as its target id.
fn collect_specs(args: &[String]) -> Vec<&String> {
    let mut skip_next = false;
    args.iter()
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
        .collect()
}

/// Drain microtask and host-callback queues until idle or a two-second deadline.
/// Framework schedulers defer renders through setTimeout and microtasks, so a
/// probe must pump before reading the committed DOM.
fn pump_host_queues(ctx: &mut SilkContext) {
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

/// Dispatch a trusted click at the element carrying `id` (root-delegated
/// framework handlers receive it through capture/bubble), pump the scheduled
/// re-render, then run the post-click `check`. Returns whether the check held.
fn dispatch_click(ctx: &mut SilkContext, id: &str, check: Option<&str>) -> bool {
    let target_expr = format!(
        "(function () {{ var el = document.getElementById('{id}'); return el ? el.nodeId : -1; }})()"
    );
    let _ = ctx.eval(&format!("globalThis.__clickTarget = {target_expr};"));
    // The nodeId travels through a global because eval returns unit.
    if ctx
        .eval("if (globalThis.__clickTarget < 0) { throw new Error('click target missing'); }")
        .is_err()
    {
        return false;
    }
    let raw = read_global_u32(ctx, "__clickTarget");
    let event = silksurf_js::SyntheticEvent::new("click", true, true);
    if let Err(err) = ctx.dispatch_dom_event(silksurf_dom::NodeId::from_raw(raw as usize), &event) {
        eprintln!("bundle_probe: click dispatch: {err}");
        return false;
    }
    pump_host_queues(ctx);
    if let Some(check) = check
        && let Err(err) = ctx.eval(check)
    {
        let head: String = err.chars().take(300).collect();
        eprintln!("bundle_probe: post-click check: {head}");
        return false;
    }
    true
}

/// Evaluate one bundle spec in `ctx`: time the eval, optionally pump deferred
/// work, run the check, and dispatch the click when `click` is set. The check
/// on the click spec describes the post-click DOM, so it is deferred past the
/// dispatch instead of run first (which would fail and gate out the click).
/// Returns `(bytes, eval_ms, ok)`, or `None` when the source cannot be read.
fn run_spec(
    ctx: &mut SilkContext,
    path: &str,
    check: Option<&str>,
    pump: bool,
    click: Option<&str>,
) -> Option<(usize, f64, bool)> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("bundle_probe: read {path}: {err}");
            return None;
        }
    };
    let start = Instant::now();
    let eval_result = ctx.eval(&source);
    if pump && eval_result.is_ok() {
        pump_host_queues(ctx);
    }
    let eval_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut ok = eval_result.is_ok();
    if let Err(err) = &eval_result {
        let head: String = err.chars().take(200).collect();
        eprintln!("bundle_probe: eval {path}: {head}");
    }
    if ok
        && click.is_none()
        && let Some(check) = check
    {
        let checked = ctx.eval(check);
        if let Err(err) = &checked {
            let head: String = err.chars().take(200).collect();
            eprintln!("bundle_probe: check {path}: {head}");
        }
        ok = checked.is_ok();
    }
    if ok && let Some(id) = click {
        ok = dispatch_click(ctx, id, check);
    }
    Some((source.len(), eval_ms, ok))
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
    let specs = collect_specs(&args);
    if specs.is_empty() {
        eprintln!(
            "usage: bundle_probe [--shared] [--dom] [--pump] [--click id] <file[=check_expr]>..."
        );
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
        let mut fresh_ctx;
        let ctx = match shared_ctx.as_mut() {
            Some(ctx) => ctx,
            None => {
                fresh_ctx = make_ctx();
                &mut fresh_ctx
            }
        };
        // Only the last spec receives the click; earlier bundles just load.
        let click = click_id
            .as_deref()
            .filter(|_| last_spec.as_deref() == Some(spec.as_str()));
        match run_spec(ctx, path, check, pump, click) {
            Some((bytes, eval_ms, ok)) => {
                if !ok {
                    failures += 1;
                }
                println!(
                    "{{\"file\":\"{path}\",\"bytes\":{bytes},\"eval_ms\":{eval_ms:.2},\"ok\":{ok}}}"
                );
            }
            None => failures += 1,
        }
    }
    std::process::exit(i32::from(failures > 0));
}
