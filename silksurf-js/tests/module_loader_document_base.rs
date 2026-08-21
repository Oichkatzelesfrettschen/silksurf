//! The document's address as the base a non-module referrer resolves against.
//!
//! HTML resolves a relative module specifier named by a classic script against
//! the document's base URL, and resolves an import map's scope prefixes
//! against it as well (HTML 8.1.3.8). The loader reads that address from
//! `SilkContext::set_document_url`, which embedders call before page script.

use silksurf_js::{ImportMap, SilkContext};

/// Report a global's string value through the error a throw carries.
fn read_global(ctx: &mut SilkContext, name: &str) -> String {
    match ctx.eval(&format!("throw new Error(String(globalThis.{name}));")) {
        Err(message) => message,
        Ok(()) => panic!("probe throw did not surface"),
    }
}

#[test]
fn import_map_scope_prefix_resolves_against_the_document_address() {
    let mut ctx = SilkContext::new();
    // Production order: the address lands before the import map, which is what
    // lets a scope prefix spelled as a path resolve.
    ctx.set_document_url("https://example.test/index.html");
    ctx.set_import_map(ImportMap {
        imports: vec![("lib".to_string(), "https://example.test/top.js".to_string())],
        scopes: vec![(
            "/app/".to_string(),
            vec![(
                "lib".to_string(),
                "https://example.test/scoped.js".to_string(),
            )],
        )],
    });

    let root = "https://example.test/app/root.js".to_string();
    let modules = vec![(
        root.clone(),
        "globalThis.seen = '';\
         import('lib').then(function () {}, function (e) { globalThis.seen = String(e); });"
            .to_string(),
    )];
    ctx.eval_module_graph(&root, &modules)
        .expect("module graph evaluates");
    ctx.run_pending_jobs();

    // The importer sits under /app/, so the scoped mapping answers `lib`. The
    // registry holds neither target, and the rejection names the URL the
    // specifier resolved to.
    let seen = read_global(&mut ctx, "seen");
    assert!(
        seen.contains("scoped.js"),
        "scoped mapping must win for an importer under its prefix: {seen}"
    );
}

#[test]
fn classic_script_resolves_a_relative_specifier_against_the_document() {
    let mut ctx = SilkContext::new();
    ctx.set_document_url("https://example.test/pages/index.html");
    ctx.eval(
        "globalThis.seen = '';\
         import('./m.js').then(function () {}, function (e) { globalThis.seen = String(e); });",
    )
    .expect("script evaluates");
    ctx.run_pending_jobs();

    // Without a base the specifier fails resolution and never reaches the
    // registry, which is a different error than a registry miss.
    let seen = read_global(&mut ctx, "seen");
    assert!(
        seen.contains("https://example.test/pages/m.js"),
        "relative specifier resolves against the document directory: {seen}"
    );
}

#[test]
fn a_module_graph_leaves_an_embedder_set_address_alone() {
    let mut ctx = SilkContext::new();
    ctx.set_document_url("https://example.test/index.html");
    let root = "https://example.test/deep/nested/root.js".to_string();
    ctx.eval_module_graph(&root, &[(root.clone(), "globalThis.ran = 1;".to_string())])
        .expect("module graph evaluates");

    ctx.eval(
        "globalThis.seen = '';\
         import('./after.js').then(function () {}, function (e) { globalThis.seen = String(e); });",
    )
    .expect("script evaluates");
    ctx.run_pending_jobs();

    // A classic script's base stays the document, rather than becoming
    // whichever module root evaluated last.
    let seen = read_global(&mut ctx, "seen");
    assert!(
        seen.contains("https://example.test/after.js"),
        "document address survives a module graph evaluation: {seen}"
    );
}

#[test]
fn a_graph_without_a_document_falls_back_to_its_root_module() {
    let mut ctx = SilkContext::new();
    let root = "https://example.test/lib/root.js".to_string();
    ctx.eval_module_graph(&root, &[(root.clone(), "globalThis.ran = 1;".to_string())])
        .expect("module graph evaluates");

    ctx.eval(
        "globalThis.seen = '';\
         import('./side.js').then(function () {}, function (e) { globalThis.seen = String(e); });",
    )
    .expect("script evaluates");
    ctx.run_pending_jobs();

    let seen = read_global(&mut ctx, "seen");
    assert!(
        seen.contains("https://example.test/lib/side.js"),
        "root module supplies the base when no document address is set: {seen}"
    );
}
