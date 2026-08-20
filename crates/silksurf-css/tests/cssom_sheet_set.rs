//! SheetSet: the document's sheets addressed and spliced the way CSSOM does.

use silksurf_css::{LiveSheet, SheetError, SheetOrigin, SheetSet, StyleIndex, parse_stylesheet};

fn set_with(source: &str) -> SheetSet {
    let mut set = SheetSet::new();
    set.replace(vec![LiveSheet::new(
        SheetOrigin::Author,
        parse_stylesheet(source).expect("fixture parses"),
    )]);
    set
}

#[test]
fn an_inserted_rule_reaches_the_cascade() {
    let mut set = set_with("p { color: red }");
    let before = StyleIndex::for_viewport_sheets(set.active_sheets(), 1280.0, 800.0);
    assert_eq!(before.active_rules.len(), 1);
    let index = set
        .insert_rule(0, ".chip { color: blue }", set.rule_count(0))
        .expect("the rule parses and splices");
    assert_eq!(index, 1, "the rule takes the index it was asked for");
    let after = StyleIndex::for_viewport_sheets(set.active_sheets(), 1280.0, 800.0);
    assert_eq!(
        after.active_rules.len(),
        2,
        "the spliced rule is an active rule"
    );
}

#[test]
fn a_scripted_splice_moves_the_generation() {
    let mut set = set_with("p { color: red }");
    let before = set.script_generation();
    set.insert_rule(0, "a { color: blue }", 0).expect("splices");
    assert!(
        set.script_generation() > before,
        "insertRule moves no DOM generation, so the set carries its own"
    );
}

#[test]
fn an_index_past_the_end_is_refused() {
    let mut set = set_with("p { color: red }");
    assert_eq!(
        set.insert_rule(0, "a { color: blue }", 2),
        Err(SheetError::IndexSize)
    );
    assert_eq!(set.delete_rule(0, 1), Err(SheetError::IndexSize));
}

#[test]
fn text_that_is_not_one_rule_is_refused() {
    let mut set = set_with("p { color: red }");
    assert_eq!(
        set.insert_rule(0, "a { color: blue } b { color: red }", 0),
        Err(SheetError::Syntax)
    );
    assert_eq!(set.insert_rule(0, "not a rule", 0), Err(SheetError::Syntax));
}

#[test]
fn a_deleted_rule_leaves_the_cascade() {
    let mut set = set_with("p { color: red } a { color: blue }");
    set.delete_rule(0, 0).expect("deletes");
    let index = StyleIndex::for_viewport_sheets(set.active_sheets(), 1280.0, 800.0);
    assert_eq!(index.active_rules.len(), 1);
    assert_eq!(set.selector_text(0, 0).as_deref(), Some("a"));
}

#[test]
fn a_disabled_sheet_contributes_no_rules() {
    let mut set = set_with("p { color: red }");
    set.set_disabled(0, true).expect("the sheet exists");
    let index = StyleIndex::for_viewport_sheets(set.active_sheets(), 1280.0, 800.0);
    assert!(
        index.active_rules.is_empty(),
        "a disabled sheet stays in the list and out of the cascade"
    );
    assert_eq!(set.len(), 1, "and stays enumerable");
}

#[test]
fn the_user_agent_sheet_stays_out_of_the_scripted_list() {
    let mut set = SheetSet::new();
    set.replace(vec![
        LiveSheet::new(
            SheetOrigin::UserAgent,
            parse_stylesheet("div { display: block }").expect("ua parses"),
        ),
        LiveSheet::new(
            SheetOrigin::Author,
            parse_stylesheet("p { color: red }").expect("author parses"),
        ),
    ]);
    assert_eq!(set.author_indices(), vec![1]);
    let index = StyleIndex::for_viewport_sheets(set.active_sheets(), 1280.0, 800.0);
    assert_eq!(
        index.active_rules.len(),
        2,
        "the cascade still reads both sheets"
    );
}

#[test]
fn a_rule_serializes_back_through_the_set() {
    let set = set_with("a.link > span { color: red; margin: 0 auto }");
    assert_eq!(
        set.rule_text(0, 0).as_deref(),
        Some("a.link > span { color: red; margin: 0 auto; }")
    );
    assert_eq!(set.selector_text(0, 0).as_deref(), Some("a.link > span"));
    assert_eq!(
        set.declaration_text(0, 0).as_deref(),
        Some("color: red; margin: 0 auto;")
    );
}
