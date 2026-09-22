use silksurf_js::{ModuleScript, SilkContext};

#[test]
fn document_roots_share_dependencies_and_keep_inline_document_url() {
    let mut context = SilkContext::new();
    let base = "https://example.test/app/page";
    let shared = "https://example.test/app/shared.js";
    let external = "https://example.test/app/external.js";
    let roots = vec![
        ModuleScript::Inline("import './shared.js'; globalThis.order += 'a'; globalThis.inlineUrl = import.meta.url;".into()),
        ModuleScript::External(external.into()),
        ModuleScript::Inline("import './shared.js'; globalThis.order += 'c';".into()),
    ];
    let modules = vec![
        (shared.into(), "globalThis.count = (globalThis.count || 0) + 1; globalThis.order = '';".into()),
        (external.into(), "import './shared.js'; globalThis.order += 'b'; globalThis.externalUrl = import.meta.url;".into()),
    ];
    let results = context
        .eval_document_modules(base, &roots, &modules)
        .expect("document graph");
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    context.eval("if (count !== 1 || order !== 'abc') throw new Error('module identity or ordering'); if (inlineUrl !== 'https://example.test/app/page' || externalUrl !== 'https://example.test/app/external.js') throw new Error('module URLs');").expect("shared dependency evaluates once");
}

#[test]
fn inline_only_module_runs_and_failed_root_allows_following_root() {
    let mut context = SilkContext::new();
    let roots = vec![
        ModuleScript::Inline("throw new Error('root failure');".into()),
        ModuleScript::Inline("globalThis.ready = true;".into()),
    ];
    let results = context
        .eval_document_modules("https://example.test/", &roots, &[])
        .expect("inline graph");
    assert!(results[0].is_err());
    assert!(results[1].is_ok());
    context
        .eval("if (!ready) throw new Error('inline skipped');")
        .expect("later inline runs");
}

#[test]
fn external_parse_failure_affects_importers_and_preserves_independent_roots() {
    let mut context = SilkContext::new();
    let roots = vec![
        ModuleScript::External("https://example.test/broken.js".into()),
        ModuleScript::Inline("import './broken.js';".into()),
        ModuleScript::Inline("globalThis.independent = true;".into()),
    ];
    let modules = vec![(
        "https://example.test/broken.js".into(),
        "export const = ;".into(),
    )];
    let results = context
        .eval_document_modules("https://example.test/", &roots, &modules)
        .expect("graph records parse failures");
    assert!(results[0].is_err());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    context
        .eval("if (!independent) throw new Error('independent root skipped');")
        .expect("independent root executes");
}
