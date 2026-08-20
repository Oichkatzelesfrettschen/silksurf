/*
 * style_sheets backs `document.styleSheets`, `CSSStyleSheet`, and the rule
 * objects they hand out with the live SheetSet the engine cascades from.
 *
 * A sheet is addressed by its position in the author list, which omits the
 * user-agent sheet, so `document.styleSheets[i]` and the natives below agree
 * on one index. Every native re-locks the set, so a sheet object script holds
 * across a repaint keeps answering from current state rather than a snapshot.
 *
 * Emotion reaches its sheet through `styleElement.sheet` and falls back to
 * scanning `document.styleSheets` for a matching `ownerNode`; both paths land
 * on the same object because the sheet wrapper is keyed by author index and
 * ownerNode returns the node wrapper the registry already cached.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{Context, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction};
use silksurf_css::{SheetError, SheetSet};
use silksurf_dom::{Dom, NodeId};

use super::dom_bridge::node_to_js_object;

type Sheets = Arc<Mutex<SheetSet>>;

fn author_index(sheets: &SheetSet, position: usize) -> Option<usize> {
    sheets.author_indices().get(position).copied()
}

fn arg_usize(args: &[JsValue], at: usize, ctx: &mut Context) -> JsResult<usize> {
    let value = args.get(at).map(|value| value.to_number(ctx)).transpose()?;
    Ok(value.filter(|n| *n >= 0.0).unwrap_or(0.0) as usize)
}

fn arg_string(args: &[JsValue], at: usize, ctx: &mut Context) -> JsResult<String> {
    let value = args
        .get(at)
        .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
        .transpose()?;
    Ok(value.unwrap_or_default())
}

/// Map a refused mutation onto the `DOMException` name CSSOM names for it.
fn sheet_error(error: SheetError) -> JsError {
    match error {
        SheetError::Syntax => JsNativeError::syntax()
            .with_message("the text does not parse as one rule")
            .into(),
        SheetError::IndexSize | SheetError::NoSuchSheet => JsNativeError::range()
            .with_message("the index is past the end of the rule list")
            .into(),
    }
}

fn string_value(text: Option<String>) -> JsValue {
    JsValue::from(JsString::from(text.unwrap_or_default().as_str()))
}

type Native = (&'static str, NativeFunction);

/// The natives answering sheet identity: how many sheets script sees, which
/// node owns one, and which sheet a node owns.
fn identity_natives(sheets: &Sheets, dom_arc: &Arc<Mutex<Dom>>) -> Vec<Native> {
    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let count = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(JsValue::from(set.author_indices().len() as u32))
        })
    };

    let set = Arc::clone(sheets);
    let dom = Arc::clone(dom_arc);
    // SAFETY: Boa stores the closure with owned Arc handles for the function lifetime.
    let owner_node = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let owner = {
                let set = set.lock().unwrap_or_else(PoisonError::into_inner);
                author_index(&set, position)
                    .and_then(|index| set.sheets().get(index).and_then(|sheet| sheet.owner))
            };
            Ok(owner.map_or(JsValue::null(), |node| node_to_js_object(&dom, node, ctx)))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let owner_index = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let node = NodeId::from_raw(arg_usize(args, 0, ctx)?);
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let position = set
                .index_of_owner(node)
                .and_then(|index| set.author_indices().iter().position(|at| *at == index));
            Ok(position.map_or(JsValue::from(-1), |at| {
                JsValue::from(u32::try_from(at).unwrap_or(0))
            }))
        })
    };

    vec![
        ("__silksurfSheetCount", count),
        ("__silksurfSheetOwnerNode", owner_node),
        ("__silksurfSheetOwnerIndex", owner_index),
    ]
}

/// The natives answering a sheet's own attributes: href, media, and disabled.
fn metadata_natives(sheets: &Sheets) -> Vec<Native> {
    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let href = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let href = author_index(&set, position)
                .and_then(|index| set.sheets().get(index))
                .and_then(|sheet| sheet.href.clone());
            Ok(href.map_or(JsValue::null(), |href| {
                JsValue::from(JsString::from(href.as_str()))
            }))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let media = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let media = author_index(&set, position)
                .and_then(|index| set.sheets().get(index))
                .map(|sheet| sheet.media.clone());
            Ok(string_value(media))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let disabled_get = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let disabled = author_index(&set, position)
                .and_then(|index| set.sheets().get(index))
                .is_some_and(|sheet| sheet.disabled);
            Ok(JsValue::from(disabled))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let disabled_set = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let disabled = args.get(1).is_some_and(JsValue::to_boolean);
            let mut set = set.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(index) = author_index(&set, position) {
                let _ = set.set_disabled(index, disabled);
            }
            Ok(JsValue::undefined())
        })
    };

    vec![
        ("__silksurfSheetHref", href),
        ("__silksurfSheetMedia", media),
        ("__silksurfSheetDisabledGet", disabled_get),
        ("__silksurfSheetDisabledSet", disabled_set),
    ]
}

/// The natives serializing a rule back to the text CSSOM hands script.
fn rule_natives(sheets: &Sheets) -> Vec<Native> {
    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let rule_count = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let count = author_index(&set, position).map_or(0, |index| set.rule_count(index));
            Ok(JsValue::from(count as u32))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let rule_text = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let rule = arg_usize(args, 1, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let text = author_index(&set, position).and_then(|index| set.rule_text(index, rule));
            Ok(string_value(text))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let selector_text = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let rule = arg_usize(args, 1, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let text =
                author_index(&set, position).and_then(|index| set.selector_text(index, rule));
            Ok(string_value(text))
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let declaration_text = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let rule = arg_usize(args, 1, ctx)?;
            let set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let text =
                author_index(&set, position).and_then(|index| set.declaration_text(index, rule));
            Ok(string_value(text))
        })
    };

    vec![
        ("__silksurfSheetRuleCount", rule_count),
        ("__silksurfSheetRuleText", rule_text),
        ("__silksurfSheetSelectorText", selector_text),
        ("__silksurfSheetDeclarationText", declaration_text),
    ]
}

/// The natives splicing a sheet's rule list.
fn mutation_natives(sheets: &Sheets) -> Vec<Native> {
    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let insert_rule = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let text = arg_string(args, 1, ctx)?;
            let at = arg_usize(args, 2, ctx)?;
            let mut set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(index) = author_index(&set, position) else {
                return Err(sheet_error(SheetError::NoSuchSheet));
            };
            match set.insert_rule(index, &text, at) {
                Ok(at) => Ok(JsValue::from(at as u32)),
                Err(error) => Err(sheet_error(error)),
            }
        })
    };

    let set = Arc::clone(sheets);
    // SAFETY: Boa stores the closure with an owned Arc for the function lifetime.
    let delete_rule = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let position = arg_usize(args, 0, ctx)?;
            let at = arg_usize(args, 1, ctx)?;
            let mut set = set.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(index) = author_index(&set, position) else {
                return Err(sheet_error(SheetError::NoSuchSheet));
            };
            match set.delete_rule(index, at) {
                Ok(()) => Ok(JsValue::undefined()),
                Err(error) => Err(sheet_error(error)),
            }
        })
    };

    vec![
        ("__silksurfSheetInsertRule", insert_rule),
        ("__silksurfSheetDeleteRule", delete_rule),
    ]
}

/// Install the `__silksurfSheet*` natives and the CSSOM bootstrap.
pub(super) fn install_style_sheet_natives(
    sheets: &Sheets,
    dom_arc: &Arc<Mutex<Dom>>,
    ctx: &mut Context,
) {
    let natives = identity_natives(sheets, dom_arc)
        .into_iter()
        .chain(metadata_natives(sheets))
        .chain(rule_natives(sheets))
        .chain(mutation_natives(sheets));
    for (name, native) in natives {
        let _ = ctx.register_global_callable(JsString::from(name), 3, native);
    }

    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(CSSOM_BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: CSSOM bootstrap failed: {err}");
    }
}

/*
 * The CSSOM object graph, built in JS over the natives above.
 *
 * A sheet wrapper caches by author index so `styleElement.sheet` and a scan of
 * `document.styleSheets` return the same object, which is the identity
 * `ownerNode ===` comparisons depend on. Index access on a rule list and on
 * the sheet list goes through a Proxy, because a numeric property name reaches
 * no ordinary accessor.
 */
const CSSOM_BOOTSTRAP: &str = r"
    (function () {
      var sheetCache = Object.create(null);

      function CSSRule() {}
      function CSSStyleRule() {}
      CSSStyleRule.prototype = Object.create(CSSRule.prototype);
      function CSSStyleSheet() {}
      function CSSRuleList() {}
      function StyleSheetList() {}

      function makeRule(sheetIndex, ruleIndex) {
        var rule = Object.create(CSSStyleRule.prototype);
        Object.defineProperty(rule, 'cssText', {
          enumerable: true,
          get: function () { return __silksurfSheetRuleText(sheetIndex, ruleIndex); }
        });
        Object.defineProperty(rule, 'selectorText', {
          enumerable: true,
          get: function () { return __silksurfSheetSelectorText(sheetIndex, ruleIndex); }
        });
        Object.defineProperty(rule, 'style', {
          enumerable: true,
          get: function () {
            return { cssText: __silksurfSheetDeclarationText(sheetIndex, ruleIndex) };
          }
        });
        Object.defineProperty(rule, 'parentStyleSheet', {
          enumerable: true,
          get: function () { return __silksurfSheetAt(sheetIndex); }
        });
        rule.type = 1;
        return rule;
      }

      function makeRuleList(sheetIndex) {
        var list = Object.create(CSSRuleList.prototype);
        Object.defineProperty(list, 'length', {
          get: function () { return __silksurfSheetRuleCount(sheetIndex); }
        });
        list.item = function (at) {
          if (at < 0 || at >= __silksurfSheetRuleCount(sheetIndex)) { return null; }
          return makeRule(sheetIndex, at);
        };
        return new Proxy(list, {
          get: function (target, prop, receiver) {
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
              return target.item(Number(prop));
            }
            return Reflect.get(target, prop, receiver);
          },
          has: function (target, prop) {
            if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
              return Number(prop) < __silksurfSheetRuleCount(sheetIndex);
            }
            return Reflect.has(target, prop);
          }
        });
      }

      function __silksurfSheetAt(sheetIndex) {
        if (sheetIndex < 0 || sheetIndex >= __silksurfSheetCount()) { return null; }
        var cached = sheetCache[sheetIndex];
        if (cached) { return cached; }
        var sheet = Object.create(CSSStyleSheet.prototype);
        Object.defineProperty(sheet, 'cssRules', {
          enumerable: true,
          get: function () { return makeRuleList(sheetIndex); }
        });
        Object.defineProperty(sheet, 'rules', {
          get: function () { return makeRuleList(sheetIndex); }
        });
        Object.defineProperty(sheet, 'ownerNode', {
          enumerable: true,
          get: function () { return __silksurfSheetOwnerNode(sheetIndex); }
        });
        Object.defineProperty(sheet, 'href', {
          enumerable: true,
          get: function () { return __silksurfSheetHref(sheetIndex); }
        });
        Object.defineProperty(sheet, 'media', {
          enumerable: true,
          get: function () { return { mediaText: __silksurfSheetMedia(sheetIndex) }; }
        });
        Object.defineProperty(sheet, 'disabled', {
          enumerable: true,
          get: function () { return __silksurfSheetDisabledGet(sheetIndex); },
          set: function (value) { __silksurfSheetDisabledSet(sheetIndex, !!value); }
        });
        sheet.type = 'text/css';
        sheet.ownerRule = null;
        sheet.parentStyleSheet = null;
        sheet.insertRule = function (text, at) {
          return __silksurfSheetInsertRule(sheetIndex, String(text),
            at === undefined ? __silksurfSheetRuleCount(sheetIndex) : Number(at));
        };
        sheet.deleteRule = function (at) {
          return __silksurfSheetDeleteRule(sheetIndex, Number(at));
        };
        sheetCache[sheetIndex] = sheet;
        return sheet;
      }
      globalThis.__silksurfSheetAt = __silksurfSheetAt;

      var list = Object.create(StyleSheetList.prototype);
      Object.defineProperty(list, 'length', {
        get: function () { return __silksurfSheetCount(); }
      });
      list.item = function (at) { return __silksurfSheetAt(Number(at)); };
      var styleSheets = new Proxy(list, {
        get: function (target, prop, receiver) {
          if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
            return __silksurfSheetAt(Number(prop));
          }
          return Reflect.get(target, prop, receiver);
        },
        has: function (target, prop) {
          if (typeof prop === 'string' && /^[0-9]+$/.test(prop)) {
            return Number(prop) < __silksurfSheetCount();
          }
          return Reflect.has(target, prop);
        }
      });

      Object.defineProperty(document, 'styleSheets', {
        configurable: true,
        get: function () { return styleSheets; }
      });

      globalThis.CSSRule = CSSRule;
      globalThis.CSSStyleRule = CSSStyleRule;
      globalThis.CSSStyleSheet = CSSStyleSheet;
      globalThis.CSSRuleList = CSSRuleList;
      globalThis.StyleSheetList = StyleSheetList;

      globalThis.__silksurfSheetForNode = function (nodeId) {
        var at = __silksurfSheetOwnerIndex(nodeId);
        return at < 0 ? null : __silksurfSheetAt(at);
      };

      // HTMLStyleElement.sheet and HTMLLinkElement.sheet are the branch
      // Emotion takes before it scans the list. The element prototype is
      // reachable only through an instance, and it answers null for an
      // element that owns no sheet.
      var seen = [];
      ['style', 'link'].forEach(function (tag) {
        var proto = Object.getPrototypeOf(document.createElement(tag));
        if (!proto || seen.indexOf(proto) >= 0) { return; }
        seen.push(proto);
        Object.defineProperty(proto, 'sheet', {
          configurable: true,
          get: function () { return __silksurfSheetForNode(this.nodeId); }
        });
      });
    })();
";
