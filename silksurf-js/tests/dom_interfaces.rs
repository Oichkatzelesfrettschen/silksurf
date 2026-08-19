//! Node wrappers as IDL objects: prototype chain, instanceof, and the members
//! that hang off `Element.prototype` and `Node.prototype`.

use std::sync::{Arc, Mutex};

use silksurf_dom::Dom;
use silksurf_js::SilkContext;

fn context_with_document() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let div = dom.create_element("div");
    dom.set_attribute(div, "id", "target").expect("id attaches");
    dom.set_attribute(div, "class", "one two")
        .expect("class attaches");
    let link = dom.create_element("link");
    dom.set_attribute(link, "id", "sheet").expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(body, div).expect("div attaches");
    dom.append_child(body, link).expect("link attaches");
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn an_element_is_an_instance_of_its_interface_chain() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var div = document.getElementById('target'); \
         if (!(div instanceof HTMLDivElement)) throw new Error('HTMLDivElement'); \
         if (!(div instanceof HTMLElement)) throw new Error('HTMLElement'); \
         if (!(div instanceof Element)) throw new Error('Element'); \
         if (!(div instanceof Node)) throw new Error('Node'); \
         if (!(div instanceof EventTarget)) throw new Error('EventTarget'); \
         if (div instanceof HTMLLinkElement) throw new Error('wrong interface');",
    )
    .expect("a div walks the HTMLDivElement chain");
}

#[test]
fn the_link_element_interface_resolves() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var link = document.getElementById('sheet'); \
         if (!(link instanceof HTMLLinkElement)) throw new Error('HTMLLinkElement'); \
         if (Object.getPrototypeOf(link) !== HTMLLinkElement.prototype) \
             throw new Error('prototype mismatch');",
    )
    .expect("a link element reports HTMLLinkElement");
}

#[test]
fn prototype_patching_reaches_every_element() {
    let mut ctx = context_with_document();
    ctx.eval(
        "Element.prototype.silksurfProbe = function () { return this.id; }; \
         if (document.getElementById('target').silksurfProbe() !== 'target') \
             throw new Error('patched method did not resolve');",
    )
    .expect("a patched prototype member reaches instances");
}

#[test]
fn attribute_members_read_and_write_the_dom() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var div = document.getElementById('target'); \
         if (!div.hasAttribute('id')) throw new Error('hasAttribute'); \
         if (div.hasAttribute('missing')) throw new Error('false positive'); \
         if (div.getAttributeNames().indexOf('class') === -1) throw new Error('names'); \
         div.setAttribute('data-x', '1'); \
         if (!div.hasAttribute('data-x')) throw new Error('setAttribute'); \
         div.removeAttribute('data-x'); \
         if (div.hasAttribute('data-x')) throw new Error('removeAttribute'); \
         if (div.toggleAttribute('hidden') !== true) throw new Error('toggle on'); \
         if (div.toggleAttribute('hidden') !== false) throw new Error('toggle off');",
    )
    .expect("attribute members operate on the live DOM");
}

#[test]
fn class_list_is_a_live_view_over_class_name() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var div = document.getElementById('target'); \
         if (div.classList.length !== 2) throw new Error('length ' + div.classList.length); \
         if (!div.classList.contains('two')) throw new Error('contains'); \
         div.classList.add('three'); \
         if (div.className !== 'one two three') throw new Error(div.className); \
         div.classList.remove('one'); \
         if (div.className !== 'two three') throw new Error(div.className); \
         if (div.classList.toggle('two') !== false) throw new Error('toggle off'); \
         div.className = 'fresh'; \
         if (div.classList.length !== 1 || !div.classList.contains('fresh')) \
             throw new Error('className write not observed'); \
         if (div.classList.replace('fresh', 'newer') !== true) throw new Error('replace'); \
         if (div.className !== 'newer') throw new Error(div.className);",
    )
    .expect("classList mirrors className in both directions");
}

#[test]
fn tree_members_walk_elements_and_text_alike() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var body = document.body; \
         if (body.childElementCount !== 2) throw new Error('count ' + body.childElementCount); \
         if (body.firstElementChild.id !== 'target') throw new Error('firstElementChild'); \
         if (body.lastElementChild.id !== 'sheet') throw new Error('lastElementChild'); \
         var div = document.getElementById('target'); \
         if (div.nextElementSibling.id !== 'sheet') throw new Error('nextElementSibling'); \
         if (div.previousElementSibling !== null) throw new Error('previousElementSibling'); \
         if (div.parentElement !== body) throw new Error('parentElement'); \
         if (!body.contains(div)) throw new Error('contains'); \
         if (div.contains(body)) throw new Error('contains reversed'); \
         if (!div.isConnected) throw new Error('isConnected'); \
         if (div.localName !== 'div') throw new Error(div.localName);",
    )
    .expect("tree members report the document structure");
}

#[test]
fn clone_node_copies_attributes_and_the_subtree_on_demand() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var div = document.getElementById('target'); \
         div.appendChild(document.createElement('span')); \
         var shallow = div.cloneNode(false); \
         if (shallow.getAttribute('class') !== 'one two') throw new Error('attributes'); \
         if (shallow.childNodes.length !== 0) throw new Error('shallow copied children'); \
         if (shallow === div) throw new Error('same node'); \
         var deep = div.cloneNode(true); \
         if (deep.childNodes.length !== 1) throw new Error('deep child count'); \
         if (deep.isConnected) throw new Error('clone is attached');",
    )
    .expect("cloneNode honours the deep flag");
}

#[test]
fn insertion_helpers_place_nodes_around_a_reference() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var div = document.getElementById('target'); \
         var before = document.createElement('i'); \
         var after = document.createElement('b'); \
         div.before(before); div.after(after); \
         if (document.body.children[0].tagName.toLowerCase() !== 'i') throw new Error('before'); \
         if (div.nextElementSibling.tagName.toLowerCase() !== 'b') throw new Error('after'); \
         div.remove(); \
         if (document.getElementById('target') !== null) throw new Error('remove'); \
         after.replaceWith(document.createElement('u')); \
         if (document.body.querySelector('b') !== null) throw new Error('replaceWith');",
    )
    .expect("before, after, remove, and replaceWith rewire the tree");
}

#[test]
fn document_creates_comments_and_fragments() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var comment = document.createComment('note'); \
         if (comment.nodeType !== 8) throw new Error('nodeType ' + comment.nodeType); \
         if (!(comment instanceof Comment)) throw new Error('Comment interface'); \
         if (comment.data !== 'note') throw new Error(comment.data); \
         var fragment = document.createDocumentFragment(); \
         if (!fragment) throw new Error('createDocumentFragment');",
    )
    .expect("createComment and createDocumentFragment produce nodes");
}

#[test]
fn document_exposes_class_lookups_and_the_default_view() {
    let mut ctx = context_with_document();
    ctx.eval(
        "if (document.getElementsByClassName('one').length !== 1) throw new Error('byClassName'); \
         if (document.getElementsByClassName('one two').length !== 1) throw new Error('multi'); \
         if (document.defaultView !== globalThis) throw new Error('defaultView'); \
         if (document.visibilityState !== 'visible') throw new Error('visibilityState'); \
         if (document.contentType !== 'text/html') throw new Error('contentType');",
    )
    .expect("document lookups and view metadata resolve");
}

#[test]
fn the_event_constructor_builds_a_dispatchable_event() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var event = new Event('ping', { bubbles: true, cancelable: true }); \
         if (event.type !== 'ping') throw new Error('type'); \
         if (!event.bubbles) throw new Error('bubbles'); \
         event.preventDefault(); \
         if (!event.defaultPrevented) throw new Error('preventDefault'); \
         var custom = new CustomEvent('pong', { detail: { n: 1 } }); \
         if (custom.detail.n !== 1) throw new Error('detail'); \
         if (!(custom instanceof Event)) throw new Error('CustomEvent chain'); \
         globalThis.seen = 0; \
         var div = document.getElementById('target'); \
         div.addEventListener('ping', function () { seen += 1; }); \
         div.dispatchEvent(new Event('ping', { bubbles: true })); \
         if (seen !== 1) throw new Error('constructed event did not dispatch');",
    )
    .expect("Event and CustomEvent construct and dispatch");
}

#[test]
fn a_node_collection_answers_both_indexing_and_item() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var list = document.querySelectorAll('div'); \
         if (list.length !== 1) throw new Error('length ' + list.length); \
         if (list.item(0) !== list[0]) throw new Error('item disagrees with indexing'); \
         if (list.item(5) !== null) throw new Error('out of range'); \
         if (!(list instanceof NodeList)) throw new Error('NodeList chain'); \
         if (!Array.isArray(list)) throw new Error('array indexing lost'); \
         if (document.querySelectorAll('nothing').item(0) !== null) throw new Error('empty list'); \
         if (document.body.children.item(0).id !== 'target') throw new Error('children.item');",
    )
    .expect("collections carry item() beside array behavior");
}

#[test]
fn get_bounding_client_rect_returns_a_complete_box() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var rect = document.getElementById('target').getBoundingClientRect(); \
         var keys = ['x', 'y', 'width', 'height', 'top', 'left', 'right', 'bottom']; \
         for (var i = 0; i < keys.length; i++) { \
             if (typeof rect[keys[i]] !== 'number') throw new Error('missing ' + keys[i]); \
         }",
    )
    .expect("getBoundingClientRect reports every edge");
}
