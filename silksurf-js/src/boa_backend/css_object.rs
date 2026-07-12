/*
 * css_object backs `element.style` and `element.dataset` with the element's
 * attributes in the live Dom.
 *
 * A style write is an attribute write: `el.style.color = 'red'` upserts the
 * `color` declaration inside the `style` attribute through `set_attribute`,
 * which marks the node dirty -- the cascade already honors the inline style
 * attribute (silksurf-css apply_inline_style_attribute), so the incremental
 * repaint pipeline picks the change up with zero engine plumbing.
 *
 * The declaration text handling is deliberately textual: inline style values
 * written by scripts are single declarations (`width: 5px`), and preserving
 * the author's byte-exact text for untouched declarations matters more than
 * re-serializing through the token stream (which is lossy). Top-level
 * semicolon splitting tracks quotes and parentheses so `url(a;b)` and
 * `content: ";"` survive.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{Context, JsResult, JsString, JsValue, NativeFunction};
use silksurf_dom::{Dom, NodeId};

// ---- declaration text manipulation (pure) ------------------------------------

/// Split `text` at top-level semicolons into `(name, value)` pairs.
/// Malformed segments (no colon) are dropped, matching CSS error recovery.
pub(super) fn split_declarations(text: &str) -> Vec<(String, String)> {
    let mut segments = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' if depth <= 0 => {
                    segments.push(&text[start..i]);
                    start = i + ch.len_utf8();
                }
                _ => {}
            },
        }
    }
    segments.push(&text[start..]);

    segments
        .into_iter()
        .filter_map(|segment| {
            let (name, value) = segment.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

fn serialize_declarations(declarations: &[(String, String)]) -> String {
    declarations
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn get_declaration(text: &str, name: &str) -> Option<String> {
    split_declarations(text)
        .into_iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
}

pub(super) fn upsert_declaration(text: &str, name: &str, value: &str) -> String {
    let mut declarations = split_declarations(text);
    if let Some(entry) = declarations.iter_mut().find(|(n, _)| n == name) {
        entry.1 = value.to_string();
    } else {
        declarations.push((name.to_string(), value.to_string()));
    }
    serialize_declarations(&declarations)
}

pub(super) fn remove_declaration(text: &str, name: &str) -> String {
    let declarations: Vec<(String, String)> = split_declarations(text)
        .into_iter()
        .filter(|(n, _)| n != name)
        .collect();
    serialize_declarations(&declarations)
}

/// camelCase JS property name -> kebab-case CSS property name.
/// Already-kebab names pass through unchanged.
pub(super) fn camel_to_kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// camelCase dataset key -> data-* attribute name (dataset.fooBar -> data-foo-bar).
pub(super) fn dataset_attribute_name(key: &str) -> String {
    format!("data-{}", camel_to_kebab(key))
}

// ---- attribute plumbing -------------------------------------------------------

fn read_attribute(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, name: &str) -> Option<String> {
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    dom.attributes(node_id).ok().and_then(|attrs| {
        attrs
            .iter()
            .find(|attr| attr.name.as_str() == name)
            .map(|attr| attr.value.to_string())
    })
}

fn write_attribute(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, name: &str, value: &str) {
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = dom.set_attribute(node_id, name, value);
}

// ---- native globals -----------------------------------------------------------

fn two_string_args(args: &[JsValue], ctx: &mut Context) -> JsResult<(NodeId, String)> {
    let node = args
        .first()
        .map(|v| v.to_u32(ctx))
        .transpose()?
        .unwrap_or(0);
    let name = args
        .get(1)
        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_lossy()))
        .transpose()?
        .unwrap_or_default();
    Ok((NodeId::from_raw(node as usize), name))
}

/// Install the `__silksurfStyle*` / `__silksurfDataset*` native globals plus
/// the proxy-maker bootstrap. Called once per live-document install.
pub(super) fn install_style_dataset_natives(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let style_get = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, prop) = two_string_args(args, ctx)?;
            let prop = camel_to_kebab(&prop);
            let value = read_attribute(&arc, node, "style")
                .and_then(|text| get_declaration(&text, &prop))
                .unwrap_or_default();
            Ok(JsValue::from(JsString::from(value.as_str())))
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let style_set = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, prop) = two_string_args(args, ctx)?;
            let prop = camel_to_kebab(&prop);
            let value = args
                .get(2)
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let current = read_attribute(&arc, node, "style").unwrap_or_default();
            let next = if value.is_empty() {
                remove_declaration(&current, &prop)
            } else {
                upsert_declaration(&current, &prop, &value)
            };
            write_attribute(&arc, node, "style", &next);
            Ok(JsValue::undefined())
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let style_remove = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, prop) = two_string_args(args, ctx)?;
            let prop = camel_to_kebab(&prop);
            let current = read_attribute(&arc, node, "style").unwrap_or_default();
            let removed = get_declaration(&current, &prop).unwrap_or_default();
            write_attribute(&arc, node, "style", &remove_declaration(&current, &prop));
            Ok(JsValue::from(JsString::from(removed.as_str())))
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let style_css_text_get = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, _) = two_string_args(args, ctx)?;
            let text = read_attribute(&arc, node, "style").unwrap_or_default();
            Ok(JsValue::from(JsString::from(text.as_str())))
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let style_css_text_set = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, text) = two_string_args(args, ctx)?;
            write_attribute(&arc, node, "style", &text);
            Ok(JsValue::undefined())
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let dataset_get = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, key) = two_string_args(args, ctx)?;
            match read_attribute(&arc, node, &dataset_attribute_name(&key)) {
                Some(value) => Ok(JsValue::from(JsString::from(value.as_str()))),
                None => Ok(JsValue::undefined()),
            }
        })
    };

    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.
    let dataset_set = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let (node, key) = two_string_args(args, ctx)?;
            let value = args
                .get(2)
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            write_attribute(&arc, node, &dataset_attribute_name(&key), &value);
            Ok(JsValue::undefined())
        })
    };

    for (name, native) in [
        ("__silksurfStyleGet", style_get),
        ("__silksurfStyleSet", style_set),
        ("__silksurfStyleRemove", style_remove),
        ("__silksurfStyleCssTextGet", style_css_text_get),
        ("__silksurfStyleCssTextSet", style_css_text_set),
        ("__silksurfDatasetGet", dataset_get),
        ("__silksurfDatasetSet", dataset_set),
    ] {
        let _ = ctx.register_global_callable(JsString::from(name), 3, native);
    }

    // Proxy makers: arbitrary property names must route through the natives,
    // which only a Proxy get/set trap can do.
    let bootstrap = r"
        function __silksurfMakeStyleProxy(nodeId) {
            return new Proxy({}, {
                get: function (t, prop) {
                    if (prop === 'setProperty') {
                        return function (name, value) { __silksurfStyleSet(nodeId, name, String(value)); };
                    }
                    if (prop === 'getPropertyValue') {
                        return function (name) { return __silksurfStyleGet(nodeId, name); };
                    }
                    if (prop === 'removeProperty') {
                        return function (name) { return __silksurfStyleRemove(nodeId, name); };
                    }
                    if (prop === 'cssText') { return __silksurfStyleCssTextGet(nodeId); }
                    if (typeof prop !== 'string') { return undefined; }
                    return __silksurfStyleGet(nodeId, prop);
                },
                set: function (t, prop, value) {
                    if (prop === 'cssText') { __silksurfStyleCssTextSet(nodeId, String(value)); return true; }
                    if (typeof prop !== 'string') { return true; }
                    __silksurfStyleSet(nodeId, prop, String(value));
                    return true;
                }
            });
        }
        function __silksurfMakeDatasetProxy(nodeId) {
            return new Proxy({}, {
                get: function (t, prop) {
                    if (typeof prop !== 'string') { return undefined; }
                    return __silksurfDatasetGet(nodeId, prop);
                },
                set: function (t, prop, value) {
                    if (typeof prop !== 'string') { return true; }
                    __silksurfDatasetSet(nodeId, prop, String(value));
                    return true;
                }
            });
        }
    ";
    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(bootstrap.as_bytes())) {
        eprintln!("silksurf-js: style/dataset bootstrap failed: {err}");
    }
}

/// Build the live style or dataset object for a node wrapper by calling the
/// bootstrap proxy maker. Falls back to a plain object when the maker is
/// absent (contexts without the DOM bridge).
pub(super) fn make_proxy_for_node(maker: &str, node_id: NodeId, ctx: &mut Context) -> JsValue {
    let global = ctx.global_object().clone();
    let maker_fn = global
        .get(JsString::from(maker), ctx)
        .ok()
        .and_then(|value| value.as_callable());
    if let Some(function) = maker_fn {
        let node_arg = JsValue::from(node_id.raw() as u32);
        if let Ok(proxy) = function.call(&JsValue::undefined(), &[node_arg], ctx) {
            return proxy;
        }
    }
    JsValue::from(boa_engine::object::ObjectInitializer::new(ctx).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_split_handles_quotes_and_parens() {
        let cases = split_declarations("color: red; background: url(a;b.png); content: \";\"");
        assert_eq!(
            cases,
            vec![
                ("color".to_string(), "red".to_string()),
                ("background".to_string(), "url(a;b.png)".to_string()),
                ("content".to_string(), "\";\"".to_string()),
            ]
        );
    }

    #[test]
    fn upsert_replaces_and_appends() {
        assert_eq!(
            upsert_declaration("color: red", "color", "blue"),
            "color: blue"
        );
        assert_eq!(
            upsert_declaration("color: red", "width", "5px"),
            "color: red; width: 5px"
        );
        assert_eq!(upsert_declaration("", "width", "5px"), "width: 5px");
    }

    #[test]
    fn remove_drops_only_named_property() {
        assert_eq!(
            remove_declaration("color: red; width: 5px", "color"),
            "width: 5px"
        );
    }

    #[test]
    fn camel_case_maps_to_kebab() {
        assert_eq!(camel_to_kebab("backgroundColor"), "background-color");
        assert_eq!(camel_to_kebab("border-width"), "border-width");
        assert_eq!(dataset_attribute_name("fooBar"), "data-foo-bar");
    }
}
