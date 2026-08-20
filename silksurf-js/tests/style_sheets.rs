//! document.styleSheets and CSSStyleSheet over the live SheetSet.

use std::sync::{Arc, Mutex};

use silksurf_css::{LiveSheet, SheetOrigin, SheetSet, parse_stylesheet};
use silksurf_dom::Dom;
use silksurf_js::SilkContext;

struct Fixture {
    ctx: SilkContext,
    sheets: Arc<Mutex<SheetSet>>,
}

fn fixture(source: &str) -> Fixture {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let head = dom.create_element("head");
    let style = dom.create_element("style");
    dom.set_attribute(style, "id", "sheet")
        .expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, head).expect("head attaches");
    dom.append_child(head, style).expect("style attaches");
    let owner = style;

    let mut set = SheetSet::new();
    set.replace(vec![
        LiveSheet::new(
            SheetOrigin::UserAgent,
            parse_stylesheet("div { display: block }").expect("ua parses"),
        ),
        LiveSheet::new(
            SheetOrigin::Author,
            parse_stylesheet(source).expect("author parses"),
        )
        .with_owner(owner),
    ]);
    let sheets = Arc::new(Mutex::new(set));

    let mut ctx = SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    ctx.set_document_url("https://example.test/");
    ctx.set_style_sheets(&sheets);
    Fixture { ctx, sheets }
}

fn expect_ok(ctx: &mut SilkContext, script: &str) {
    if let Err(err) = ctx.eval(script) {
        panic!("script failed: {err}");
    }
}

#[test]
fn the_style_sheet_list_omits_the_user_agent_sheet() {
    let mut f = fixture("p { color: red }");
    expect_ok(
        &mut f.ctx,
        "if (document.styleSheets.length !== 1) \
           throw new Error('length was ' + document.styleSheets.length); \
         if (document.styleSheets[0].cssRules.length !== 1) \
           throw new Error('rules were ' + document.styleSheets[0].cssRules.length);",
    );
}

#[test]
fn a_sheet_reports_its_owner_node_by_identity() {
    let mut f = fixture("p { color: red }");
    expect_ok(
        &mut f.ctx,
        "var el = document.getElementById('sheet'); \
         if (document.styleSheets[0].ownerNode !== el) throw new Error('ownerNode mismatch'); \
         if (el.sheet !== document.styleSheets[0]) throw new Error('el.sheet mismatch');",
    );
}

#[test]
fn emotion_reaches_its_sheet_and_inserts_a_rule() {
    let mut f = fixture("p { color: red }");
    // The accessor and insert are Emotion's, in the shape the bundle ships:
    // the accessor call sits outside the try that guards insertRule.
    expect_ok(
        &mut f.ctx,
        "function sheetFor(e){ if(e.sheet) return e.sheet; \
           for(var t=0;t<document.styleSheets.length;t++) \
             if(document.styleSheets[t].ownerNode===e) return document.styleSheets[t]; } \
         var tag = document.getElementById('sheet'); \
         var n = sheetFor(tag); \
         n.insertRule('.css-1x2y3z{color:blue}', n.cssRules.length); \
         if (n.cssRules.length !== 2) throw new Error('rules were ' + n.cssRules.length);",
    );
    let set = f.sheets.lock().expect("set unlocked");
    assert_eq!(set.rule_count(1), 2, "the rule reached the engine's sheet");
    assert_eq!(set.selector_text(1, 1).as_deref(), Some(".css-1x2y3z"));
    assert!(
        set.script_generation() > 0,
        "the splice moved the scripted generation"
    );
}

#[test]
fn a_rule_reports_its_text_back_to_script() {
    let mut f = fixture("a.link > span { color: red; margin: 0 auto }");
    expect_ok(
        &mut f.ctx,
        "var rule = document.styleSheets[0].cssRules[0]; \
         if (rule.selectorText !== 'a.link > span') \
           throw new Error('selectorText was ' + rule.selectorText); \
         if (rule.cssText !== 'a.link > span { color: red; margin: 0 auto; }') \
           throw new Error('cssText was ' + rule.cssText); \
         if (rule.style.cssText !== 'color: red; margin: 0 auto;') \
           throw new Error('style.cssText was ' + rule.style.cssText);",
    );
}

#[test]
fn a_deleted_rule_leaves_the_sheet() {
    let mut f = fixture("p { color: red } a { color: blue }");
    expect_ok(
        &mut f.ctx,
        "document.styleSheets[0].deleteRule(0); \
         if (document.styleSheets[0].cssRules.length !== 1) throw new Error('not deleted');",
    );
    assert_eq!(f.sheets.lock().expect("set unlocked").rule_count(1), 1);
}

#[test]
fn a_malformed_rule_raises_a_syntax_error() {
    let mut f = fixture("p { color: red }");
    expect_ok(
        &mut f.ctx,
        "var threw = false; \
         try { document.styleSheets[0].insertRule('not a rule', 0); } catch (e) { threw = true; } \
         if (!threw) throw new Error('insertRule accepted invalid text');",
    );
    assert_eq!(f.sheets.lock().expect("set unlocked").rule_count(1), 1);
}

#[test]
fn a_detached_style_element_owns_no_sheet() {
    let mut f = fixture("p { color: red }");
    expect_ok(
        &mut f.ctx,
        "var fresh = document.createElement('style'); \
         if (fresh.sheet !== null) throw new Error('sheet was ' + fresh.sheet); \
         var link = document.createElement('link'); \
         if (link.sheet !== null) throw new Error('link sheet was ' + link.sheet);",
    );
}

#[test]
fn disabling_a_sheet_keeps_it_enumerable() {
    let mut f = fixture("p { color: red }");
    expect_ok(
        &mut f.ctx,
        "document.styleSheets[0].disabled = true; \
         if (!document.styleSheets[0].disabled) throw new Error('not disabled'); \
         if (document.styleSheets.length !== 1) throw new Error('dropped from the list');",
    );
    let set = f.sheets.lock().expect("set unlocked");
    assert_eq!(set.active_sheets().count(), 1, "the UA sheet stays active");
}
