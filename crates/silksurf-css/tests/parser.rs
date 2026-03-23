use silksurf_css::{AtRuleBlock, CssToken, Rule, TypeSelector, parse_stylesheet};
use silksurf_dom::TagName;

#[test]
fn parses_style_rule_declarations() {
    let sheet = parse_stylesheet("body { color: red !important; margin: 0 }").unwrap();
    assert_eq!(sheet.rules.len(), 1);
    let rule = match &sheet.rules[0] {
        Rule::Style(rule) => rule,
        _ => panic!("expected style rule"),
    };

    assert_eq!(rule.selectors.selectors.len(), 1);
    let selector = &rule.selectors.selectors[0];
    assert_eq!(selector.steps.len(), 1);
    let step = &selector.steps[0];
    assert!(step.combinator.is_none());
    assert_eq!(
        step.compound.type_selector,
        Some(TypeSelector::Tag(TagName::Body))
    );
    assert!(step.compound.modifiers.is_empty());
    assert_eq!(rule.declarations.len(), 2);

    let color = &rule.declarations[0];
    assert_eq!(color.name, "color");
    assert_eq!(color.value, vec![CssToken::Ident("red".into())]);
    assert!(color.important);

    let margin = &rule.declarations[1];
    assert_eq!(margin.name, "margin");
    assert_eq!(margin.value, vec![CssToken::Number("0".into())]);
    assert!(!margin.important);
}

#[test]
fn parses_at_rule_blocks() {
    let sheet = parse_stylesheet(
        "@media screen { body { color: red; } } @font-face { font-family: Test; }",
    )
    .unwrap();

    assert_eq!(sheet.rules.len(), 2);

    let media = match &sheet.rules[0] {
        Rule::At(rule) => rule,
        _ => panic!("expected at-rule"),
    };
    match &media.block {
        Some(AtRuleBlock::Rules(rules)) => {
            assert_eq!(rules.len(), 1);
        }
        _ => panic!("expected nested rules"),
    }

    let font = match &sheet.rules[1] {
        Rule::At(rule) => rule,
        _ => panic!("expected at-rule"),
    };
    match &font.block {
        Some(AtRuleBlock::Declarations(decls)) => {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "font-family");
        }
        _ => panic!("expected declarations block"),
    }
}

#[test]
fn parses_multiple_selectors() {
    let sheet = parse_stylesheet("h1, h2.title { margin: 0; }").unwrap();
    assert_eq!(sheet.rules.len(), 1);
    let rule = match &sheet.rules[0] {
        Rule::Style(rule) => rule,
        _ => panic!("expected style rule"),
    };
    assert_eq!(rule.selectors.selectors.len(), 2);

    let h1 = &rule.selectors.selectors[0].steps[0].compound;
    assert_eq!(h1.type_selector, Some(TypeSelector::Tag(TagName::H1)));
    assert!(h1.modifiers.is_empty());

    let h2 = &rule.selectors.selectors[1].steps[0].compound;
    assert_eq!(h2.type_selector, Some(TypeSelector::Tag(TagName::H2)));
    assert!(matches!(
        h2.modifiers.get(0),
        Some(silksurf_css::SelectorModifier::Class(name)) if name.as_str() == "title"
    ));
}
