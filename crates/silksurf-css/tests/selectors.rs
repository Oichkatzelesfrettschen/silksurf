use silksurf_css::{
    parse_selector_list, AttributeOperator, Combinator, CssTokenizer, SelectorModifier, TypeSelector,
};
use silksurf_dom::{AttributeName, TagName};

#[test]
fn parses_combinators_and_modifiers() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(".item > #main + a:hover").unwrap();
    tokens.extend(tokenizer.finish().unwrap());
    let list = parse_selector_list(tokens);

    assert_eq!(list.selectors.len(), 1);
    let selector = &list.selectors[0];
    assert_eq!(selector.steps.len(), 3);

    let first = &selector.steps[0];
    assert!(first.combinator.is_none());
    assert!(matches!(
        first.compound.modifiers.get(0),
        Some(SelectorModifier::Class(name)) if name.as_str() == "item"
    ));

    let second = &selector.steps[1];
    assert_eq!(second.combinator, Some(Combinator::Child));
    assert!(matches!(
        second.compound.modifiers.get(0),
        Some(SelectorModifier::Id(name)) if name.as_str() == "main"
    ));

    let third = &selector.steps[2];
    assert_eq!(third.combinator, Some(Combinator::NextSibling));
    assert_eq!(
        third.compound.type_selector,
        Some(TypeSelector::Tag(TagName::A))
    );
    assert!(matches!(
        third.compound.modifiers.get(0),
        Some(SelectorModifier::PseudoClass(name)) if name.as_str() == "hover"
    ));
}

#[test]
fn parses_attribute_selector() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("input[type=\"text\"]").unwrap();
    tokens.extend(tokenizer.finish().unwrap());
    let list = parse_selector_list(tokens);

    assert_eq!(list.selectors.len(), 1);
    let selector = &list.selectors[0];
    let step = &selector.steps[0];
    assert_eq!(
        step.compound.type_selector,
        Some(TypeSelector::Tag(TagName::Input))
    );

    let attr = step
        .compound
        .modifiers
        .iter()
        .find_map(|modifier| match modifier {
            SelectorModifier::Attribute(attr) => Some(attr),
            _ => None,
        })
        .expect("attribute selector");

    assert_eq!(attr.name, AttributeName::Type);
    assert_eq!(attr.operator, Some(AttributeOperator::Equals));
    assert_eq!(attr.value.as_ref().map(|value| value.as_str()), Some("text"));
}

#[test]
fn parses_attribute_operator_variants() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer
        .feed("a[href^=https][rel~=nofollow][lang|=en][title*=hero][name$=id]")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());
    let list = parse_selector_list(tokens);

    let selector = &list.selectors[0];
    let step = &selector.steps[0];
    let mut ops = step
        .compound
        .modifiers
        .iter()
        .filter_map(|modifier| match modifier {
            SelectorModifier::Attribute(attr) => Some((attr.name.as_str(), &attr.operator)),
            _ => None,
        })
        .collect::<Vec<_>>();
    ops.sort_by_key(|(name, _)| *name);

    assert_eq!(ops.len(), 5);
    assert_eq!(ops[0].0, "href");
    assert_eq!(ops[0].1, &Some(AttributeOperator::PrefixMatch));
    assert_eq!(ops[1].0, "lang");
    assert_eq!(ops[1].1, &Some(AttributeOperator::DashMatch));
    assert_eq!(ops[2].0, "name");
    assert_eq!(ops[2].1, &Some(AttributeOperator::SuffixMatch));
    assert_eq!(ops[3].0, "rel");
    assert_eq!(ops[3].1, &Some(AttributeOperator::Includes));
    assert_eq!(ops[4].0, "title");
    assert_eq!(ops[4].1, &Some(AttributeOperator::SubstringMatch));
}

#[test]
fn parses_descendant_combinator() {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed("div span").unwrap();
    tokens.extend(tokenizer.finish().unwrap());
    let list = parse_selector_list(tokens);

    let selector = &list.selectors[0];
    assert_eq!(selector.steps.len(), 2);
    assert!(selector.steps[0].combinator.is_none());
    assert_eq!(selector.steps[1].combinator, Some(Combinator::Descendant));
}
