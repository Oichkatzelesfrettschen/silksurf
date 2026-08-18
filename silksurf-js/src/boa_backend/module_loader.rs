/*
 * module_loader resolves module specifiers the way the HTML and ECMAScript
 * standards define, and answers `import.meta`.
 *
 * boa calls `load_imported_module` once per requested specifier with the
 * referrer that named it, which is the engine's own parser reporting what a
 * module imports. That is the authority a scanner over the source text cannot
 * be: a minified bundle writes `import{a}from"./m.js"` with no space around
 * `from`, and every bundler emits exactly that.
 *
 * Modules are keyed by absolute URL. `Source::with_path` carries the URL into
 * `Module::path`, so the referrer of a relative specifier is the importing
 * module's own URL and `url::Url::join` performs the resolution. `import.meta`
 * reads its `url` from the same key.
 *
 * A specifier the registry does not hold is recorded in `missing` and reported
 * as an error, so a caller drives the fetch rounds from what the parser
 * actually requested.
 */

use std::cell::RefCell;
use std::rc::Rc;

use boa_engine::{
    Context, JsError, JsNativeError, JsObject, JsResult, JsString, Source, js_string,
    module::{Module, ModuleLoader, Referrer},
};
use rustc_hash::FxHashMap;

/// Registry of fetched modules plus the specifiers that missed it.
#[derive(Default)]
pub(super) struct PageModuleLoader {
    modules: RefCell<FxHashMap<String, Module>>,
    missing: RefCell<Vec<String>>,
    /// Import map `imports` entries, longest key first, applied before URL
    /// resolution per HTML's resolve-a-module-specifier.
    import_map: RefCell<Vec<(String, String)>>,
    /// The document's address, the referrer for a specifier named by page
    /// script rather than by another module.
    document_url: RefCell<String>,
}

impl PageModuleLoader {
    pub(super) fn clear(&self) {
        self.modules.borrow_mut().clear();
        self.missing.borrow_mut().clear();
    }

    pub(super) fn insert(&self, url: &str, module: Module) {
        self.modules.borrow_mut().insert(url.to_string(), module);
    }

    pub(super) fn take_missing(&self) -> Vec<String> {
        std::mem::take(&mut *self.missing.borrow_mut())
    }

    pub(super) fn set_document_url(&self, url: &str) {
        *self.document_url.borrow_mut() = url.to_string();
    }

    /// Replace the import map. Entries sort by descending key length so a
    /// prefix mapping never shadows a longer, more specific one.
    pub(super) fn set_import_map(&self, entries: Vec<(String, String)>) {
        let mut entries = entries;
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        *self.import_map.borrow_mut() = entries;
    }

    fn referrer_url(&self, referrer: &Referrer) -> String {
        match referrer {
            Referrer::Module(module) => module
                .path()
                .and_then(|path| path.to_str())
                .map_or_else(|| self.document_url.borrow().clone(), str::to_string),
            _ => self.document_url.borrow().clone(),
        }
    }

    /// Apply the import map, then resolve against the referrer's URL.
    fn resolve(&self, referrer: &Referrer, specifier: &str) -> Option<String> {
        let mapped = self.apply_import_map(specifier);
        let base = self.referrer_url(referrer);
        if let Ok(absolute) = url::Url::parse(&mapped) {
            return Some(absolute.to_string());
        }
        let base = url::Url::parse(&base).ok()?;
        base.join(&mapped).ok().map(|url| url.to_string())
    }

    /// HTML's import-map lookup: an exact key wins, then the longest key ending
    /// in `/` that prefixes the specifier, with the remainder appended.
    fn apply_import_map(&self, specifier: &str) -> String {
        for (key, value) in self.import_map.borrow().iter() {
            if key == specifier {
                return value.clone();
            }
            if key.ends_with('/')
                && let Some(rest) = specifier.strip_prefix(key.as_str())
            {
                return format!("{value}{rest}");
            }
        }
        specifier.to_string()
    }
}

impl ModuleLoader for PageModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        specifier: JsString,
        _context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let specifier = specifier.to_std_string_lossy();
        let Some(url) = self.resolve(&referrer, &specifier) else {
            return Err(JsNativeError::typ()
                .with_message(format!("module specifier {specifier} does not resolve"))
                .into());
        };
        if let Some(module) = self.modules.borrow().get(&url) {
            return Ok(module.clone());
        }
        self.missing.borrow_mut().push(url.clone());
        Err(JsNativeError::typ()
            .with_message(format!("module {url} was not fetched"))
            .into())
    }

    fn init_import_meta(
        self: Rc<Self>,
        import_meta: &JsObject,
        module: &Module,
        context: &mut Context,
    ) {
        let url = module
            .path()
            .and_then(|path| path.to_str())
            .map_or_else(|| self.document_url.borrow().clone(), str::to_string);
        let _ = import_meta.set(js_string!("url"), js_string!(url.as_str()), false, context);
    }
}

/*
 * module_import_specifiers -- the URLs one module source imports.
 *
 * A throwaway context parses the source and drives `Module::load` against an
 * empty registry, so every specifier boa's parser reports lands in `missing`.
 * The caller fetches those URLs and repeats, which walks the graph without a
 * scanner over the source text.
 *
 * # Errors
 *
 * Returns the parse error when the source is not a valid module.
 */
pub fn module_import_specifiers(
    module_url: &str,
    source: &str,
    import_map: &[(String, String)],
) -> Result<Vec<String>, String> {
    let loader = Rc::new(PageModuleLoader::default());
    loader.set_document_url(module_url);
    loader.set_import_map(import_map.to_vec());
    let mut context = Context::builder()
        .module_loader(Rc::clone(&loader))
        .build()
        .map_err(|err| format!("module scan context: {err}"))?;
    let path = std::path::PathBuf::from(module_url);
    let parsed = Module::parse(
        Source::from_bytes(source.as_bytes()).with_path(path.as_path()),
        None,
        &mut context,
    )
    .map_err(|err: JsError| format!("module parse {module_url}: {err}"))?;
    let _ = parsed.load(&mut context);
    let _ = context.run_jobs();
    Ok(loader.take_missing())
}

#[cfg(test)]
mod tests {
    use super::module_import_specifiers;

    fn specifiers(source: &str) -> Vec<String> {
        module_import_specifiers("https://example.test/app/main.js", source, &[])
            .expect("module parses")
    }

    #[test]
    fn a_minified_import_clause_resolves_without_spaces() {
        let found = specifiers("import{a as b}from\"./runtime.js\";export default b;");
        assert_eq!(found, vec!["https://example.test/app/runtime.js"]);
    }

    #[test]
    fn every_import_form_reports_its_specifier() {
        let found = specifiers(
            "import 'a.js';\
             import d from './b.js';\
             import * as ns from '../c.js';\
             export { x } from './d.js';\
             export * from './e.js';",
        );
        assert!(found.contains(&"https://example.test/app/a.js".to_string()));
        assert!(found.contains(&"https://example.test/app/b.js".to_string()));
        assert!(found.contains(&"https://example.test/c.js".to_string()));
        assert!(found.contains(&"https://example.test/app/d.js".to_string()));
        assert!(found.contains(&"https://example.test/app/e.js".to_string()));
    }

    #[test]
    fn a_string_holding_the_word_import_is_not_a_specifier() {
        let found = specifiers("var text = 'import x from \"./ghost.js\"'; export default text;");
        assert!(found.is_empty(), "found {found:?}");
    }

    #[test]
    fn import_meta_is_not_a_module_request() {
        let found = specifiers("export const here = import.meta.url;");
        assert!(found.is_empty(), "found {found:?}");
    }

    #[test]
    fn an_absolute_specifier_keeps_its_own_origin() {
        let found = specifiers("import 'https://cdn.test/lib.js';");
        assert_eq!(found, vec!["https://cdn.test/lib.js"]);
    }

    #[test]
    fn an_import_map_rewrites_a_bare_specifier() {
        let map = vec![("react".to_string(), "/vendor/react.js".to_string())];
        let found = module_import_specifiers(
            "https://example.test/app/main.js",
            "import React from 'react';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/react.js"]);
    }

    #[test]
    fn an_import_map_prefix_key_maps_the_remainder() {
        let map = vec![("lib/".to_string(), "/vendor/lib/".to_string())];
        let found = module_import_specifiers(
            "https://example.test/app/main.js",
            "import x from 'lib/deep/mod.js';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/lib/deep/mod.js"]);
    }

    #[test]
    fn a_longer_import_map_key_wins_over_a_shorter_prefix() {
        let map = vec![
            ("lib/".to_string(), "/vendor/lib/".to_string()),
            ("lib/special/".to_string(), "/vendor/special/".to_string()),
        ];
        let found = module_import_specifiers(
            "https://example.test/app/main.js",
            "import x from 'lib/special/mod.js';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/special/mod.js"]);
    }

    #[test]
    fn a_syntax_error_reports_the_parse_failure() {
        let result = module_import_specifiers(
            "https://example.test/app/main.js",
            "import {{{ from 'broken';",
            &[],
        );
        assert!(result.is_err());
    }
}
