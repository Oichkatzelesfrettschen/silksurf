use silksurf_css::{
    AtRuleBlock, CssToken, Rule, TypeSelector, parse_declaration_list, parse_stylesheet,
    parse_stylesheet_bytes,
};
use silksurf_dom::TagName;

#[test]
fn parses_style_rule_declarations() {
    let sheet = parse_stylesheet("body { color: red !important; margin: 0 }").unwrap();
    assert_eq!(sheet.rules.len(), 1);
    let Rule::Style(rule) = &sheet.rules[0] else {
        panic!("expected style rule");
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

    let Rule::At(media) = &sheet.rules[0] else {
        panic!("expected at-rule");
    };
    match &media.block {
        Some(AtRuleBlock::Rules(rules)) => {
            assert_eq!(rules.len(), 1);
        }
        _ => panic!("expected nested rules"),
    }

    let Rule::At(font) = &sheet.rules[1] else {
        panic!("expected at-rule");
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
    let Rule::Style(rule) = &sheet.rules[0] else {
        panic!("expected style rule");
    };
    assert_eq!(rule.selectors.selectors.len(), 2);

    let h1 = &rule.selectors.selectors[0].steps[0].compound;
    assert_eq!(h1.type_selector, Some(TypeSelector::Tag(TagName::H1)));
    assert!(h1.modifiers.is_empty());

    let h2 = &rule.selectors.selectors[1].steps[0].compound;
    assert_eq!(h2.type_selector, Some(TypeSelector::Tag(TagName::H2)));
    assert!(matches!(
        h2.modifiers.first(),
        Some(silksurf_css::SelectorModifier::Class(name)) if name.as_str() == "title"
    ));
}

#[test]
fn parses_inline_declaration_list() {
    let declarations = parse_declaration_list("color: red; display: flex;").unwrap();
    assert_eq!(declarations.len(), 2);
    assert_eq!(declarations[0].name, "color");
    assert_eq!(declarations[0].value, vec![CssToken::Ident("red".into())]);
    assert_eq!(declarations[1].name, "display");
    assert_eq!(declarations[1].value, vec![CssToken::Ident("flex".into())]);
}

#[test]
fn inline_declaration_limit_preserves_utf8_boundary() {
    let multibyte = char::from_u32(0x1f642).expect("valid scalar");
    let prefix = "color: red; background: ";
    let fill_len = (16 * 1024) - prefix.len() - 1;
    let input = format!("{prefix}{}{};", "a".repeat(fill_len), multibyte);

    let declarations = parse_declaration_list(&input).unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].name, "color");
}

#[test]
fn malformed_functional_selector_argument_reaches_eof() {
    let sheet = parse_stylesheet(":where(.) { color: red; }").unwrap();
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn parses_utf16le_stylesheet_bytes_with_bom() {
    let utf16: Vec<u16> = "body { color: red; }".encode_utf16().collect();
    let mut bytes = Vec::with_capacity(2 + utf16.len() * 2);
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for unit in utf16 {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    let sheet = parse_stylesheet_bytes(&bytes).unwrap();
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn honors_charset_label_for_non_utf8_stylesheet_bytes() {
    let mut bytes = br#"@charset "windows-1250";
body { content: ""#
        .to_vec();
    bytes.push(0x8a);
    bytes.extend_from_slice(br#""; }"#);

    let sheet = parse_stylesheet_bytes(&bytes).unwrap();
    let rule = sheet
        .rules
        .iter()
        .find_map(|rule| match rule {
            Rule::Style(rule) => Some(rule),
            Rule::At(_) => None,
        })
        .expect("expected style rule");
    assert_eq!(rule.declarations.len(), 1);
    assert_eq!(rule.declarations[0].name, "content");
    assert_eq!(
        rule.declarations[0].value,
        vec![CssToken::String("\u{0160}".to_string())]
    );
}
