//! CSS Custom Properties (CSS Variables) per CSS Custom Properties Level 1.
//!
//! Custom properties are declared with `--name: value` and referenced with
//! `var(--name)` or `var(--name, fallback)`. They inherit by default.

use crate::CssToken;
use rustc_hash::FxHashMap;

/// Storage for custom property values.
/// Maps `--property-name` -> token list value.
#[derive(Debug, Clone, Default)]
pub struct CustomPropertyMap {
    properties: FxHashMap<String, Vec<CssToken>>,
}

impl CustomPropertyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a custom property value.
    pub fn set(&mut self, name: &str, value: Vec<CssToken>) {
        self.properties.insert(name.to_string(), value);
    }

    /// Get a custom property value.
    pub fn get(&self, name: &str) -> Option<&[CssToken]> {
        self.properties.get(name).map(Vec::as_slice)
    }

    /// Merge another map into this one (child inherits from parent).
    pub fn inherit_from(&mut self, parent: &CustomPropertyMap) {
        for (name, value) in &parent.properties {
            self.properties
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
    }

    /// Check if a declaration name is a custom property (starts with --).
    pub fn is_custom_property(name: &str) -> bool {
        name.starts_with("--")
    }
}

/// Resolve `var(--name)` and `var(--name, fallback)` references in a token list.
///
/// Returns a new token list with all var() references substituted.
/// Handles nested var() in fallback values.
pub fn resolve_var_references(
    tokens: &[CssToken],
    custom_props: &CustomPropertyMap,
) -> Vec<CssToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            CssToken::Function(name) if name.eq_ignore_ascii_case("var") => {
                i += 1; // skip 'var('
                // Parse var(--name) or var(--name, fallback)
                let (resolved, new_i) = resolve_single_var(&tokens[i..], custom_props);
                result.extend(resolved);
                i += new_i;
            }
            token => {
                result.push(token.clone());
                i += 1;
            }
        }
    }

    result
}

/// Parse and resolve a single var() reference starting after the 'var(' function token.
/// Returns (resolved tokens, number of tokens consumed).
fn resolve_single_var(
    tokens: &[CssToken],
    custom_props: &CustomPropertyMap,
) -> (Vec<CssToken>, usize) {
    let mut i = 0;

    // Skip whitespace
    while i < tokens.len() && matches!(tokens[i], CssToken::Whitespace) {
        i += 1;
    }

    // Extract property name (should be an ident starting with --)
    let prop_name = match tokens.get(i) {
        Some(CssToken::Ident(name)) if name.starts_with("--") => {
            i += 1;
            name.clone()
        }
        _ => return (vec![], i),
    };

    // Skip whitespace
    while i < tokens.len() && matches!(tokens[i], CssToken::Whitespace) {
        i += 1;
    }

    // Check for comma (fallback value follows)
    let fallback = if matches!(tokens.get(i), Some(CssToken::Comma)) {
        i += 1; // skip comma
        // Collect everything until matching close paren
        let mut depth = 1;
        let fallback_start = i;
        while i < tokens.len() && depth > 0 {
            match &tokens[i] {
                CssToken::ParenOpen | CssToken::Function(_) => depth += 1,
                CssToken::ParenClose => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        Some(tokens[fallback_start..i].to_vec())
    } else {
        None
    };

    // Skip close paren
    if matches!(tokens.get(i), Some(CssToken::ParenClose)) {
        i += 1;
    }

    // Resolve: look up property, use fallback if not found
    if let Some(value) = custom_props.get(&prop_name) {
        // Recursively resolve var() in the property value itself
        let resolved = resolve_var_references(value, custom_props);
        (resolved, i)
    } else if let Some(fallback) = fallback {
        // Recursively resolve var() in fallback
        let resolved = resolve_var_references(&fallback, custom_props);
        (resolved, i)
    } else {
        // No value, no fallback -- return empty (property becomes invalid)
        (vec![], i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_property_set_get() {
        let mut map = CustomPropertyMap::new();
        map.set("--color", vec![CssToken::Ident("red".into())]);
        assert!(map.get("--color").is_some());
        assert!(map.get("--other").is_none());
    }

    #[test]
    fn test_is_custom_property() {
        assert!(CustomPropertyMap::is_custom_property("--color"));
        assert!(CustomPropertyMap::is_custom_property("--my-var"));
        assert!(!CustomPropertyMap::is_custom_property("color"));
        assert!(!CustomPropertyMap::is_custom_property("-webkit-thing"));
    }

    #[test]
    fn test_resolve_var_simple() {
        let mut props = CustomPropertyMap::new();
        props.set("--primary", vec![CssToken::Ident("blue".into())]);

        let tokens = vec![
            CssToken::Function("var".into()),
            CssToken::Ident("--primary".into()),
            CssToken::ParenClose,
        ];
        let resolved = resolve_var_references(&tokens, &props);
        assert_eq!(resolved, vec![CssToken::Ident("blue".into())]);
    }

    #[test]
    fn test_resolve_var_fallback() {
        let props = CustomPropertyMap::new(); // empty -- no --missing

        let tokens = vec![
            CssToken::Function("var".into()),
            CssToken::Ident("--missing".into()),
            CssToken::Comma,
            CssToken::Ident("green".into()),
            CssToken::ParenClose,
        ];
        let resolved = resolve_var_references(&tokens, &props);
        assert_eq!(resolved, vec![CssToken::Ident("green".into())]);
    }

    #[test]
    fn test_resolve_var_missing_no_fallback() {
        let props = CustomPropertyMap::new();
        let tokens = vec![
            CssToken::Function("var".into()),
            CssToken::Ident("--missing".into()),
            CssToken::ParenClose,
        ];
        let resolved = resolve_var_references(&tokens, &props);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_inherit() {
        let mut parent = CustomPropertyMap::new();
        parent.set("--from-parent", vec![CssToken::Number("42".into())]);

        let mut child = CustomPropertyMap::new();
        child.set("--from-child", vec![CssToken::Ident("own".into())]);
        child.inherit_from(&parent);

        assert!(child.get("--from-parent").is_some());
        assert!(child.get("--from-child").is_some());
    }
}
