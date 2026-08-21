//! The module loader answering a registry miss by fetching.
//!
//! A dynamic import whose specifier is computed at run time reaches the loader
//! with no scan over the source text having predicted it, so the module is
//! absent from the graph the embedder fetched ahead of evaluation. AD-032
//! gives the loader a fetcher for that case, under a budget the static walk
//! and the loader share.

use std::cell::RefCell;
use std::rc::Rc;

use silksurf_js::{ModuleFetchBudget, SilkContext};

/// Record of what a test fetcher was asked for.
type Log = Rc<RefCell<Vec<String>>>;

/// A table fetcher paired with the log of the URLs it answered.
type FetcherWithLog = (silksurf_js::ModuleFetcher, Log);

/// A fetcher answering from a fixed table, recording every URL it is asked for.
fn table_fetcher(table: Vec<(&'static str, &'static str)>) -> FetcherWithLog {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&log);
    let fetcher = move |url: &str| -> Result<String, String> {
        sink.borrow_mut().push(url.to_string());
        table
            .iter()
            .find_map(|(key, body)| (*key == url).then(|| (*body).to_string()))
            .ok_or_else(|| "404".to_string())
    };
    (Box::new(fetcher), log)
}

/// Evaluate `root_source` as the document's only module and report the value
/// the script left in `globalThis.seen`.
fn run_root(ctx: &mut SilkContext, root: &str, root_source: &str) -> String {
    ctx.eval_module_graph(root, &[(root.to_string(), root_source.to_string())])
        .expect("module graph evaluates");
    ctx.run_pending_jobs();
    match ctx.eval("throw new Error(String(globalThis.seen));") {
        Err(message) => message,
        Ok(()) => panic!("probe throw did not surface"),
    }
}

const IMPORT_PROBE: &str = "globalThis.seen='pending';\
     import('./lazy.js').then(function (m) { globalThis.seen = 'ok:' + m.v; },\
     function (e) { globalThis.seen = String(e); });";

fn budgeted(urls: usize, bytes: usize) -> ModuleFetchBudget {
    ModuleFetchBudget { urls, bytes }
}

#[test]
fn a_registry_miss_fetches_parses_and_evaluates() {
    let mut ctx = SilkContext::new();
    let (fetcher, log) = table_fetcher(vec![(
        "https://example.test/lazy.js",
        "export const v = 42;",
    )]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("ok:42"),
        "import resolves to the fetched module: {seen}"
    );
    assert_eq!(log.borrow().as_slice(), ["https://example.test/lazy.js"]);
}

#[test]
fn a_loader_without_a_fetcher_reports_the_miss() {
    let mut ctx = SilkContext::new();
    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("was not fetched"),
        "a context with no fetcher keeps reporting the miss: {seen}"
    );
}

#[test]
fn a_fetch_failure_rejects_the_import_with_its_reason() {
    let mut ctx = SilkContext::new();
    let (fetcher, _log) = table_fetcher(Vec::new());
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("404"),
        "the fetcher's reason reaches the page: {seen}"
    );
}

#[test]
fn a_body_that_is_not_a_module_rejects_the_import() {
    let mut ctx = SilkContext::new();
    let (fetcher, _log) = table_fetcher(vec![("https://example.test/lazy.js", "export const = ;")]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("lazy.js"),
        "a parse failure names the module: {seen}"
    );
    assert!(
        !seen.contains("ok:"),
        "a parse failure does not resolve: {seen}"
    );
}

#[test]
fn an_exhausted_url_allowance_rejects_rather_than_fetching() {
    let mut ctx = SilkContext::new();
    let (fetcher, log) = table_fetcher(vec![(
        "https://example.test/lazy.js",
        "export const v = 42;",
    )]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(0, 4096));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("allowance"),
        "the allowance names the stop: {seen}"
    );
    assert!(
        log.borrow().is_empty(),
        "an exhausted allowance reaches no network"
    );
}

#[test]
fn a_body_over_the_byte_allowance_rejects() {
    let mut ctx = SilkContext::new();
    let (fetcher, _log) = table_fetcher(vec![(
        "https://example.test/lazy.js",
        "export const v = 42;",
    )]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(
        seen.contains("byte allowance"),
        "a body over the allowance is rejected rather than admitted: {seen}"
    );
}

#[test]
fn a_fetch_charges_the_allowance_by_the_body_it_received() {
    let mut ctx = SilkContext::new();
    let body = "export const v = 42;";
    let (fetcher, _log) = table_fetcher(vec![("https://example.test/lazy.js", body)]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    let left = ctx.module_fetch_budget();
    assert_eq!(left.urls, 3);
    assert_eq!(left.bytes, 4096 - body.len());
}

/// One module record per URL, within one root's evaluation.
///
/// `eval_module_graph` clears the registry on entry, so a document with
/// several module roots re-fetches and re-evaluates what an earlier root
/// already paid for. AD-032 names that as `module-record-identity-across-roots`.
#[test]
fn a_second_import_of_one_url_answers_from_the_registry() {
    let mut ctx = SilkContext::new();
    let (fetcher, log) = table_fetcher(vec![(
        "https://example.test/lazy.js",
        "globalThis.evaluations = (globalThis.evaluations || 0) + 1;\
         export const v = 42;",
    )]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    ctx.eval_module_graph(
        "https://example.test/root.js",
        &[(
            "https://example.test/root.js".to_string(),
            "globalThis.seen='';\
             import('./lazy.js').then(function () { return import('./lazy.js'); })\
             .then(function (m) { globalThis.seen = 'ok:' + m.v; },\
             function (e) { globalThis.seen = String(e); });"
                .to_string(),
        )],
    )
    .expect("module graph evaluates");
    ctx.run_pending_jobs();

    let Err(seen) = ctx.eval("throw new Error(globalThis.seen + '/' + globalThis.evaluations);")
    else {
        panic!("probe throw did not surface");
    };
    assert!(
        seen.contains("ok:42/1"),
        "one module record, evaluated once: {seen}"
    );
    assert_eq!(
        log.borrow().len(),
        1,
        "the registry answers the second import without a fetch"
    );
    assert_eq!(ctx.module_fetch_budget().urls, 3, "one fetch, one charge");
}

#[test]
fn a_fetched_module_s_own_static_imports_fetch_too() {
    let mut ctx = SilkContext::new();
    let (fetcher, log) = table_fetcher(vec![
        (
            "https://example.test/lazy.js",
            "import { w } from './deep.js'; export const v = w + 1;",
        ),
        ("https://example.test/deep.js", "export const w = 41;"),
    ]);
    ctx.set_module_fetcher(fetcher);
    ctx.set_module_fetch_budget(budgeted(4, 4096));

    let seen = run_root(&mut ctx, "https://example.test/root.js", IMPORT_PROBE);
    assert!(seen.contains("ok:42"), "the transitive graph links: {seen}");
    assert_eq!(log.borrow().len(), 2, "both modules fetch");
    assert_eq!(
        ctx.module_fetch_budget().urls,
        2,
        "both charge the allowance"
    );
}
