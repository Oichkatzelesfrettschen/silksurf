use silksurf_dom::Dom;
use silksurf_js::SilkContext;
use std::sync::{Arc, Mutex};

fn context() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    dom.append_child(document, html).unwrap();
    dom.append_child(html, body).unwrap();
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn form_collection_tracks_mutation_and_named_lookup() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        document.body.innerHTML = '<form id="f"><input name="imageAttachments"><input type="image"><fieldset disabled></fieldset></form>';
        const form = document.getElementById('f');
        const controls = form.elements;
        check(controls === form.elements, 'same object');
        check(controls instanceof HTMLFormControlsCollection && controls instanceof HTMLCollection, 'interfaces');
        check(controls.length === 2 && form.length === 2, 'listed controls');
        check(controls.namedItem('imageAttachments') === controls[0], 'named item');
        check(controls.imageAttachments === controls[0], 'named property');
        check(controls.namedItem('') === null && controls.item(42) === null && controls[42] === undefined, 'missing');
        const input = document.createElement('input'); input.name = 'late'; form.appendChild(input);
        check(controls.length === 3 && controls[2] === input && '2' in controls, 'live append');
        check(Array.from(controls)[2] === input, 'iterator');
        check(Object.keys(controls).join(',') === '0,1,2', 'enumerable indices');
        input.remove(); check(controls.length === 2 && !('2' in controls), 'live removal');
    "#).expect("live form collection");
}

#[test]
fn form_ownership_tracks_external_references_and_detached_ancestry() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        document.body.innerHTML = '<input id="before" form="f"><form id="f"><input id="inside"><input form="missing"></form><input id="after" form="f">';
        const form = document.getElementById('f'), list = form.elements;
        check(list.length === 3 && list[0].id === 'before' && list[2].id === 'after', 'tree order and ownership');
        const blocker = document.createElement('div'); blocker.id = 'f'; document.body.prepend(blocker);
        check(list.length === 1 && list[0].id === 'inside', 'first matching ID blocks association');
        blocker.remove(); check(list.length === 3, 'association restored');
        form.id = 'other'; check(list.length === 1, 'ID mutation');
        form.remove(); check(list.length === 2, 'detached explicit form uses ancestor');
        const nested = document.createElement('form'), input = document.createElement('input');
        nested.appendChild(input); form.appendChild(nested);
        check(list.length === 2 && nested.elements.length === 1, 'nearest ancestor');
    "#).expect("form owner algorithm");
}

#[test]
fn duplicate_names_return_live_radio_lists_and_preserve_empty_values() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        document.body.innerHTML = '<form id="f"><input type="radio" name="r"><input type="radio" name="r" value=""><input type="radio" name="r" value="x"><output name="r"></output></form>';
        const form = document.getElementById('f'), radios = form.elements.namedItem('r');
        check(radios instanceof RadioNodeList && radios instanceof NodeList && radios.length === 4, 'live list interface');
        radios.value = ''; check(radios[1].checked && radios.value === '', 'empty differs from absent');
        radios.value = 'on'; check(radios[0].checked && !radios[1].checked && radios.value === 'on', 'absent value is on');
        radios.value = 'x'; check(radios[2].checked && !radios[0].checked && radios.value === 'x', 'radio exclusivity');
        radios[2].name = 'other'; check(radios.length === 3 && radios.value === '', 'live rename');
        form.elements[0].remove(); check(radios.length === 2, 'live remove');
        form.elements[0].remove(); check(radios.length === 1 && form.elements.namedItem('r') === radios[0], 'single result');
    "#).expect("RadioNodeList live semantics");
}

#[test]
fn collection_expandos_and_radio_groups_preserve_native_state() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        document.body.innerHTML = '<form id="f"><input id="pick" type="radio" name="x" value="a"><input id="pick" type="radio" name="y" value="b"><input type="radio" name="x" checked></form>';
        const form = document.getElementById('f'), list = form.elements;
        Object.defineProperty(list, 'marker', {value:1}); list.extra = 2;
        check(Object.getOwnPropertyNames(list).includes('marker') && Object.getOwnPropertyDescriptor(list,'marker').value === 1, 'proxy invariant');
        check(Object.keys(list).includes('extra'), 'expando enumeration');
        check(!Reflect.defineProperty(list, '0', {value:42}), 'indexed property readonly');
        list.namedItem('pick').value = 'a'; check(list[0].checked && !list[2].checked, 'complete radio group');
        check(list[0].form === form, 'shared owner');
        list[0].removeAttribute('checked'); check(list[0].checked, 'dirty checkedness');
        list[0].checked = false; list[0].setAttribute('checked',''); check(!list[0].checked, 'dirty state survives content attribute');
        list[0].name = ''; list[1].name = ''; list[1].checked = true; list[0].checked = true;
        check(list[0].checked && list[1].checked, 'empty names remain independent');
    "#).expect("collection reflection and shared checkedness");
}

#[test]
fn empty_form_reference_and_dirty_checkedness_survive_clone_and_group_changes() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        document.body.innerHTML = '<form id=""><input form=""></form><form id="f"><input name="r" type="radio"><input name="r" type="radio" checked></form>';
        const empty = document.body.firstChild;
        check(empty.elements.length === 0 && empty.firstChild.form === null, 'empty ID establishes zero association');
        const controls = document.getElementById('f').elements;
        controls[0].checked = true;
        check(!controls[1].checked, 'group clears sibling');
        controls[1].removeAttribute('checked'); controls[1].setAttribute('checked','');
        check(controls[1].checked && !controls[0].checked, 'untouched sibling remains clean');
        controls[0].checked = true;
        const copy = controls[0].cloneNode(); check(copy.checked, 'clone checkedness');
        copy.setAttribute('checked',''); copy.removeAttribute('checked'); check(copy.checked, 'clone dirty flag');
    "#).expect("form ID and checkedness state");
}
