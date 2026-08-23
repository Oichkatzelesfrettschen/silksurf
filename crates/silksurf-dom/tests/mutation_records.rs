/*
 * The mutation record queue, checked against the paths that reach the tree.
 *
 * The queue sits behind the mutators rather than behind the JS bridge because
 * innerHTML splices through import_subtree and the fragment parser builds
 * through the ordinary constructors; these cases pin that coverage and the two
 * filters that decide what an observer can see.
 */

use silksurf_dom::{Dom, MutationKind, MutationRecord};

/// A document with one connected div, recording open.
fn recording_dom() -> (Dom, silksurf_dom::NodeId, silksurf_dom::NodeId) {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let div = dom.create_element("div");
    dom.append_child(root, div).expect("append div");
    dom.set_mutation_recording(true);
    (dom, root, div)
}

fn kinds(records: &[MutationRecord]) -> Vec<&MutationKind> {
    records.iter().map(|r| &r.kind).collect()
}

#[test]
fn a_document_with_no_observer_queues_nothing() {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let div = dom.create_element("div");
    dom.append_child(root, div).expect("append");
    dom.set_attribute(div, "id", "a").expect("attr");
    assert_eq!(dom.pending_mutation_records(), 0);
    assert!(dom.take_mutation_records().is_empty());
}

#[test]
fn closing_recording_discards_the_queue() {
    let (mut dom, _root, div) = recording_dom();
    dom.set_attribute(div, "id", "a").expect("attr");
    assert_eq!(dom.pending_mutation_records(), 1);
    dom.set_mutation_recording(false);
    assert_eq!(dom.pending_mutation_records(), 0);
}

#[test]
fn an_attribute_write_carries_the_value_it_replaced() {
    let (mut dom, _root, div) = recording_dom();
    dom.set_attribute(div, "id", "first").expect("attr");
    dom.set_attribute(div, "id", "second").expect("attr");
    dom.remove_attribute(div, "id").expect("remove");
    let records = dom.take_mutation_records();
    assert_eq!(
        kinds(&records),
        vec![
            &MutationKind::Attributes {
                name: "id".to_string(),
                old: None
            },
            &MutationKind::Attributes {
                name: "id".to_string(),
                old: Some("first".to_string())
            },
            &MutationKind::Attributes {
                name: "id".to_string(),
                old: Some("second".to_string())
            },
        ]
    );
    assert!(records.iter().all(|r| r.target == div));
}

#[test]
fn a_child_list_record_names_the_siblings_the_node_sat_between() {
    let (mut dom, _root, div) = recording_dom();
    let a = dom.create_element("a");
    let b = dom.create_element("b");
    let c = dom.create_element("c");
    dom.append_child(div, a).expect("a");
    dom.append_child(div, c).expect("c");
    dom.insert_before(div, b, c).expect("b before c");
    dom.remove_child(div, b).expect("remove b");
    let records = dom.take_mutation_records();
    assert_eq!(
        kinds(&records),
        vec![
            &MutationKind::ChildList {
                added: vec![a],
                removed: vec![],
                previous: None,
                next: None
            },
            &MutationKind::ChildList {
                added: vec![c],
                removed: vec![],
                previous: Some(a),
                next: None
            },
            &MutationKind::ChildList {
                added: vec![b],
                removed: vec![],
                previous: Some(a),
                next: Some(c)
            },
            &MutationKind::ChildList {
                added: vec![],
                removed: vec![b],
                previous: Some(a),
                next: Some(c)
            },
        ]
    );
}

#[test]
fn a_subtree_built_before_it_is_spliced_reports_the_splice() {
    // The filters exist for this shape: a page builds a subtree, then attaches
    // it, and an observer sees one addition rather than one per node.
    let (mut dom, _root, div) = recording_dom();
    let outer = dom.create_element("section");
    let inner = dom.create_element("p");
    dom.append_child(outer, inner).expect("inner");
    dom.set_attribute(inner, "class", "x").expect("class");
    assert_eq!(
        dom.pending_mutation_records(),
        0,
        "an unconnected tree is unobserved"
    );
    dom.append_child(div, outer).expect("splice");
    let records = dom.take_mutation_records();
    assert_eq!(
        kinds(&records),
        vec![&MutationKind::ChildList {
            added: vec![outer],
            removed: vec![],
            previous: None,
            next: None
        }]
    );
    // Once the subtree is in the document an observer watching it sees later
    // edits, so a mutation after the splice reports normally.
    dom.set_attribute(inner, "class", "y").expect("class again");
    let after = dom.take_mutation_records();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].target, inner);
}

#[test]
fn suppression_follows_an_added_subtree_to_its_leaves() {
    let (mut dom, _root, div) = recording_dom();
    let a = dom.create_element("a");
    dom.append_child(div, a).expect("a");
    // a is added and suppressed; b under a and c under b must stay suppressed
    // too, or a three-deep splice reports the levels it built after the first.
    let b = dom.create_element("b");
    dom.append_child(a, b).expect("b");
    let c = dom.create_element("c");
    dom.append_child(b, c).expect("c");
    let records = dom.take_mutation_records();
    assert_eq!(
        kinds(&records),
        vec![&MutationKind::ChildList {
            added: vec![a],
            removed: vec![],
            previous: None,
            next: None
        }]
    );
}

#[test]
fn a_take_reopens_the_subtree_for_recording() {
    let (mut dom, _root, div) = recording_dom();
    let child = dom.create_element("span");
    dom.append_child(div, child).expect("append");
    let _ = dom.take_mutation_records();
    dom.set_attribute(child, "id", "later").expect("attr");
    let records = dom.take_mutation_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].target, child);
}

#[test]
fn text_content_records_character_data_on_a_text_node_and_a_child_list_on_an_element() {
    let (mut dom, _root, div) = recording_dom();
    let text = dom.append_text(div, "one").expect("text");
    let _ = dom.take_mutation_records();
    dom.set_text_content(text, "two").expect("edit text");
    let edits = dom.take_mutation_records();
    assert_eq!(
        kinds(&edits),
        vec![&MutationKind::CharacterData {
            old: "one".to_string()
        }]
    );
    assert_eq!(edits[0].target, text);

    dom.set_text_content(div, "replaced")
        .expect("replace children");
    let replace = dom.take_mutation_records();
    assert_eq!(replace.len(), 1);
    let MutationKind::ChildList { added, removed, .. } = &replace[0].kind else {
        panic!("expected a child list record, got {:?}", replace[0].kind);
    };
    assert_eq!(removed, &vec![text]);
    assert_eq!(
        added.len(),
        1,
        "the replacement text node is the one addition"
    );
}

#[test]
fn appending_to_a_trailing_text_node_records_the_data_it_extended() {
    let (mut dom, _root, div) = recording_dom();
    let text = dom.append_text(div, "half").expect("text");
    let _ = dom.take_mutation_records();
    let same = dom.append_text(div, " and half").expect("extend");
    assert_eq!(same, text, "the append merges into the trailing text node");
    let records = dom.take_mutation_records();
    assert_eq!(
        kinds(&records),
        vec![&MutationKind::CharacterData {
            old: "half".to_string()
        }]
    );
}

#[test]
fn import_subtree_reports_one_addition_per_spliced_root() {
    // innerHTML parses into a scratch Dom and splices through import_subtree,
    // which re-creates every node through the ordinary constructors.
    let mut source = Dom::new();
    let source_root = source.create_document();
    let section = source.create_element("section");
    let para = source.create_element("p");
    source.append_child(section, para).expect("para");
    source.append_child(source_root, section).expect("section");

    let (mut dom, _root, div) = recording_dom();
    dom.import_subtree(&source, section, div).expect("import");
    let records = dom.take_mutation_records();
    assert_eq!(
        records.len(),
        1,
        "one splice, not one record per imported node"
    );
    assert_eq!(records[0].target, div);
}
