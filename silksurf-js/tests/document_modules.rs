use silksurf_js::{ModuleScript, SilkContext};

#[test]
fn classic_import_and_later_roots_retain_one_record_and_consumed_budget() {
    use silksurf_js::ModuleFetchBudget;
    let mut context = SilkContext::new();
    let base = "https://example.test/app/";
    let source =
        "globalThis.executions = (globalThis.executions || 0) + 1; export const value = 42;";
    context.set_document_url(base);
    context.set_module_fetcher(Box::new(move |url| {
        assert_eq!(url, "https://example.test/app/shared.js");
        Ok(source.into())
    }));
    context.set_module_fetch_budget(ModuleFetchBudget {
        urls: 1,
        bytes: source.len(),
    });
    context
        .prepare_document_modules(base, &[])
        .expect("empty registry");
    context
        .eval("import('./shared.js').then(m => globalThis.imported = m.value);")
        .expect("classic import");
    let roots = [ModuleScript::External(
        "https://example.test/app/shared.js".into(),
    )];
    let sources = [(
        "https://example.test/app/shared.js".into(),
        "throw new Error('replacement source');".into(),
    )];
    let results = context
        .eval_document_modules(base, &roots, &sources)
        .expect("document roots");
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    context
        .eval("if (executions !== 1 || imported !== 42) throw new Error('module identity');")
        .expect("same module record");
    assert_eq!(context.module_fetch_budget().urls, 0);
    assert_eq!(context.module_fetch_budget().bytes, 0);
}

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

#[test]
fn late_external_root_fetches_through_document_allowance() {
    let mut context = SilkContext::new();
    let source = "globalThis.lateRoot = 42;";
    context.set_document_url("https://example.test/");
    context.set_module_fetch_budget(silksurf_js::ModuleFetchBudget {
        urls: 1,
        bytes: source.len(),
    });
    context.set_module_fetcher(Box::new(move |url| {
        assert_eq!(url, "https://example.test/late.js");
        Ok(source.into())
    }));
    let result = context
        .eval_document_modules(
            "https://example.test/",
            &[ModuleScript::External(
                "https://example.test/late.js".into(),
            )],
            &[],
        )
        .unwrap();
    assert!(result[0].is_ok(), "{result:?}");
    context
        .eval("if (lateRoot !== 42) throw new Error('late root');")
        .unwrap();
    assert_eq!(context.module_fetch_budget().urls, 0);
}

#[test]
fn fetched_parse_failure_is_cached_across_imports() {
    use std::{cell::Cell, rc::Rc};
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let mut context = SilkContext::new();
    context.set_document_url("https://example.test/");
    context.set_module_fetch_budget(silksurf_js::ModuleFetchBudget {
        urls: 2,
        bytes: 100,
    });
    context.set_module_fetcher(Box::new(move |_| {
        observed.set(observed.get() + 1);
        Ok("export const = ;".into())
    }));
    context
        .eval("globalThis.rejections = 0; import('./bad.js').catch(() => rejections++);")
        .unwrap();
    context
        .eval("import('./bad.js').catch(() => rejections++);")
        .unwrap();
    assert_eq!(calls.get(), 1);
    context
        .eval("if (rejections !== 2) throw new Error('cached parse rejection');")
        .unwrap();
}

#[test]
fn import_map_registration_precedes_resolution_and_freezes_afterward() {
    let mut context = SilkContext::new();
    context.set_document_url("https://example.test/");
    context
        .prepare_document_modules(
            "https://example.test/",
            &[(
                "https://example.test/a.js".into(),
                "export const value = 42;".into(),
            )],
        )
        .unwrap();
    context.update_unresolved_import_map(silksurf_js::ImportMap::from_imports(vec![(
        "dep".into(),
        "/a.js".into(),
    )]));
    context
        .eval("import('dep').then(m => globalThis.first = m.value);")
        .unwrap();
    context.update_unresolved_import_map(silksurf_js::ImportMap::from_imports(vec![(
        "dep".into(),
        "/missing.js".into(),
    )]));
    context
        .eval("import('dep').then(m => globalThis.second = m.value);")
        .unwrap();
    context
        .eval("if (first !== 42 || second !== 42) throw new Error('map registration');")
        .unwrap();
}
