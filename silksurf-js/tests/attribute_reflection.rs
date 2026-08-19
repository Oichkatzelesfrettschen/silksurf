//! IDL attribute reflection: element properties that read and write a content
//! attribute rather than a plain own property.

use std::sync::{Arc, Mutex};

use silksurf_dom::Dom;
use silksurf_js::SilkContext;

fn context_with_document() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let head = dom.create_element("head");
    let body = dom.create_element("body");
    let link = dom.create_element("link");
    dom.set_attribute(link, "id", "sheet").expect("id attaches");
    dom.set_attribute(link, "rel", "preload")
        .expect("rel attaches");
    dom.set_attribute(link, "href", "/late.css")
        .expect("href attaches");
    let image = dom.create_element("img");
    dom.set_attribute(image, "id", "pic").expect("id attaches");
    dom.set_attribute(image, "src", "/photo.png")
        .expect("src attaches");
    let input = dom.create_element("input");
    dom.set_attribute(input, "id", "field")
        .expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, head).expect("head attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(head, link).expect("link attaches");
    dom.append_child(body, image).expect("img attaches");
    dom.append_child(body, input).expect("input attaches");
    let mut ctx = SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    ctx.set_document_url("https://example.test/app/index.html");
    ctx
}

#[test]
fn a_string_reflection_reads_the_content_attribute() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var link = document.getElementById('sheet'); \
         if (link.rel !== 'preload') throw new Error('rel was ' + link.rel); \
         if (document.getElementById('field').type !== '') \
             throw new Error('absent attribute is not the empty string');",
    )
    .expect("string reflection reads through");
}

#[test]
fn a_string_reflection_write_reaches_get_attribute() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var link = document.getElementById('sheet'); \
         link.rel = 'stylesheet'; \
         if (link.getAttribute('rel') !== 'stylesheet') \
             throw new Error('getAttribute saw ' + link.getAttribute('rel')); \
         if (link.rel !== 'stylesheet') throw new Error('read back ' + link.rel);",
    )
    .expect("a reflected write lands on the content attribute");
}

#[test]
fn a_url_reflection_resolves_against_the_document_address() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var image = document.getElementById('pic'); \
         if (image.src !== 'https://example.test/photo.png') \
             throw new Error('src was ' + image.src); \
         if (image.getAttribute('src') !== '/photo.png') \
             throw new Error('content attribute was rewritten'); \
         var link = document.getElementById('sheet'); \
         if (link.href !== 'https://example.test/late.css') \
             throw new Error('href was ' + link.href);",
    )
    .expect("URL reflection resolves and leaves the attribute raw");
}

#[test]
fn a_boolean_reflection_tracks_attribute_presence() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var field = document.getElementById('field'); \
         if (field.disabled !== false) throw new Error('absent attribute is not false'); \
         field.disabled = true; \
         if (field.getAttribute('disabled') !== '') \
             throw new Error('set did not write the empty attribute'); \
         if (field.disabled !== true) throw new Error('read back false'); \
         field.disabled = false; \
         if (field.hasAttribute('disabled')) throw new Error('clear did not remove it');",
    )
    .expect("boolean reflection adds and removes the attribute");
}

#[test]
fn a_long_reflection_parses_and_defaults_to_zero() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var image = document.getElementById('pic'); \
         if (image.width !== 0) throw new Error('absent long is ' + image.width); \
         image.width = 240; \
         if (image.getAttribute('width') !== '240') \
             throw new Error('attribute is ' + image.getAttribute('width')); \
         if (image.width !== 240) throw new Error('read back ' + image.width); \
         image.setAttribute('height', 'not-a-number'); \
         if (image.height !== 0) throw new Error('unparsable long is ' + image.height);",
    )
    .expect("long reflection parses with a zero default");
}

#[test]
fn a_reflected_name_differs_from_its_content_attribute() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var label = document.createElement('label'); \
         label.htmlFor = 'field'; \
         if (label.getAttribute('for') !== 'field') \
             throw new Error('htmlFor wrote ' + label.getAttribute('for')); \
         var meta = document.createElement('meta'); \
         meta.httpEquiv = 'refresh'; \
         if (meta.getAttribute('http-equiv') !== 'refresh') \
             throw new Error('httpEquiv wrote ' + meta.getAttribute('http-equiv'));",
    )
    .expect("camelCase IDL names map to their content attribute");
}

#[test]
fn reflection_lives_on_the_prototype_not_the_instance() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var link = document.getElementById('sheet'); \
         if (Object.prototype.hasOwnProperty.call(link, 'rel')) \
             throw new Error('rel is an own property'); \
         var descriptor = Object.getOwnPropertyDescriptor(HTMLLinkElement.prototype, 'rel'); \
         if (!descriptor || typeof descriptor.get !== 'function') \
             throw new Error('HTMLLinkElement.prototype.rel is not an accessor');",
    )
    .expect("reflected members hang off the interface prototype");
}

#[test]
fn an_anchor_reflects_href_and_target() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var anchor = document.createElement('a'); \
         anchor.setAttribute('href', '../up.html'); \
         if (anchor.href !== 'https://example.test/up.html') \
             throw new Error('href was ' + anchor.href); \
         anchor.target = '_blank'; \
         if (anchor.getAttribute('target') !== '_blank') throw new Error('target');",
    )
    .expect("anchor href resolves and target reflects");
}

#[test]
fn a_script_element_reflects_its_boolean_members() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var script = document.createElement('script'); \
         if (script.async || script.defer || script.noModule) \
             throw new Error('absent booleans are not false'); \
         script.defer = true; \
         script.noModule = true; \
         if (script.getAttribute('defer') !== '') throw new Error('defer'); \
         if (script.getAttribute('nomodule') !== '') throw new Error('nomodule');",
    )
    .expect("script booleans reflect through their content attributes");
}
