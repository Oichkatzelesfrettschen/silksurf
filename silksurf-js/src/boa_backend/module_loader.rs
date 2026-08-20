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

/// A document's import map: the top-level `imports` and the `scopes` that
/// override them by referrer (HTML 8.1.3.8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportMap {
    /// `imports` entries as (specifier key, target).
    pub imports: Vec<(String, String)>,
    /// `scopes` entries as (scope prefix, that scope's `imports`). The prefix
    /// is a URL; a scope applies to a referrer whose URL it prefixes.
    pub scopes: Vec<(String, Vec<(String, String)>)>,
}

impl ImportMap {
    /// The map holding only top-level `imports`.
    #[must_use]
    pub fn from_imports(imports: Vec<(String, String)>) -> Self {
        Self {
            imports,
            scopes: Vec::new(),
        }
    }
}

/// Registry of fetched modules plus the specifiers that missed it.
#[derive(Default)]
pub(super) struct PageModuleLoader {
    modules: RefCell<FxHashMap<String, Module>>,
    missing: RefCell<Vec<String>>,
    /// The document's import map, both member lists sorted longest key first
    /// so a prefix never shadows a longer, more specific match. It applies
    /// before URL resolution per HTML's resolve-a-module-specifier.
    import_map: RefCell<ImportMap>,
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

    /// Replace the import map.
    ///
    /// Both member lists sort by descending key length so a prefix mapping
    /// never shadows a longer, more specific one. A scope key is a URL, which
    /// HTML resolves against the document's address before it is compared to a
    /// referrer; a key that does not resolve names no referrer and is dropped.
    pub(super) fn set_import_map(&self, map: ImportMap) {
        let base = url::Url::parse(&self.document_url.borrow()).ok();
        let mut map = map;
        map.imports.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        map.scopes = map
            .scopes
            .into_iter()
            .filter_map(|(prefix, mut entries)| {
                entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
                let resolved = base
                    .as_ref()
                    .and_then(|base| base.join(&prefix).ok())
                    .map(|url| url.to_string())
                    .or_else(|| url::Url::parse(&prefix).ok().map(|url| url.to_string()))?;
                Some((resolved, entries))
            })
            .collect();
        map.scopes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        *self.import_map.borrow_mut() = map;
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
        let base = self.referrer_url(referrer);
        let mapped = self.apply_import_map(&base, specifier);
        if let Ok(absolute) = url::Url::parse(&mapped) {
            return Some(absolute.to_string());
        }
        let base = url::Url::parse(&base).ok()?;
        base.join(&mapped).ok().map(|url| url.to_string())
    }

    /// HTML's import-map lookup, scoped by referrer.
    ///
    /// The scopes whose prefix matches the referrer's URL are consulted first,
    /// longest prefix first, and the top-level `imports` answer when none of
    /// them holds the specifier. Applying a scoped mapping to every referrer
    /// would resolve the specifier to the wrong module, which is why the
    /// referrer reaches this lookup rather than the specifier alone.
    fn apply_import_map(&self, referrer_url: &str, specifier: &str) -> String {
        let map = self.import_map.borrow();
        for (prefix, entries) in &map.scopes {
            if referrer_url.starts_with(prefix.as_str())
                && let Some(mapped) = lookup_specifier(entries, specifier)
            {
                return mapped;
            }
        }
        lookup_specifier(&map.imports, specifier).unwrap_or_else(|| specifier.to_string())
    }
}

/// One import-map lookup: an exact key wins, then the longest key ending in
/// `/` that prefixes the specifier, with the remainder appended.
fn lookup_specifier(entries: &[(String, String)], specifier: &str) -> Option<String> {
    for (key, value) in entries {
        if key == specifier {
            return Some(value.clone());
        }
        if key.ends_with('/')
            && let Some(rest) = specifier.strip_prefix(key.as_str())
        {
            return Some(format!("{value}{rest}"));
        }
    }
    None
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
    import_map: &ImportMap,
) -> Result<Vec<String>, String> {
    let loader = Rc::new(PageModuleLoader::default());
    loader.set_document_url(module_url);
    loader.set_import_map(import_map.clone());
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
    use super::{ImportMap, module_import_specifiers};

    fn specifiers(source: &str) -> Vec<String> {
        module_import_specifiers(
            "https://example.test/app/main.js",
            source,
            &ImportMap::default(),
        )
        .expect("module parses")
    }

    fn imports(entries: &[(&str, &str)]) -> ImportMap {
        ImportMap::from_imports(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        )
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
        let map = imports(&[("react", "/vendor/react.js")]);
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
        let map = imports(&[("lib/", "/vendor/lib/")]);
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
        let map = imports(&[
            ("lib/", "/vendor/lib/"),
            ("lib/special/", "/vendor/special/"),
        ]);
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
            &ImportMap::default(),
        );
        assert!(result.is_err());
    }

    /*
     * A scope rewrites a specifier only for referrers its prefix covers, so
     * these cases pair one referrer inside the scope with one outside it. A
     * lookup that applied the scope to every referrer passes the first and
     * fails the second.
     */
    fn scoped(scopes: &[(&str, &[(&str, &str)])], top: &[(&str, &str)]) -> ImportMap {
        ImportMap {
            imports: top
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
            scopes: scopes
                .iter()
                .map(|(prefix, entries)| {
                    (
                        (*prefix).to_string(),
                        entries
                            .iter()
                            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn a_scope_rewrites_a_specifier_for_a_referrer_it_covers() {
        let map = scoped(
            &[("/app/", &[("react", "/vendor/react-18.js")])],
            &[("react", "/vendor/react-17.js")],
        );
        let found = module_import_specifiers(
            "https://example.test/app/main.js",
            "import React from 'react';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/react-18.js"]);
    }

    #[test]
    fn a_referrer_outside_every_scope_takes_the_top_level_mapping() {
        let map = scoped(
            &[("/app/", &[("react", "/vendor/react-18.js")])],
            &[("react", "/vendor/react-17.js")],
        );
        let found = module_import_specifiers(
            "https://example.test/legacy/main.js",
            "import React from 'react';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/react-17.js"]);
    }

    #[test]
    fn the_longest_matching_scope_wins() {
        let map = scoped(
            &[
                ("/app/", &[("dep", "/vendor/outer.js")]),
                ("/app/inner/", &[("dep", "/vendor/inner.js")]),
            ],
            &[("dep", "/vendor/top.js")],
        );
        let found = module_import_specifiers(
            "https://example.test/app/inner/main.js",
            "import d from 'dep';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/inner.js"]);
    }

    #[test]
    fn a_scope_that_omits_the_specifier_falls_through_to_imports() {
        let map = scoped(
            &[("/app/", &[("other", "/vendor/other.js")])],
            &[("react", "/vendor/react-17.js")],
        );
        let found = module_import_specifiers(
            "https://example.test/app/main.js",
            "import React from 'react';",
            &map,
        )
        .expect("module parses");
        assert_eq!(found, vec!["https://example.test/vendor/react-17.js"]);
    }
}
