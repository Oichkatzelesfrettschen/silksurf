use silksurf_js::SilkContext;
use std::sync::{Arc, Mutex};

fn context() -> SilkContext {
    let dom = silksurf_html::parse_html(
        "<!doctype html><body><template id='sample'><p id='inert'>one &amp; two</p><template><b>nested</b></template></template><div id='target'></div>",
    );
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn parsed_template_content_is_inert_and_has_stable_fragment_identity() {
    context().eval(r"
        var template = document.getElementById('sample');
        var content = template.content;
        if (content !== template.content) throw new Error('content identity');
        if (!(content instanceof DocumentFragment) || content.nodeType !== 11) throw new Error('fragment identity');
        if (content.parentNode !== null || content.isConnected) throw new Error('inert parent');
        if (template.childNodes.length || template.textContent !== '') throw new Error('ordinary children');
        if (document.getElementById('inert') !== null) throw new Error('document query reaches inert content');
        if (content.querySelector('#inert').textContent !== 'one & two') throw new Error('fragment query');
        if (content.querySelector('b') !== null) throw new Error('query reaches nested content');
        if (content.querySelector('template').content.querySelector('b').textContent !== 'nested') throw new Error('nested template');
    ").expect("template ownership");
}

#[test]
fn template_clone_and_inner_html_preserve_markup_and_nested_content() {
    context().eval(r#"
        var template = document.getElementById('sample');
        var markup = '<p id="inert">one &amp; two</p><template><b>nested</b></template>';
        if (template.innerHTML !== markup) throw new Error(template.innerHTML);
        var copy = template.cloneNode(true);
        if (copy.innerHTML !== markup || copy.content === template.content) throw new Error('deep clone');
        if (template.cloneNode(false).content.childNodes.length) throw new Error('shallow clone');
        copy.innerHTML = '<span title="&quot;&amp;">a&lt;b</span><br><script>if (a < b) x &= 1;</script>';
        if (copy.innerHTML !== '<span title="&quot;&amp;">a&lt;b</span><br><script>if (a < b) x &= 1;</script>') throw new Error(copy.innerHTML);
        if (copy.childNodes.length || copy.content.childNodes.length !== 3) throw new Error('innerHTML destination');
        if (template.innerHTML !== markup) throw new Error('clone aliases source');
    "#).expect("template clone and serialization");
}

#[test]
fn fragment_insertion_moves_children_and_validation_preserves_source() {
    context().eval(r"
        var fragment = document.createDocumentFragment();
        if (fragment.nodeType !== 11 || fragment.nodeName !== '#document-fragment') throw new Error('fragment type');
        var first = document.createElement('span');
        var second = document.createTextNode('two');
        fragment.appendChild(first);
        fragment.appendChild(second);
        var target = document.getElementById('target');
        var stranger = document.createElement('p');
        var threw = false;
        try { target.insertBefore(fragment, stranger); } catch (error) { threw = true; }
        if (!threw || fragment.childNodes.length !== 2) throw new Error('failed insertion mutates source');
        if (target.appendChild(fragment) !== fragment) throw new Error('append return');
        if (fragment.childNodes.length || fragment.parentNode !== null) throw new Error('fragment attachment');
        if (target.firstChild !== first || target.lastChild !== second) throw new Error('child order');
        threw = false;
        try { first.appendChild(target); } catch (error) { threw = true; }
        if (!threw || first.parentNode !== target) throw new Error('cycle validation');
        if (target.replaceChild(first, first) !== first || first.parentNode !== target) throw new Error('self replacement');
    ").expect("fragment insertion");
}

#[test]
fn serialization_excludes_void_descendants() {
    context()
        .eval(
            r"
        var parent = document.createElement('div');
        var br = document.createElement('br');
        br.appendChild(document.createTextNode('hidden'));
        br.appendChild(document.createComment('hidden'));
        parent.appendChild(br);
        if (parent.innerHTML !== '<br>') throw new Error(parent.innerHTML);
    ",
        )
        .expect("namespace-aware void serialization");
}
